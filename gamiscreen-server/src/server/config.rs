use crate::shared::{Child, Task};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::Path};

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub children: Vec<Child>,
    pub tasks: Vec<Task>,
    pub jwt_secret: String,
    pub users: Vec<UserConfig>,
    pub dev_cors_origin: Option<String>,
    pub listen_port: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserConfig {
    pub username: String,
    pub password_hash: String, // bcrypt hash
    pub role: Role,
    pub child_id: Option<String>, // required when role == child
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Parent,
    Child,
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Yaml(serde_yaml::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "IO error: {}", e),
            ConfigError::Yaml(e) => write!(f, "YAML error: {}", e),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(value: std::io::Error) -> Self {
        ConfigError::Io(value)
    }
}

impl From<serde_yaml::Error> for ConfigError {
    fn from(value: serde_yaml::Error) -> Self {
        ConfigError::Yaml(value)
    }
}

impl AppConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let path = env::var("CONFIG_PATH").unwrap_or_else(|_| "config.yaml".to_string());
        Self::load_from_path(path)
    }

    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let text = fs::read_to_string(&path)?;
        let cfg: AppConfig = serde_yaml::from_str(&text)?;
        Ok(cfg)
    }
}
