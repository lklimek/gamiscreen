use std::hash::{Hash, Hasher};
use std::{
    collections::hash_map::DefaultHasher,
    env, fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let web_dir = manifest_dir.join("../gamiscreen-web");
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // If web dir doesn’t exist, nothing to do.
    if !web_dir.exists() {
        println!(
            "cargo:warning=gamiscreen-web not found at {}",
            web_dir.display()
        );
        return;
    }

    // Watch all files under gamiscreen-web except node_modules and dist
    watch_dir_recursively(&web_dir, &["node_modules", "dist"]);

    // Also watch these env vars for rebuild behavior changes
    println!("cargo:rerun-if-env-changed=SKIP_WEB_BUILD");
    println!("cargo:rerun-if-env-changed=CARGO_DOC");
    println!("cargo:rerun-if-env-changed=DOCS_RS");

    // Allow skipping the web build (CI/docs or developer opt-out)
    let skip = env::var("SKIP_WEB_BUILD").ok().is_some()
        || env::var("CARGO_DOC").ok().is_some()
        || env::var("DOCS_RS").ok().is_some();
    if skip {
        println!("cargo:warning=Skipping web build (SKIP_WEB_BUILD/CARGO_DOC/DOCS_RS set)");
    } else {
        // Change detection: compute tree checksum (content + relative paths)
        let stamp_path = web_dir.join("dist").join(".web_build.stamp");
        let current_hash = compute_tree_checksum(&web_dir, &["node_modules", "dist"]);
        let last_hash = read_hash_stamp(&stamp_path);

        let needs_build = match (&current_hash, &last_hash) {
            (None, _) => false,               // no sources? nothing to do
            (Some(_), None) => true,          // never built
            (Some(h1), Some(h0)) => h1 != h0, // content changed
        };

        if !needs_build {
            println!("cargo:warning=Web sources unchanged; skipping npm build");
        } else {
            // Build the web app; fail Cargo build if it fails
            let npm_ok = Command::new("npm").arg("--version").output().is_ok();
            if !npm_ok {
                panic!(
                    "npm not found but web sources changed; install Node.js/npm or set SKIP_WEB_BUILD=1"
                );
            }

            // Prefer `npm ci` when lockfile exists; fallback to `npm install` if CI fails (lock out-of-sync).
            let has_lock = web_dir.join("package-lock.json").exists();
            let mut installed = false;
            if has_lock {
                match Command::new("npm").current_dir(&web_dir).arg("ci").status() {
                    Ok(s) if s.success() => {
                        installed = true;
                    }
                    Ok(_s) => {
                        // Fallback to install
                        eprintln!("npm ci failed; falling back to npm install");
                    }
                    Err(e) => {
                        eprintln!("failed to run npm ci: {e}; falling back to npm install");
                    }
                }
            }
            if !installed {
                match Command::new("npm")
                    .current_dir(&web_dir)
                    .arg("install")
                    .status()
                {
                    Ok(s) if s.success() => {}
                    Ok(s) => panic!("npm install failed with status: {s}"),
                    Err(e) => panic!("failed to run npm install: {e}"),
                }
            }

            match Command::new("npm")
                .current_dir(&web_dir)
                .args(["run", "build"])
                .status()
            {
                Ok(status) if status.success() => {
                    if let Some(h) = current_hash.as_deref() {
                        write_hash_stamp(&stamp_path, h);
                    }
                }
                Ok(status) => {
                    panic!("web build failed with status: {status}");
                }
                Err(err) => {
                    panic!("failed to invoke npm run build: {err}");
                }
            }
        }
    }

    // Always generate update manifest and write to OUT_DIR
    if let Err(e) = generate_update_manifest(&out_dir) {
        // Do not fail compilation locally; warn and write a minimal manifest
        println!("cargo:warning=update manifest generation failed: {}", e);
        let _ = write_minimal_manifest(&out_dir);
    }
}

fn watch_dir_recursively(root: &PathBuf, ignore_dirs: &[&str]) {
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|s| s.to_str())
                        && ignore_dirs.iter().any(|ig| ig == &name)
                    {
                        continue;
                    }
                    stack.push(path);
                } else if path.is_file() {
                    if path
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|ext| ext.eq_ignore_ascii_case("tsbuildinfo"))
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    println!("cargo:rerun-if-changed={}", path.display());
                }
            }
        }
    }
}

fn compute_tree_checksum(root: &Path, ignore_dirs: &[&str]) -> Option<String> {
    // Collect files deterministically (sorted by relative path)
    let mut files: Vec<PathBuf> = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Some(name) = path.file_name().and_then(|s| s.to_str())
                        && ignore_dirs.iter().any(|ig| ig == &name)
                    {
                        continue;
                    }
                    stack.push(path);
                } else if path.is_file() {
                    if path
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|ext| ext.eq_ignore_ascii_case("tsbuildinfo"))
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    files.push(path);
                }
            }
        }
    }
    if files.is_empty() {
        return None;
    }
    files.sort();

    let mut hasher = DefaultHasher::new();
    for file in files {
        let rel = file.strip_prefix(root).unwrap_or(&file);
        rel.to_string_lossy().hash(&mut hasher);
        if let Ok(mut f) = fs::File::open(&file) {
            let mut buf = Vec::new();
            if f.read_to_end(&mut buf).is_ok() {
                hasher.write(&buf);
            }
        }
    }
    let sum = hasher.finish();
    Some(format!("{:016x}", sum))
}

fn read_hash_stamp(path: &Path) -> Option<String> {
    let mut f = fs::File::open(path).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    let hex = s.trim().to_string();
    if hex.is_empty() { None } else { Some(hex) }
}

fn write_hash_stamp(path: &Path, hex: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut f) = fs::File::create(path) {
        let _ = writeln!(f, "{}", hex);
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct UpdateManifest {
    schema_version: u32,
    generated_at: String,
    latest_version: String,
    artifacts: Vec<Artifact>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct Artifact {
    os: String,
    arch: String,
    url: String,
    sha256: String,
}

fn repo_slug() -> String {
    // Prefer env var for flexibility; fallback to repo of this project
    std::env::var("UPDATE_REPO")
        .ok()
        .unwrap_or_else(|| "lklimek/gamiscreen".to_string())
}

fn generate_update_manifest(out_dir: &Path) -> Result<(), String> {
    let repo = repo_slug();
    // Use GitHub API to get latest release assets
    let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
    let client = reqwest::blocking::Client::builder()
        .user_agent("gamiscreen-server-build")
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(&url)
        .send()
        .map_err(|e| format!("http error: {e}"))?;
    let json: serde_json::Value = resp.json().map_err(|e| format!("json error: {e}"))?;

    let tag = json.get("tag_name").and_then(|v| v.as_str()).unwrap_or("");
    let version = tag.trim_start_matches('v').to_string();
    let assets = json
        .get("assets")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // Look for linux x86_64 artifacts with simple naming scheme
    let mut artifacts: Vec<Artifact> = Vec::new();
    for a in &assets {
        let name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let dl_url = a
            .get("browser_download_url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.ends_with(".sha256") {
            continue;
        }
        // Expect a sibling .sha256 asset
        let sha_name = format!("{}.sha256", name);
        let mut sha256 = String::new();
        for b in &assets {
            if b.get("name").and_then(|v| v.as_str()) == Some(sha_name.as_str())
                && let Some(url2) = b.get("browser_download_url").and_then(|v| v.as_str()) {
                    // download sha256 file (small)
                    if let Ok(r) = client.get(url2).send()
                        && let Ok(text) = r.text() {
                            sha256 = text.split_whitespace().next().unwrap_or("").to_string();
                        }
                }
        }
        // Heuristically map filename to os/arch
        let mut os = String::new();
        let mut arch = String::new();
        let lname = name.to_lowercase();
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
        if !os.is_empty() && !arch.is_empty() && !dl_url.is_empty() && !sha256.is_empty() {
            artifacts.push(Artifact {
                os,
                arch,
                url: dl_url,
                sha256,
            });
        }
    }

    // Note: do not add fallback entries without verified sha256. If none found, produce empty list.

    let manifest = UpdateManifest {
        schema_version: 1,
        generated_at: chrono_now_rfc3339(),
        latest_version: version,
        artifacts,
    };
    let data = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    let path = out_dir.join("update_manifest.json");
    std::fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(())
}

fn write_minimal_manifest(out_dir: &Path) -> Result<(), String> {
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let manifest = UpdateManifest {
        schema_version: 1,
        generated_at: chrono_now_rfc3339(),
        latest_version: version.clone(),
        artifacts: vec![], // no unverified artifacts
    };
    let data = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    let path = out_dir.join("update_manifest.json");
    std::fs::write(&path, data).map_err(|e| e.to_string())
}

fn chrono_now_rfc3339() -> String {
    // Avoid pulling chrono; do a simple UTC RFC3339 using std
    // Note: std doesn't format RFC3339; write minimal format
    // Fallback to UNIX epoch string if system time unavailable
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => format!("{}", d.as_secs()),
        Err(_) => "0".into(),
    }
}
