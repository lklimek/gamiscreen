#[cfg(not(target_os = "windows"))]
pub mod linux;
#[cfg(target_os = "windows")]
pub mod windows;

use crate::{AppError, config::ClientConfig};

use async_trait::async_trait;
use std::sync::Arc;

/// Cross-platform interface for OS-level actions we need.
#[async_trait]
pub trait Platform: Send + Sync {
    async fn lock(&self) -> Result<(), AppError>;
    async fn is_session_locked(&self) -> Result<bool, AppError>;
    async fn notify(&self, total_secs: u64);
    async fn update_notification(&self, seconds_left: u64);
    async fn hide_notification(&self);
    /// Generate a stable device identifier for this OS
    fn device_id(&self) -> String;
    /// Install background service/agent for this platform.
    ///
    /// On Linux, this installs polkit rules and a user systemd unit.
    /// On Windows, this path is deprecated; callers should use the `service` commands instead.
    async fn install(&self, user: Option<String>) -> Result<(), AppError>;
    /// Uninstall background service/agent for this platform.
    async fn uninstall(&self, user: Option<String>) -> Result<(), AppError>;
    /// Install a prepared update and restart the application.
    ///
    /// `staged_src` points to a complete, executable binary staged on disk
    /// next to the current executable. Implementations should atomically
    /// replace the current binary at `current_exe` with `staged_src` using
    /// OS-appropriate mechanisms, then restart the process with `args`.
    ///
    /// This function does not return on success; it terminates the current
    /// process image (either via `exec` on Unix or exiting after spawning on Windows).
    fn replace_and_restart(
        &self,
        staged_src: &std::path::Path,
        current_exe: &std::path::Path,
        args: &[String],
    ) -> !;
}

/// Detect the current platform and return an implementation.
#[allow(unused_variables)]
pub async fn detect(cfg: &ClientConfig) -> Result<Arc<dyn Platform>, AppError> {
    #[cfg(target_os = "windows")]
    {
        Ok(Arc::new(windows::WindowsPlatform::new()))
    }

    #[cfg(not(target_os = "windows"))]
    {
        let backend = linux::lock::detect_lock_backend(cfg).await?;
        Ok(Arc::new(linux::LinuxPlatform::new(backend)))
    }
}

/// Detect platform without requiring a config (used early in CLI flows).
pub async fn detect_default() -> Result<Arc<dyn Platform>, AppError> {
    #[cfg(target_os = "windows")]
    {
        Ok(Arc::new(windows::WindowsPlatform::new()))
    }

    #[cfg(not(target_os = "windows"))]
    {
        let dummy_cfg = ClientConfig {
            server_url: String::new(),
        };
        let backend = linux::lock::detect_lock_backend(&dummy_cfg).await?;
        Ok(Arc::new(linux::LinuxPlatform::new(backend)))
    }
}
