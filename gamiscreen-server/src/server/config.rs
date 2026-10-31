use std::path::Path;
use std::{env, fs};

pub use gamiscreen_shared::auth::Role;
use gamiscreen_shared::domain::{Child, Task};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tracing::warn;

type Mapping = Map<String, Value>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default)]
    pub config_version: String,
    #[serde(default = "default_tenant_id")]
    pub tenant_id: String,
    pub children: Vec<Child>,
    #[serde(default)]
    pub tasks: Vec<Task>,
    pub jwt_secret: String,
    pub users: Vec<UserConfig>,
    pub dev_cors_origin: Option<String>,
    pub listen_port: Option<u16>,
    #[serde(default)]
    pub push: Option<PushConfig>,
    #[serde(default)]
    pub timezone: Option<String>,
    /// Parsed timezone, populated after config load via `resolve_timezone()`.
    #[serde(skip)]
    pub family_tz: chrono_tz::Tz,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserConfig {
    pub username: String,
    pub password_hash: String, // bcrypt hash
    pub role: Role,
    pub child_id: Option<String>, // required when role == child
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
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
    Yaml(serde_saphyr::Error),
    SerializeYaml(serde_saphyr::SerializeError),
    SerializeValues(serde_json::Error),
    Invalid(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "IO error: {}", e),
            ConfigError::Yaml(e) => {
                write!(f, "invalid YAML configuration")?;
                if let Some(location) = e.location() {
                    write!(
                        f,
                        " at line {} column {}",
                        location.line(),
                        location.column()
                    )?;
                }
                Ok(())
            }
            ConfigError::SerializeYaml(_) | ConfigError::SerializeValues(_) => {
                write!(f, "failed to serialize configuration")
            }
            ConfigError::Invalid(e) => write!(f, "invalid config: {}", e),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Yaml(error) => Some(error),
            Self::SerializeYaml(error) => Some(error),
            Self::SerializeValues(error) => Some(error),
            Self::Invalid(_) => None,
        }
    }
}

fn default_tenant_id() -> String {
    "first".to_owned()
}

fn parse_config<'de, T: Deserialize<'de>>(text: &'de str) -> Result<T, serde_saphyr::Error> {
    let mut options = serde_saphyr::Options::default();
    options.with_snippet = false;
    serde_saphyr::from_str_with_options(text, options)
}

impl From<std::io::Error> for ConfigError {
    fn from(value: std::io::Error) -> Self {
        ConfigError::Io(value)
    }
}

impl From<serde_saphyr::Error> for ConfigError {
    fn from(value: serde_saphyr::Error) -> Self {
        ConfigError::Yaml(value)
    }
}

impl From<serde_saphyr::SerializeError> for ConfigError {
    fn from(value: serde_saphyr::SerializeError) -> Self {
        Self::SerializeYaml(value)
    }
}

impl From<serde_json::Error> for ConfigError {
    fn from(value: serde_json::Error) -> Self {
        Self::SerializeValues(value)
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
        let mut cfg: AppConfig = parse_config(&text)?;
        apply_env_overrides(&mut cfg);
        cfg.resolve_timezone()?;
        Ok(cfg)
    }

    /// Parse the `timezone` string into a `chrono_tz::Tz`. Defaults to UTC when absent.
    fn resolve_timezone(&mut self) -> Result<(), ConfigError> {
        self.family_tz = match &self.timezone {
            Some(tz_str) => tz_str.parse::<chrono_tz::Tz>().map_err(|_| {
                ConfigError::Invalid(format!(
                    "invalid timezone '{}': expected IANA timezone like 'Europe/Warsaw' or 'UTC'",
                    tz_str
                ))
            })?,
            None => chrono_tz::UTC,
        };
        Ok(())
    }
}

type MigrationFn = fn(&mut Mapping) -> bool;

const MIGRATIONS: &[(&str, MigrationFn)] =
    &[("0.7.0", migrate_to_0_7_0), ("0.9.0", migrate_to_0_9_0)];

fn migrate_config(path: &Path) -> Result<(), ConfigError> {
    let text = fs::read_to_string(path)?;
    let mut doc: Value = parse_config(&text)?;

    // Schema-directed strings retain their original spelling before YAML is rewritten.
    let typed: AppConfig = parse_config(&text)?;
    preserve_schema_values(&mut doc, &serde_json::to_value(typed)?);

    let Some(mapping) = doc.as_object_mut() else {
        return Ok(());
    };

    let version_key = "config_version".to_string();

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
        let updated = serde_saphyr::to_string(&doc)?;
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

fn preserve_schema_values(value: &mut Value, typed: &Value) {
    match (value, typed) {
        (Value::Object(values), Value::Object(fields)) => {
            for (key, typed) in fields {
                if let Some(value) = values.get_mut(key) {
                    preserve_schema_values(value, typed);
                }
            }
        }
        (Value::Array(values), Value::Array(fields)) => {
            for (value, typed) in values.iter_mut().zip(fields) {
                preserve_schema_values(value, typed);
            }
        }
        (value, typed) => *value = typed.clone(),
    }
}

fn migrate_to_0_7_0(map: &mut Mapping) -> bool {
    let mut changed = false;

    let tenant_key = "tenant_id".to_string();
    if !map.contains_key(&tenant_key) {
        map.insert(tenant_key, Value::String("first".to_string()));
        changed = true;
    }

    changed
}

fn migrate_to_0_9_0(map: &mut Mapping) -> bool {
    let push_key = "push".to_string();
    if map.contains_key(&push_key) {
        return false;
    }

    let mut push_map = Mapping::new();
    push_map.insert("enabled".to_string(), Value::Bool(false));

    map.insert(push_key, Value::Object(push_map));
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
    use serde_json::Value;
    use tempfile::NamedTempFile;

    use super::*;

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
        let value: Value = serde_saphyr::from_str(&text).expect("parse migrated");
        let mapping = value.as_object().expect("mapping root");

        assert_eq!(
            mapping.get("tenant_id").and_then(Value::as_str),
            Some("first")
        );
        let version = mapping
            .get("config_version")
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
                .get("push")
                .and_then(Value::as_object),
            Some(push_map) if push_map.get("enabled") == Some(&Value::Bool(false))
        ));
    }
    #[test]
    fn upgrade_preserves_schema_string_values() {
        let original = "config_version: '0.13.0'\ntenant_id: no\njwt_secret: 00123\nchildren:\n  - id: 00123\n    display_name: NO\nusers:\n  - username: on\n    password_hash: fake-hash\n    role: child\n    child_id: 00123\ntasks:\n  - id: off\n    name: yes\n    minutes: 5\n    required: true\ncustom: {retained: 'extra'}\n";
        let file = NamedTempFile::new().unwrap();
        fs::write(file.path(), original).unwrap();
        let cfg = AppConfig::load_from_path(file.path()).unwrap();
        assert_eq!(cfg.tenant_id, "no");
        assert_eq!(cfg.jwt_secret, "00123");
        assert_eq!(cfg.children[0].id, "00123");
        assert_eq!(cfg.children[0].display_name, "NO");
        assert_eq!(cfg.users[0].username, "on");
        assert_eq!(cfg.users[0].child_id.as_deref(), Some("00123"));
        assert_eq!(cfg.tasks[0].id, "off");
        assert_eq!(cfg.tasks[0].name, "yes");
        assert!(cfg.tasks[0].required);
        let text = fs::read_to_string(file.path()).unwrap();
        let migrated: Value = serde_saphyr::from_str(&text).unwrap();
        assert_eq!(migrated["custom"]["retained"], "extra");
        assert_eq!(migrated["children"][0]["id"], "00123");
        assert_eq!(migrated["config_version"], env!("CARGO_PKG_VERSION"));
        let again = AppConfig::load_from_path(file.path()).unwrap();
        assert_eq!(again.jwt_secret, cfg.jwt_secret);
        assert_eq!(fs::read_to_string(file.path()).unwrap(), text);
    }

    #[test]
    fn malformed_yaml_diagnostics_do_not_expose_neighboring_secrets() {
        let file = NamedTempFile::new().unwrap();
        fs::write(
            file.path(),
            "jwt_secret: review-sentinel-never-log\nlisten_port: [unterminated\n",
        )
        .unwrap();
        let error = AppConfig::load_from_path(file.path()).unwrap_err();
        let rendered = error.to_string();
        assert!(
            !rendered.contains("review-sentinel-never-log"),
            "{rendered}"
        );
        assert!(!format!("{error:?}").contains("review-sentinel-never-log"));
        assert!(rendered.contains("line 2"), "{rendered}");
    }

    #[test]
    fn typed_yaml_errors_do_not_render_values() {
        let file = NamedTempFile::new().unwrap();
        fs::write(file.path(), format!("config_version: '{}'\ntenant_id: family\njwt_secret: review-sentinel-never-log\nchildren: []\nusers: []\nlisten_port: not-a-port\n", env!("CARGO_PKG_VERSION"))).unwrap();
        let error = AppConfig::load_from_path(file.path()).unwrap_err();
        assert!(!error.to_string().contains("review-sentinel-never-log"));
        assert!(!error.to_string().contains("not-a-port"));
    }
}
