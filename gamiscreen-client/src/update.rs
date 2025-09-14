use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use tempfile::NamedTempFile;

use semver::Version;
use sha2::{Digest, Sha256};
// no tokio::process here; restart uses std::process with OS-specific exec/spawn
use crate::platform;
use tracing::{info, warn};

use crate::{AppError, ClientConfig};

pub async fn maybe_self_update(cfg: &ClientConfig) -> Result<(), AppError> {
    let base = crate::config::normalize_server_url(&cfg.server_url);
    let Some(server_version) = fetch_server_version(&base).await else {
        return Ok(());
    };

    let current_version =
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0));
    let gh = http_client()?;
    let releases = fetch_releases(&gh, "lklimek/gamiscreen").await?;
    let candidates = collect_candidates(&gh, releases, &server_version, &current_version).await;
    let Some((ver, art)) = select_asset(candidates) else {
        info!(%current_version, %server_version, "update: no compatible releases found");
        return Ok(());
    };

    info!(%current_version, %server_version,new_version=%ver, "update: found compatible release, proceeding to update");

    let exe = std::env::current_exe().map_err(AppError::Io)?;
    let parent = exe.parent().unwrap_or_else(|| std::path::Path::new("."));

    let tmp = download_artifact(&art, parent).await?;
    if !verify_sha256(tmp.path(), &art.sha256)? {
        warn!(expected=%art.sha256, "update: sha256 mismatch; aborting");
        return Ok(());
    }

    let mut extracted_opt: Option<NamedTempFile> = None;
    if is_zip_asset(&art) {
        extracted_opt = Some(extract_zip_to_temp(tmp.path(), parent)?);
    }
    // Stage the new binary at a stable path next to the current exe
    let staged_path = parent.join(format!(".gamiscreen-update-{}", std::process::id()));
    info!(from=?tmp.path().display(), staged=?staged_path.display(), "update: staging new binary");
    // Persist and immediately drop any open File handle to avoid ETXTBUSY during exec
    let persist_res = if let Some(f) = extracted_opt {
        f.persist(&staged_path)
    } else {
        tmp.persist(&staged_path)
    };
    match persist_res {
        Ok(_file) => {
            // drop file handle here
        }
        Err(e) => {
            warn!(error=%e, "update: failed to stage new binary");
            return Ok(());
        }
    }
    // Preserve permissions from current exe for the staged file
    if let Ok(meta) = std::fs::metadata(&exe) {
        let _ = std::fs::set_permissions(&staged_path, meta.permissions());
    }
    // Delegate OS-specific replace + restart to Platform
    let plat = match platform::detect_default().await {
        Ok(p) => p,
        Err(e) => {
            warn!(error=%e, "update: failed to detect platform for restart");
            return Ok(());
        }
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    plat.replace_and_restart(&staged_path, &exe, &args);
}

#[derive(Clone)]
struct Asset {
    name: String,
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

async fn download_to_file(url: &str, path: &Path) -> Result<(), AppError> {
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

fn sha256_file_hex(path: &Path) -> Result<String, AppError> {
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

fn extract_single_from_zip(zip_path: &Path, out_path: &Path) -> Result<(), AppError> {
    let f = File::open(zip_path).map_err(AppError::Io)?;
    let mut zip = zip::ZipArchive::new(f)
        .map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("zip open error: {}", e))
        })
        .map_err(AppError::Io)?;
    if zip.len() == 0 {
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "zip archive empty",
        )));
    }
    // Find first non-directory entry index
    let mut chosen_idx: Option<usize> = None;
    for i in 0..zip.len() {
        let f = zip
            .by_index(i)
            .map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, format!("zip entry error: {}", e))
            })
            .map_err(AppError::Io)?;
        if !f.is_dir() {
            chosen_idx = Some(i);
            break;
        }
        // `f` drops here before next iteration
    }
    let Some(idx) = chosen_idx else {
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "no file entries in zip",
        )));
    };
    let mut file = zip
        .by_index(idx)
        .map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("zip entry error: {}", e))
        })
        .map_err(AppError::Io)?;

    // Write to out_path
    let mut out = File::create(out_path).map_err(AppError::Io)?;
    std::io::copy(&mut file, &mut out).map_err(AppError::Io)?;
    Ok(())
}

// OS-specific restart logic moved into Platform::replace_and_restart

async fn fetch_server_version(base: &str) -> Option<Version> {
    match gamiscreen_shared::api::rest::server_version(base).await {
        Ok(v) => Version::parse(&v.version).ok(),
        Err(e) => {
            warn!(error=%e, "update: failed to fetch server version; skipping update");
            None
        }
    }
}

fn http_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .user_agent("gamiscreen-client-updater")
        .build()
        .map_err(|e| AppError::Http(e.to_string()))
}

async fn fetch_releases(
    gh: &reqwest::Client,
    repo: &str,
) -> Result<Vec<serde_json::Value>, AppError> {
    let url = format!("https://api.github.com/repos/{}/releases", repo);
    let resp = gh
        .get(url)
        .send()
        .await
        .map_err(|e| AppError::Http(e.to_string()))?;
    resp.json().await.map_err(|e| AppError::Http(e.to_string()))
}

async fn collect_candidates(
    gh: &reqwest::Client,
    releases: Vec<serde_json::Value>,
    server_version: &Version,
    current_version: &Version,
) -> Vec<(Version, Vec<Asset>)> {
    let mut candidates = Vec::new();
    for rel in releases.into_iter() {
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
        if !ver.pre.is_empty() {
            continue;
        }
        if !is_compatible(&ver, server_version) || ver <= *current_version {
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
            let (os, arch) = map_os_arch(name);
            if os.is_empty() || arch.is_empty() {
                continue;
            }
            out_assets.push(Asset {
                name: name.to_string(),
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
    candidates
}

fn select_asset(mut candidates: Vec<(Version, Vec<Asset>)>) -> Option<(Version, Asset)> {
    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    let (target_version, assets) = candidates.remove(0);
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let art = assets.into_iter().find(|a| a.os == os && a.arch == arch)?;
    Some((target_version, art))
}

async fn download_artifact(art: &Asset, dir: &Path) -> Result<NamedTempFile, AppError> {
    let tmp = NamedTempFile::new_in(dir).map_err(AppError::Io)?;
    download_to_file(&art.url, tmp.path()).await?;
    Ok(tmp)
}

fn verify_sha256(path: &Path, expected: &str) -> Result<bool, AppError> {
    let sum = sha256_file_hex(path)?;
    Ok(eq_hex_constant_time(&sum, expected))
}

fn is_zip_asset(art: &Asset) -> bool {
    art.name.to_lowercase().ends_with(".zip") || art.url.to_lowercase().ends_with(".zip")
}

fn extract_zip_to_temp(zip_path: &Path, dir: &Path) -> Result<NamedTempFile, AppError> {
    let tmp = NamedTempFile::new_in(dir).map_err(AppError::Io)?;
    extract_single_from_zip(zip_path, tmp.path())?;
    Ok(tmp)
}
