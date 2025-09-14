use tokio::process::Command;
use tracing::{info, warn};

use crate::AppError;

pub const TASK_NAME: &str = "GamiScreen Client";

pub async fn install_for_current_user(start_now: bool) -> Result<(), AppError> {
    let exe = std::env::current_exe().map_err(AppError::Io)?;
    let exe_str = exe.display().to_string();
    // schtasks expects quotes around the full path
    let tr = format!("\"{}\"", exe_str);

    // Create or update the task (use /F)
    let status = Command::new("schtasks")
        .args([
            "/Create",
            "/F",
            "/SC",
            "ONLOGON",
            "/RL",
            "LIMITED",
            "/TN",
            TASK_NAME,
            "/TR",
            &tr,
        ])
        .status()
        .await
        .map_err(AppError::Io)?;
    if !status.success() {
        return Err(AppError::Io(std::io::Error::other(format!(
            "schtasks /Create failed with status {}",
            status
        ))));
    }

    info!(task=TASK_NAME, path=%exe_str, "Windows Scheduled Task installed for current user");

    if start_now {
        // Best-effort: start it immediately in current session
        let _ = Command::new("schtasks")
            .args(["/Run", "/TN", TASK_NAME])
            .status()
            .await;
    }
    Ok(())
}

pub async fn uninstall_for_current_user() -> Result<(), AppError> {
    let status = Command::new("schtasks")
        .args(["/Delete", "/F", "/TN", TASK_NAME])
        .status()
        .await
        .map_err(AppError::Io)?;
    if !status.success() {
        // Treat missing task as success
        warn!(task=TASK_NAME, status=%status, "schtasks /Delete failed or task missing; continuing");
    }
    Ok(())
}

