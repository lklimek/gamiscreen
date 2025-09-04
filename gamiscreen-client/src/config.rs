use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::AppError;

pub const ENV_CONFIG: &str = "GAMISCREEN_CONFIG";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub server_url: String,
    pub child_id: String,
    pub device_id: String,
    #[serde(default = "default_interval")]
    pub interval_secs: u64,
    /// Optional override for lock command. Example: ["loginctl", "lock-session", "$XDG_SESSION_ID"]
    #[serde(default)]
    pub lock_cmd: Option<Vec<String>>,
}

fn default_interval() -> u64 {
    60
}

pub fn resolve_config_path(cli_value: Option<PathBuf>) -> Result<PathBuf, AppError> {
    if let Some(p) = cli_value {
        return Ok(p);
    }
    if let Ok(p) = std::env::var(ENV_CONFIG) {
        return Ok(PathBuf::from(p));
    }
    default_config_path().ok_or_else(|| AppError::Config("could not determine config dir".into()))
}

pub fn default_config_path() -> Option<PathBuf> {
    let pd = ProjectDirs::from("dev", "gamiscreen", "gamiscreen")?;
    Some(pd.config_dir().join("client.yaml"))
}

pub fn load_config(path: &PathBuf) -> Result<ClientConfig, AppError> {
    let data = std::fs::read_to_string(path)
        .map_err(|e| AppError::Config(format!("read {} failed: {e}", path.display())))?;
    let cfg: ClientConfig = serde_yaml::from_str(&data)
        .map_err(|e| AppError::Config(format!("parse {} failed: {e}", path.display())))?;
    Ok(cfg)
}

pub fn save_config(path: &PathBuf, cfg: &ClientConfig) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let data = serde_yaml::to_string(cfg)
        .map_err(|e| AppError::Config(format!("serialize config failed: {e}")))?;
    std::fs::write(path, data)
        .map_err(|e| AppError::Config(format!("write {} failed: {e}", path.display())))
}

pub fn normalize_server_url(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.trim_end_matches('/').to_string()
    } else {
        format!("http://{}", trimmed.trim_end_matches('/'))
    }
}
