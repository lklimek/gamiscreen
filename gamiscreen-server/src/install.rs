use std::fs;
use std::io::Write;
use std::path::Path;

use tinytemplate::TinyTemplate;

const EXAMPLE_CONFIG: &str = include_str!("../config.yaml.example");
const UNIT_TEMPLATE: &str = include_str!("../systemd/gamiscreen-server.service");

fn generate_secret() -> String {
    // Use UUIDv4 as a simple random secret seed
    uuid::Uuid::new_v4().to_string()
}

fn render_default_config() -> String {
    let secret = generate_secret();
    // Replace the placeholder secret if present; otherwise append/patch minimally
    if EXAMPLE_CONFIG.contains("change-this-to-a-long-random-secret") {
        EXAMPLE_CONFIG.replace("change-this-to-a-long-random-secret", &secret)
    } else {
        EXAMPLE_CONFIG.to_string()
    }
}

#[derive(serde::Serialize)]
struct UnitCtx<'a> {
    binary_path: &'a str,
    config_path: &'a str,
    db_path: &'a str,
    user: &'a str,
    group: &'a str,
    working_dir: &'a str,
}

fn render_unit(ctx: &UnitCtx) -> Result<String, String> {
    let mut tt = TinyTemplate::new();
    tt.add_template("unit", UNIT_TEMPLATE)
        .map_err(|e| format!("template: {e}"))?;
    tt.render("unit", ctx).map_err(|e| format!("render: {e}"))
}

#[allow(clippy::too_many_arguments)]
pub fn install_system(
    unit_path: &Path,
    config_path: &Path,
    db_path: &Path,
    binary_path: &Path,
    user: &str,
    group: &str,
    working_dir: &Path,
    force: bool,
) -> Result<(), String> {
    // Ensure dirs
    if let Some(dir) = config_path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("create dir {}: {}", dir.display(), e))?;
    }
    if let Some(dir) = unit_path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("create dir {}: {}", dir.display(), e))?;
    }
    if let Some(dir) = db_path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("create dir {}: {}", dir.display(), e))?;
    }

    // Config
    if config_path.exists() && !force {
        eprintln!(
            "Config exists at {}; skipping (use --force to overwrite)",
            config_path.display()
        );
    } else {
        let cfg = render_default_config();
        let mut f = fs::File::create(config_path)
            .map_err(|e| format!("write {}: {}", config_path.display(), e))?;
        f.write_all(cfg.as_bytes())
            .map_err(|e| format!("write {}: {}", config_path.display(), e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(config_path, fs::Permissions::from_mode(0o640));
        }
        println!("Wrote config to {}", config_path.display());
    }

    // Unit
    if unit_path.exists() && !force {
        eprintln!(
            "Unit exists at {}; skipping (use --force to overwrite)",
            unit_path.display()
        );
    } else {
        let ctx = UnitCtx {
            binary_path: &binary_path.display().to_string(),
            config_path: &config_path.display().to_string(),
            db_path: &db_path.display().to_string(),
            user,
            group,
            working_dir: &working_dir.display().to_string(),
        };
        let unit_txt = render_unit(&ctx)?;
        let mut f = fs::File::create(unit_path)
            .map_err(|e| format!("write {}: {}", unit_path.display(), e))?;
        f.write_all(unit_txt.as_bytes())
            .map_err(|e| format!("write {}: {}", unit_path.display(), e))?;
        println!("Wrote unit to {}", unit_path.display());
    }

    println!(
        "Done. Run: sudo systemctl daemon-reload && sudo systemctl enable --now gamiscreen-server"
    );
    Ok(())
}

pub fn uninstall_system(
    unit_path: &Path,
    remove_config: bool,
    config_path: &Path,
) -> Result<(), String> {
    if unit_path.exists() {
        fs::remove_file(unit_path).map_err(|e| format!("remove {}: {}", unit_path.display(), e))?;
        println!("Removed unit {}", unit_path.display());
    } else {
        println!("Unit {} not found; skipping", unit_path.display());
    }
    if remove_config {
        if config_path.exists() {
            fs::remove_file(config_path)
                .map_err(|e| format!("remove {}: {}", config_path.display(), e))?;
            println!("Removed config {}", config_path.display());
        } else {
            println!("Config {} not found; skipping", config_path.display());
        }
    }
    println!("Run: sudo systemctl daemon-reload && sudo systemctl disable --now gamiscreen-server");
    Ok(())
}
