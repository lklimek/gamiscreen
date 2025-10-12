use std::net::IpAddr;
use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::AppError;

pub const ENV_CONFIG: &str = "GAMISCREEN_CONFIG";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub server_url: String,
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
    if trimmed.is_empty() {
        return String::new();
    }
    let trimmed = trimmed.trim_end_matches('/');
    let lowered = trimmed.to_ascii_lowercase();

    if let Some(pos) = lowered.find("://") {
        let scheme = &lowered[..pos];
        if scheme == "http" || scheme == "https" {
            let rest = trimmed[pos + 3..].trim_end_matches('/');
            return format!("{}://{}", scheme, rest);
        }
    }

    let scheme = default_scheme(trimmed);
    format!("{}://{}", scheme, trimmed)
}

fn default_scheme(url_without_scheme: &str) -> &'static str {
    let authority = url_without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(url_without_scheme);

    let (host_part, port) = parse_authority(authority);

    if let Some(port) = port {
        if port == 443 {
            return "https";
        }
        return "http";
    }

    let host = host_part.trim_matches(|c| c == '[' || c == ']');

    if host.eq_ignore_ascii_case("localhost") {
        return "http";
    }
    if host.parse::<IpAddr>().is_ok() {
        return "http";
    }

    "https"
}

fn parse_authority(authority: &str) -> (&str, Option<u16>) {
    if authority.starts_with('[') {
        if let Some(end) = authority.find(']') {
            let host = &authority[..=end];
            let rest = &authority[end + 1..];
            if let Some(port_str) = rest.strip_prefix(':') {
                if let Ok(port) = port_str.parse::<u16>() {
                    return (host, Some(port));
                }
            }
            return (host, None);
        }
        return (authority, None);
    }

    if let Some(idx) = authority.rfind(':') {
        let port_part = &authority[idx + 1..];
        if !port_part.is_empty() && port_part.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(port) = port_part.parse::<u16>() {
                return (&authority[..idx], Some(port));
            }
        }
    }

    (authority, None)
}

impl ClientConfig {
    /// Resolves the config path from CLI arg, env, or default location and loads it.
    /// Returns the resolved path and the loaded config.
    pub fn find_and_load(cli_path: Option<PathBuf>) -> Result<(PathBuf, ClientConfig), AppError> {
        let path = resolve_config_path(cli_path)?;
        let cfg = load_config(&path)?;
        Ok((path, cfg))
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_server_url;

    #[test]
    fn keeps_explicit_https() {
        assert_eq!(
            normalize_server_url("https://example.com/"),
            "https://example.com"
        );
        assert_eq!(
            normalize_server_url("HTTPS://Example.com/path/"),
            "https://Example.com/path"
        );
    }

    #[test]
    fn keeps_explicit_http() {
        assert_eq!(
            normalize_server_url("http://example.com"),
            "http://example.com"
        );
        assert_eq!(
            normalize_server_url("HTTP://example.com/api/"),
            "http://example.com/api"
        );
    }

    #[test]
    fn defaults_to_https_for_domains() {
        assert_eq!(normalize_server_url("example.com"), "https://example.com");
        assert_eq!(
            normalize_server_url("example.com/path"),
            "https://example.com/path"
        );
    }

    #[test]
    fn respects_ports_and_local_addresses() {
        assert_eq!(
            normalize_server_url("example.com:443"),
            "https://example.com:443"
        );
        assert_eq!(
            normalize_server_url("example.com:8080"),
            "http://example.com:8080"
        );
        assert_eq!(normalize_server_url("localhost"), "http://localhost");
        assert_eq!(
            normalize_server_url("127.0.0.1:5151"),
            "http://127.0.0.1:5151"
        );
        assert_eq!(
            normalize_server_url("[2001:db8::1]"),
            "http://[2001:db8::1]"
        );
        assert_eq!(
            normalize_server_url("[2001:db8::1]:443"),
            "https://[2001:db8::1]:443"
        );
    }
}
