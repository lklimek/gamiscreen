pub mod install;
pub mod lock;
pub mod lock_tester;
pub mod notify;

use std::path::{Path, PathBuf};
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

pub fn ensure_console_dbus_env() {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
        return;
    }

    let Some(runtime_dir) = find_runtime_dir_with_bus() else {
        return;
    };

    export_runtime_dir(&runtime_dir);
    if let Some(addr) = build_bus_address(&runtime_dir) {
        // SAFETY: we provide owned UTF-8 data, so setting the process env var is fine.
        unsafe {
            std::env::set_var("DBUS_SESSION_BUS_ADDRESS", addr);
        }
    }
}

fn find_runtime_dir_with_bus() -> Option<PathBuf> {
    runtime_dir_from_env()
        .and_then(runtime_dir_if_bus_exists)
        .or_else(|| runtime_dir_if_bus_exists(default_runtime_dir()))
}

fn runtime_dir_if_bus_exists(dir: PathBuf) -> Option<PathBuf> {
    dir.join("bus").exists().then_some(dir)
}

fn runtime_dir_from_env() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)
}

fn default_runtime_dir() -> PathBuf {
    let uid = nix::unistd::geteuid().as_raw();
    PathBuf::from(format!("/run/user/{uid}"))
}

fn export_runtime_dir(runtime: &Path) {
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        // SAFETY: runtime originates from a valid PathBuf and remains owned for the program lifetime.
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", runtime.as_os_str());
        }
    }
}

fn build_bus_address(runtime: &Path) -> Option<String> {
    let bus = runtime.join("bus");
    bus.exists().then(|| format!("unix:path={}", bus.display()))
}

#[async_trait::async_trait]
impl Platform for LinuxPlatform {
    fn initialize_process(&self) {
        ensure_console_dbus_env();
    }

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
