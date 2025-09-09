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
    // Get server version (for compatibility checks)
    let server_version = match gamiscreen_shared::api::rest::server_version(&base).await {
        Ok(v) => v.version,
        Err(e) => {
            warn!(error=%e, "update: failed to fetch server version; skipping update");
            return Ok(());
        }
    };
    let server_version = match Version::parse(&server_version) {
        Ok(v) => v,
        Err(_) => {
            warn!("update: server version is not a valid semver; skipping update");
            return Ok(());
        }
    };

    let current_version =
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0));
    // Find newest compatible client release on GitHub
    let repo = "lklimek/gamiscreen";
    let gh = reqwest::Client::builder()
        .user_agent("gamiscreen-client-updater")
        .build()
        .map_err(|e| AppError::Http(e.to_string()))?;
    let url = format!("https://api.github.com/repos/{}/releases", repo);
    let releases: Vec<serde_json::Value> = gh
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Http(e.to_string()))?
        .json()
        .await
        .map_err(|e| AppError::Http(e.to_string()))?;

    // Collect candidate versions with assets
    let mut candidates: Vec<(Version, Vec<Asset>)> = Vec::new();
    for rel in releases.into_iter() {
        // Exclude GitHub prerelease and draft entries outright
        if rel
            .get("prerelease")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            continue;
        }
        if rel.get("draft").and_then(|v| v.as_bool()).unwrap_or(false) {
            continue;
        }
        let tag = rel.get("tag_name").and_then(|v| v.as_str()).unwrap_or("");
        let ver_str = tag.trim_start_matches('v');
        let Ok(ver) = Version::parse(ver_str) else {
            continue;
        };
        // Also exclude semantic pre-release tags like 1.2.3-beta.1
        if !ver.pre.is_empty() {
            continue;
        }
        // Compatibility rule:
        // - Stable (>=1): same major as server, and not newer than server
        // - Pre-1.0 (0.y.z): same 0.minor as server, and not newer than server
        if !is_compatible(&ver, &server_version) || ver <= current_version {
            continue;
        }
        let assets = rel
            .get("assets")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out_assets: Vec<Asset> = Vec::new();
        for a in assets.iter() {
            let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if !name.starts_with("gamiscreen-client") || name.ends_with(".sha256") {
                continue;
            }
            let url = a
                .get("browser_download_url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if url.is_empty() {
                continue;
            }

            // Expect sibling sha256 asset
            let sha_name = format!("{}.sha256", name);
            let mut sha256 = String::new();
            for b in assets.iter() {
                if b.get("name").and_then(|v| v.as_str()) == Some(sha_name.as_str())
                    && let Some(url2) = b.get("browser_download_url").and_then(|v| v.as_str())
                {
                    if let Ok(resp) = gh.get(url2).send().await {
                        if let Ok(text) = resp.text().await {
                            sha256 = text.split_whitespace().next().unwrap_or("").to_string();
                        }
                    }
                }
            }
            if sha256.is_empty() {
                continue;
            }

            // Map name to os/arch
            let (os, arch) = map_os_arch(name);
            if os.is_empty() || arch.is_empty() {
                continue;
            }
            out_assets.push(Asset {
                os,
                arch,
                url,
                sha256,
            });
        }
        if !out_assets.is_empty() {
            candidates.push((ver, out_assets));
        }
    }
    if candidates.is_empty() {
        info!(%current_version, %server_version, "update: no compatible releases found");
        return Ok(());
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    let (target_version, assets) = candidates.remove(0);

    // Choose artifact by OS/arch
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let art = match assets.into_iter().find(|a| a.os == os && a.arch == arch) {
        Some(a) => a,
        None => {
            warn!(%os, %arch, %target_version, "update: no artifact for this platform");
            return Ok(());
        }
    };

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

#[derive(Clone)]
struct Asset {
    os: String,
    arch: String,
    url: String,
    sha256: String,
}

fn map_os_arch(name: &str) -> (String, String) {
    let lname = name.to_lowercase();
    let mut os = String::new();
    let mut arch = String::new();
    if lname.contains("linux") {
        os = "linux".into();
    } else if lname.contains("windows") || lname.contains("win32") || lname.ends_with(".exe") {
        os = "windows".into();
    } else if lname.contains("darwin") || lname.contains("macos") || lname.contains("apple") {
        os = "macos".into();
    }
    if lname.contains("x86_64") || lname.contains("amd64") {
        arch = "x86_64".into();
    } else if lname.contains("aarch64") || lname.contains("arm64") {
        arch = "aarch64".into();
    } else if lname.contains("armv7") || lname.contains("armv7l") {
        arch = "armv7".into();
    }
    (os, arch)
}

fn is_compatible(client: &Version, server: &Version) -> bool {
    if server.major == 0 {
        client.major == 0 && client.minor == server.minor && client <= server
    } else {
        client.major == server.major && client <= server
    }
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
