pub use gamiscreen_shared::auth::Role;
use gamiscreen_shared::domain::{Child, Task};
use semver::Version;
use serde::Deserialize;
use serde_yaml::{Mapping, Value};
use std::{env, fs, path::Path};
use tracing::warn;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub config_version: String,
    pub tenant_id: String,
    pub children: Vec<Child>,
    pub tasks: Vec<Task>,
    pub jwt_secret: String,
    pub users: Vec<UserConfig>,
    pub dev_cors_origin: Option<String>,
    pub listen_port: Option<u16>,
    #[serde(default)]
    pub push: Option<PushConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserConfig {
    pub username: String,
    pub password_hash: String, // bcrypt hash
    pub role: Role,
    pub child_id: Option<String>, // required when role == child
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct PushConfig {
    pub enabled: bool,
    pub vapid_public: Option<String>,
    pub vapid_private: Option<String>,
    pub contact_email: Option<String>,
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Yaml(serde_yaml::Error),
    Invalid(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "IO error: {}", e),
            ConfigError::Yaml(e) => write!(f, "YAML error: {}", e),
            ConfigError::Invalid(e) => write!(f, "invalid config: {}", e),
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
        migrate_config(path.as_ref())?;
        let text = fs::read_to_string(&path)?;
        let mut cfg: AppConfig = serde_yaml::from_str(&text)?;
        apply_env_overrides(&mut cfg);
        Ok(cfg)
    }
}

type MigrationFn = fn(&mut Mapping) -> bool;

const MIGRATIONS: &[(&str, MigrationFn)] =
    &[("0.7.0", migrate_to_0_7_0), ("0.9.0", migrate_to_0_9_0)];

fn migrate_config(path: &Path) -> Result<(), ConfigError> {
    let text = fs::read_to_string(path)?;
    let mut doc: Value = serde_yaml::from_str(&text)?;

    let Some(mapping) = doc.as_mapping_mut() else {
        return Ok(());
    };

    let version_key = Value::String("config_version".to_string());

    let mut changed = false;
    let mut major_update = false;

    let mut current_version = mapping
        .get(&version_key)
        .and_then(|v| v.as_str())
        .and_then(|s| Version::parse(s).ok())
        .unwrap_or_else(|| Version::new(0, 0, 0));
    let original_version = current_version.clone();

    for (version_str, migrate) in MIGRATIONS {
        let target_version = Version::parse(version_str).map_err(|e| {
            ConfigError::Invalid(format!("invalid migration version {}: {}", version_str, e))
        })?;
        if current_version < target_version {
            if target_version.major > original_version.major {
                major_update = true;
            }
            let migration_changed = migrate(mapping);
            let previous =
                mapping.insert(version_key.clone(), Value::String(version_str.to_string()));
            let version_changed = previous
                .as_ref()
                .and_then(|v| v.as_str())
                .map(|s| s != *version_str)
                .unwrap_or(true);
            if migration_changed || version_changed {
                changed = true;
            }
            current_version = target_version;
        }
    }

    let pkg_version = Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|e| ConfigError::Invalid(format!("invalid package version: {}", e)))?;
    if current_version < pkg_version {
        if pkg_version.major > original_version.major {
            major_update = true;
        }
        let previous = mapping.insert(
            version_key,
            Value::String(env!("CARGO_PKG_VERSION").to_string()),
        );
        let version_changed = previous
            .as_ref()
            .and_then(|v| v.as_str())
            .map(|s| s != env!("CARGO_PKG_VERSION"))
            .unwrap_or(true);
        if version_changed {
            if pkg_version.major > current_version.major {
                major_update = true;
            }
            changed = true;
        }
    }

    if changed {
        let updated = serde_yaml::to_string(&doc)?;
        if let Err(e) = fs::write(path, updated) {
            if major_update {
                warn!(
                    error = %e,
                    path = ?path,
                    "config migration for major update failed; continuing with existing config"
                );
            } else {
                warn!(error = %e, path = ?path, "config migration skipped (read-only)");
            }
        }
    }

    Ok(())
}

fn migrate_to_0_7_0(map: &mut Mapping) -> bool {
    let mut changed = false;

    let tenant_key = Value::String("tenant_id".to_string());
    if !map.contains_key(&tenant_key) {
        map.insert(tenant_key, Value::String("first".to_string()));
        changed = true;
    }

    changed
}

fn migrate_to_0_9_0(map: &mut Mapping) -> bool {
    let push_key = Value::String("push".to_string());
    if map.contains_key(&push_key) {
        return false;
    }

    let mut push_map = Mapping::new();
    push_map.insert(Value::String("enabled".to_string()), Value::Bool(false));

    map.insert(push_key, Value::Mapping(push_map));
    true
}

fn apply_env_overrides(cfg: &mut AppConfig) {
    if let Ok(val) = env::var("PUSH_VAPID_PUBLIC") {
        let push = cfg.push.get_or_insert_with(Default::default);
        push.vapid_public = Some(val);
    }
    if let Ok(val) = env::var("PUSH_VAPID_PRIVATE") {
        let push = cfg.push.get_or_insert_with(Default::default);
        push.vapid_private = Some(val);
    }
    if let Ok(val) = env::var("PUSH_CONTACT_EMAIL") {
        let push = cfg.push.get_or_insert_with(Default::default);
        push.contact_email = Some(val);
    }
    if let Ok(val) = env::var("PUSH_ENABLED")
        && let Ok(parsed) = val.parse::<bool>()
    {
        let push = cfg.push.get_or_insert_with(Default::default);
        push.enabled = parsed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::Value;
    use tempfile::NamedTempFile;

    #[test]
    fn migrates_legacy_config_to_current_version() {
        let legacy = r#"
jwt_secret: "secret"
dev_cors_origin: null
listen_port: 5151
users:
  - username: "parent"
    password_hash: "hash"
    role: parent
  - username: "child"
    password_hash: "hash"
    role: child
    child_id: "alice"
children:
  - id: "alice"
    display_name: "Alice"
tasks:
  - id: "task"
    name: "Task"
    minutes: 30
"#;

        let file = NamedTempFile::new().expect("tmp file");
        std::fs::write(file.path(), legacy).expect("write legacy config");

        migrate_config(file.path()).expect("migrate config");

        let text = std::fs::read_to_string(file.path()).expect("read migrated");
        let value: Value = serde_yaml::from_str(&text).expect("parse migrated");
        let mapping = value.as_mapping().expect("mapping root");

        assert_eq!(
            mapping
                .get(&Value::String("tenant_id".into()))
                .and_then(Value::as_str),
            Some("first")
        );
        let version = mapping
            .get(&Value::String("config_version".into()))
            .and_then(Value::as_str)
            .and_then(|s| Version::parse(s).ok());
        let current = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
        assert!(
            version.as_ref() == Some(&current)
                || version
                    .as_ref()
                    .map(|v| v.major == current.major && v.minor == current.minor)
                    .unwrap_or(false),
            "unexpected config_version: {:?}",
            version
        );

        assert!(matches!(
            mapping
                .get(&Value::String("push".into()))
                .and_then(Value::as_mapping),
            Some(push_map) if push_map.get(&Value::String("enabled".into())) == Some(&Value::Bool(false))
        ));
    }
}
