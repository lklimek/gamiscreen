pub mod install;
pub mod lock;
pub mod lock_tester;
pub mod notify;

use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;

use super::Platform;
use crate::AppError;

/// Linux implementation of the cross-platform interface.
pub struct LinuxPlatform {
    lock_backend: lock::LockBackend,
    notifier: Arc<Mutex<notify::Notifier>>, // single notifier instance
}

impl LinuxPlatform {
    pub fn new(lock_backend: lock::LockBackend) -> Self {
        Self {
            lock_backend,
            notifier: Arc::new(Mutex::new(notify::Notifier::new())),
        }
    }
}

#[async_trait::async_trait]
impl Platform for LinuxPlatform {
    async fn lock(&self) -> Result<(), AppError> {
        lock::enforce_lock_backend(&self.lock_backend).await
    }

    async fn is_session_locked(&self) -> Result<bool, AppError> {
        lock::is_session_locked().await
    }

    async fn notify(&self, total_secs: u64) {
        self.notifier.lock().await.show_countdown(total_secs).await;
    }

    async fn update_notification(&self, remaining_secs: i64) {
        self.notifier.lock().await.update(remaining_secs).await;
    }

    async fn hide_notification(&self) {
        self.notifier.lock().await.close().await;
    }

    fn device_id(&self) -> String {
        let uid = nix::unistd::getuid().as_raw();
        let machine_id = read_machine_id().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        format!("uid{}-{}", uid, machine_id)
    }

    async fn install(&self, user: Option<String>) -> Result<(), AppError> {
        install::install_all(user).await
    }

    async fn uninstall(&self, user: Option<String>) -> Result<(), AppError> {
        install::uninstall_all(user).await
    }

    fn replace_and_restart(&self, staged_src: &Path, current_exe: &Path, args: &[String]) -> ! {
        // Move the staged binary into place atomically, then exec into it
        if let Err(e) = std::fs::rename(staged_src, current_exe) {
            tracing::warn!(error=%e, "Linux: failed to replace binary");
            std::process::exit(0);
        }
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(current_exe).args(args).exec();
        tracing::warn!(error=?err, "Linux: exec failed after update; exiting");
        std::process::exit(0);
    }
}

fn read_machine_id() -> Option<String> {
    let paths = ["/etc/machine-id", "/var/lib/dbus/machine-id"];
    for p in paths {
        if let Ok(s) = std::fs::read_to_string(p) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}
