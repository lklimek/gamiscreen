use std::io::Write;
use std::path::PathBuf;

use crate::AppError;
use tokio::process::Command;
use tracing::{info, warn};
use tinytemplate::TinyTemplate;

// Include templates from files in the repo
const POLKIT_RULE_PATH_DST: &str = "/etc/polkit-1/rules.d/49-gamiscreen-lock.rules";
const POLKIT_RULE_TEMPLATE: &str = include_str!("../../../polkit/49-gamiscreen-lock.rules");
const USER_UNIT_NAME: &str = "gamiscreen-client.service";
const USER_UNIT_TEMPLATE: &str = include_str!("../../../systemd/gamiscreen-client.service");

pub async fn install_all(user_opt: Option<String>) -> Result<(), AppError> {
    let target_user = resolve_target_user(user_opt, true)?;

    info!("install: creating gamiscreen group and adding target user");
    ensure_group_and_membership("gamiscreen", &target_user).await?;

    info!("install: installing polkit rule to {}", POLKIT_RULE_PATH_DST);
    install_polkit_rule().await?;

    info!(user=%target_user, "install: installing user systemd unit and enabling service");
    // When running as the target user, attempt enable --now; as root, enable only
    let start_now = !is_root() && Some(target_user.clone()) == current_username();
    install_user_unit(&target_user, start_now).await?;

    println!(
        "Install complete for user '{}'. If the user was newly added to the 'gamiscreen' group, please log out and log back in for group membership to take effect.",
        target_user
    );
    if !start_now {
        println!(
            "Note: user manager was not started. The service is enabled and will start on next login."
        );
    }
    Ok(())
}

pub async fn uninstall_all(user_opt: Option<String>) -> Result<(), AppError> {
    let target_user = resolve_target_user(user_opt, true)?;

    info!(user=%target_user, "uninstall: disabling and removing user systemd unit");
    let stop_now = !is_root() && Some(target_user.clone()) == current_username();
    uninstall_user_unit(&target_user, stop_now).await?;

    info!("uninstall: removing polkit rule from {}", POLKIT_RULE_PATH_DST);
    uninstall_polkit_rule().await?;

    println!(
        "Uninstall complete for user '{}'. The 'gamiscreen' group and membership were not removed.",
        target_user
    );
    Ok(())
}

async fn ensure_group_and_membership(group: &str, user: &str) -> Result<(), AppError> {
    // groupadd -f <group>
    run_root_cmd("groupadd", &["-f", group]).await?;

    // usermod -aG <group> <user>
    run_root_cmd("usermod", &["-aG", group, user]).await?;
    Ok(())
}

fn current_username() -> Option<String> {
    use nix::unistd::{Uid, User};
    let uid = Uid::current();
    User::from_uid(uid).ok().flatten().map(|u| u.name)
}

async fn install_polkit_rule() -> Result<(), AppError> {
    let tmp_path = write_temp_file("49-gamiscreen-lock.rules", POLKIT_RULE_TEMPLATE)?;
    // sudo install -D -m 0644 <tmp> /etc/polkit-1/rules.d/49-gamiscreen-lock.rules
    run_root_cmd(
        "install",
        &["-D", "-m", "0644", tmp_path.to_str().unwrap(), POLKIT_RULE_PATH_DST],
    )
    .await?;
    // best-effort cleanup
    let _ = std::fs::remove_file(&tmp_path);
    Ok(())
}

async fn uninstall_polkit_rule() -> Result<(), AppError> {
    // sudo rm -f /etc/polkit-1/rules.d/49-gamiscreen-lock.rules
    run_root_cmd("rm", &["-f", POLKIT_RULE_PATH_DST]).await?;
    Ok(())
}

async fn install_user_unit(username: &str, start_now: bool) -> Result<(), AppError> {
    let unit_dir = user_systemd_unit_dir(username)?;
    std::fs::create_dir_all(&unit_dir).map_err(AppError::Io)?;
    let unit_path = unit_dir.join(USER_UNIT_NAME);
    let bin_path = resolve_binary_path()?;
    let unit_text = render_user_unit(&bin_path)?;
    std::fs::write(&unit_path, unit_text).map_err(AppError::Io)?;

    // systemctl --user daemon-reload
    run_user_cmd(username, "systemctl", &["--user", "daemon-reload"]).await?;
    // enable; start now only when running as that user
    run_user_cmd(username, "systemctl", &["--user", "enable", "gamiscreen-client"]).await?;
    if start_now {
        let _ = run_user_cmd(username, "systemctl", &["--user", "start", "gamiscreen-client"]).await;
    }
    Ok(())
}

async fn uninstall_user_unit(username: &str, stop_now: bool) -> Result<(), AppError> {
    // systemctl --user disable --now gamiscreen-client (best-effort)
    if stop_now {
        let _ = run_user_cmd(username, "systemctl", &["--user", "disable", "--now", "gamiscreen-client"]).await;
    } else {
        let _ = run_user_cmd(username, "systemctl", &["--user", "disable", "gamiscreen-client"]).await;
    }

    let unit_dir = user_systemd_unit_dir(username)?;
    let unit_path = unit_dir.join(USER_UNIT_NAME);
    if unit_path.exists() {
        std::fs::remove_file(&unit_path).map_err(AppError::Io)?;
        // Reload to purge stale unit
        let _ = run_user_cmd(username, "systemctl", &["--user", "daemon-reload"]).await;
    }
    Ok(())
}

fn user_systemd_unit_dir(username: &str) -> Result<PathBuf, AppError> {
    let home = user_home_dir(username).ok_or_else(|| AppError::Config(format!("cannot find home for user {}", username)))?;
    Ok(home.join(".config").join("systemd").join("user"))
}

fn write_temp_file(name: &str, content: &str) -> Result<PathBuf, AppError> {
    let dir = std::env::temp_dir();
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).map_err(AppError::Io)?;
    f.write_all(content.as_bytes()).map_err(AppError::Io)?;
    Ok(path)
}

fn resolve_binary_path() -> Result<String, AppError> {
    let exe = std::env::current_exe().map_err(AppError::Io)?;
    Ok(exe.display().to_string())
}

#[derive(serde::Serialize)]
struct UnitCtx<'a> {
    binary_path: &'a str,
}

fn render_user_unit(binary_path: &str) -> Result<String, AppError> {
    let mut tt = TinyTemplate::new();
    tt.add_template("unit", USER_UNIT_TEMPLATE)
        .map_err(|e| AppError::Config(format!("template error: {e}")))?;
    let ctx = UnitCtx { binary_path };
    tt.render("unit", &ctx)
        .map_err(|e| AppError::Config(format!("render error: {e}")))
}

async fn run_root_cmd(prog: &str, args: &[&str]) -> Result<(), AppError> {
    // Use sudo for privileged operations; if already root this still works.
    let status = Command::new("sudo")
        .arg(prog)
        .args(args)
        .status()
        .await
        .map_err(AppError::Io)?;
    if !status.success() {
        return Err(AppError::Io(std::io::Error::other(format!(
            "sudo {} {:?} failed with status {}",
            prog, args, status
        ))));
    }
    Ok(())
}

async fn run_cmd(prog: &str, args: &[&str]) -> Result<(), AppError> {
    let status = Command::new(prog).args(args).status().await.map_err(AppError::Io)?;
    if !status.success() {
        warn!(program=%prog, ?args, %status, "command failed");
    }
    Ok(())
}

async fn run_user_cmd(user: &str, prog: &str, args: &[&str]) -> Result<(), AppError> {
    // If already that user, run directly
    if !is_root() && current_username().as_deref() == Some(user) {
        return run_cmd(prog, args).await;
    }
    // Run with sudo -u <user> -H
    let status = Command::new("sudo")
        .arg("-u")
        .arg(user)
        .arg("-H")
        .arg(prog)
        .args(args)
        .status()
        .await
        .map_err(AppError::Io)?;
    if !status.success() {
        warn!(program=%prog, user=%user, ?args, %status, "sudo -u command failed");
    }
    Ok(())
}

fn user_home_dir(user: &str) -> Option<PathBuf> {
    use nix::unistd::User;
    User::from_name(user).ok().flatten().map(|u| PathBuf::from(u.dir))
}

fn is_root() -> bool {
    use nix::unistd::Uid;
    Uid::effective().is_root()
}

fn resolve_target_user(user_opt: Option<String>, prompt_if_missing: bool) -> Result<String, AppError> {
    let cur = current_username().ok_or_else(|| AppError::Config("cannot determine current user".into()))?;
    if is_root() {
        if let Some(u) = user_opt {
            return Ok(u);
        }
        if prompt_if_missing {
            let u = prompt("Target username for user-level install: ")?;
            if u.is_empty() {
                return Err(AppError::Config("username is required when running as root".into()));
            }
            return Ok(u);
        }
        return Err(AppError::Config("username is required when running as root".into()));
    } else {
        if let Some(u) = user_opt {
            if u != cur {
                return Err(AppError::Config(format!(
                    "cannot install for user '{}' when running as '{}' (run as root)",
                    u, cur
                )));
            }
        }
        Ok(cur)
    }
}

fn prompt(msg: &str) -> Result<String, AppError> {
    use std::io::Write;
    print!("{}", msg);
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    Ok(s.trim().to_string())
}
