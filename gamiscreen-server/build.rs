use gamiscreen_shared::api::ts_export;
use serde_json::Value;
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
    let workspace_manifest = manifest_dir.join("../Cargo.toml");
    let _out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // If web dir doesn’t exist, nothing to do.
    if !web_dir.exists() {
        println!(
            "cargo:warning=gamiscreen-web not found at {}",
            web_dir.display()
        );
        return;
    }

    if workspace_manifest.exists() {
        println!("cargo:rerun-if-changed={}", workspace_manifest.display());
    }

    // Ensure the web package version tracks the workspace version
    if let Err(err) = sync_web_package_version(&web_dir) {
        panic!("failed to synchronize gamiscreen-web version: {}", err);
    }

    // Generate shared TypeScript definitions for the web app
    let generated_ts = web_dir.join("src/generated/api-types.ts");
    if let Err(err) = ts_export::export_types(&generated_ts) {
        panic!(
            "failed to export TypeScript definitions to {}: {}",
            generated_ts.display(),
            err
        );
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
}

fn sync_web_package_version(web_dir: &Path) -> Result<(), String> {
    let version = env::var("CARGO_PKG_VERSION")
        .map_err(|err| format!("CARGO_PKG_VERSION not available: {err}"))?;
    let package_json_path = web_dir.join("package.json");
    if !package_json_path.exists() {
        return Err(format!("{} not found", package_json_path.display()));
    }

    let mut updated = update_version_in_file(&package_json_path, &version, false)?;
    let package_lock_path = web_dir.join("package-lock.json");
    if package_lock_path.exists() {
        updated |= update_version_in_file(&package_lock_path, &version, true)?;
    }

    if updated {
        println!(
            "cargo:warning=Synchronized gamiscreen-web version to {}",
            version
        );
    }

    Ok(())
}

fn update_version_in_file(
    path: &Path,
    version: &str,
    update_lock_root: bool,
) -> Result<bool, String> {
    let raw = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let json: Value = serde_json::from_str(&raw)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;

    let current = json
        .as_object()
        .and_then(|obj| obj.get("version"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("{} missing string version field", path.display()))?;

    if current == version {
        return Ok(false);
    }

    let mut updated =
        replace_version_literal(&raw, current, version, 0, path, "top-level version")?;

    if update_lock_root
        && let Some(root_version) = json
            .get("packages")
            .and_then(|v| v.as_object())
            .and_then(|map| map.get(""))
            .and_then(|v| v.as_object())
            .and_then(|obj| obj.get("version"))
            .and_then(|v| v.as_str())
        && root_version != version
    {
        let packages_pos = updated
            .find("\"packages\"")
            .ok_or_else(|| format!("{} missing packages section", path.display()))?;
        let empty_key_pos = updated[packages_pos..]
            .find("\"\": {")
            .map(|idx| packages_pos + idx)
            .ok_or_else(|| format!("could not locate root package entry in {}", path.display()))?;
        updated = replace_version_literal(
            &updated,
            root_version,
            version,
            empty_key_pos,
            path,
            "root package version",
        )?;
    }

    fs::write(path, updated).map_err(|err| format!("failed to write {}: {err}", path.display()))?;

    Ok(true)
}

fn replace_version_literal(
    source: &str,
    current: &str,
    desired: &str,
    search_start: usize,
    path: &Path,
    context: &str,
) -> Result<String, String> {
    let needle = format!("\"version\": \"{}\"", current);
    let haystack = &source[search_start..];
    let relative = haystack.find(&needle).ok_or_else(|| {
        format!(
            "could not find {context} ({}) in {}",
            needle,
            path.display()
        )
    })?;
    let pos = search_start + relative;
    let mut result =
        String::with_capacity(source.len() + desired.len().saturating_sub(current.len()));
    result.push_str(&source[..pos]);
    result.push_str(&format!("\"version\": \"{}\"", desired));
    result.push_str(&source[pos + needle.len()..]);
    Ok(result)
}

fn watch_dir_recursively(root: &Path, ignore_dirs: &[&str]) {
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
