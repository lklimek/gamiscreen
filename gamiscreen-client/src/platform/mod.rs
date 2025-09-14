pub mod linux;

use crate::{AppError, config::ClientConfig};

use async_trait::async_trait;
use std::sync::Arc;

/// Cross-platform interface for OS-level actions we need.
#[async_trait]
pub trait Platform: Send + Sync {
    async fn lock(&self) -> Result<(), AppError>;
    async fn unlock(&self) -> Result<(), AppError>;
    async fn notify(&self, total_secs: u64);
    async fn update_notification(&self, seconds_left: u64);
    async fn hide_notification(&self);
}

/// Detect the current platform and return an implementation.
pub async fn detect(_cfg: &ClientConfig) -> Result<Arc<dyn Platform>, AppError> {
    // For now we only implement Linux. Windows support will be added next.
    let backend = linux::lock::detect_lock_backend(_cfg).await?;
    Ok(Arc::new(linux::LinuxPlatform::new(backend)))
}

/// Temporary shim to query session lock state via Linux implementation.
/// This keeps existing behavior while the Platform API stabilizes.
pub async fn is_session_locked() -> Result<bool, AppError> {
    linux::lock::is_session_locked().await
}
