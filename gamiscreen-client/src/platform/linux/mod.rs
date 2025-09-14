pub mod install;
pub mod lock;
pub mod lock_tester;
pub mod notify;

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::AppError;

use super::Platform;

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

    async fn update_notification(&self, seconds_left: u64) {
        self.notifier.lock().await.update(seconds_left).await;
    }

    async fn hide_notification(&self) {
        self.notifier.lock().await.close().await;
    }

    fn device_id(&self) -> String {
        let uid = nix::unistd::getuid().as_raw();
        let machine_id = read_machine_id().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        format!("uid{}-{}", uid, machine_id)
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
