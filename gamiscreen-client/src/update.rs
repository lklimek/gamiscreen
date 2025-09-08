use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;

use semver::Version;
use sha2::{Digest, Sha256};
use tokio::process::Command;
use tracing::{info, warn};

use crate::{AppError, ClientConfig};

pub async fn maybe_self_update(cfg: &ClientConfig) -> Result<(), AppError> {
    let base = crate::config::normalize_server_url(&cfg.server_url);
    // Fetch manifest (public)
    let manifest = match gamiscreen_shared::api::rest::update_manifest(&base).await {
        Ok(m) => m,
        Err(e) => {
            warn!(error=%e, "update: failed to fetch manifest; continuing with current binary");
            return Ok(());
        }
    };

    let current_version =
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0));
    let latest_version = match Version::parse(&manifest.latest_version) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    if latest_version <= current_version {
        info!(%current_version, %latest_version, "update: current version is up to date");
        return Ok(());
    }

    // Choose artifact by OS/arch
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let art = match manifest
        .artifacts
        .into_iter()
        .find(|a| a.os == os && a.arch == arch)
    {
        Some(a) => a,
        None => {
            warn!(%os, %arch, "update: no artifact for this platform");
            return Ok(());
        }
    };
    if art.sha256.is_empty() {
        warn!("update: manifest missing sha256 for selected artifact; skipping update");
        return Ok(());
    }

    // Download to a temporary path next to the current executable
    let exe = std::env::current_exe().map_err(AppError::Io)?;
    let parent = exe.parent().unwrap_or_else(|| std::path::Path::new("."));
    let tmp_path = parent.join(format!(
        ".{}.download-{}",
        exe.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("gamiscreen-client"),
        std::process::id()
    ));

    info!(url=%art.url, path=%tmp_path.display(), "update: downloading new binary");
    match download_to_file(&art.url, &tmp_path).await {
        Ok(_) => {}
        Err(e) => {
            warn!(error=%e, "update: download failed");
            let _ = std::fs::remove_file(&tmp_path);
            return Ok(());
        }
    }

    // Verify sha256
    match sha256_file_hex(&tmp_path) {
        Ok(sum) => {
            if !eq_hex_constant_time(&sum, &art.sha256) {
                warn!(expected=%art.sha256, actual=%sum, "update: sha256 mismatch; aborting");
                let _ = std::fs::remove_file(&tmp_path);
                return Ok(());
            }
        }
        Err(e) => {
            warn!(error=%e, "update: failed to compute sha256");
            let _ = std::fs::remove_file(&tmp_path);
            return Ok(());
        }
    }

    // Preserve permissions from current exe
    if let Ok(meta) = std::fs::metadata(&exe) {
        let _ = std::fs::set_permissions(&tmp_path, meta.permissions());
    }

    // Atomic replace
    info!(from=%tmp_path.display(), to=%exe.display(), "update: replacing binary");
    if let Err(e) = std::fs::rename(&tmp_path, &exe) {
        warn!(error=%e, "update: failed to replace binary (insufficient permissions?)");
        let _ = std::fs::remove_file(&tmp_path);
        return Ok(());
    }

    // Restart: exec the same binary with same args
    info!("update: restart into new binary");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let _ = Command::new(&exe).args(args).spawn();
    std::process::exit(0);
}

async fn download_to_file(url: &str, path: &PathBuf) -> Result<(), AppError> {
    let client = reqwest::Client::new();
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Http(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(AppError::Http(format!("download status {}", resp.status())));
    }
    let mut f = File::create(path).map_err(AppError::Io)?;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| AppError::Http(e.to_string()))?
    {
        f.write_all(&chunk).map_err(AppError::Io)?;
    }
    Ok(())
}

fn sha256_file_hex(path: &PathBuf) -> Result<String, AppError> {
    let mut f = File::open(path).map_err(AppError::Io)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(AppError::Io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let sum = hasher.finalize();
    Ok(hex::encode(sum))
}

fn eq_hex_constant_time(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.bytes().zip(b.bytes()) {
        diff |= x ^ y;
    }
    diff == 0
}
