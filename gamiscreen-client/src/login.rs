use std::io::{self, Write};
use std::path::PathBuf;

use crate::AppError;
use crate::config::{load_config, resolve_config_path};
use base64::Engine;
use gamiscreen_shared::api::{self};

pub async fn login(
    server_arg: Option<String>,
    username_arg: Option<String>,
    cfg_path_opt: Option<PathBuf>,
) -> Result<(), AppError> {
    // Resolve server url: CLI arg > config if present > prompt; normalize and strip trailing slash
    let server_url = if let Some(s) = server_arg {
        crate::config::normalize_server_url(&s)
    } else {
        let from_cfg = (|| {
            let p = resolve_config_path(cfg_path_opt.clone()).ok()?;
            let cfg = load_config(&p).ok()?;
            Some(crate::config::normalize_server_url(&cfg.server_url))
        })();
        match from_cfg {
            Some(s) => s,
            None => {
                crate::config::normalize_server_url(&prompt("Server URL (e.g., 127.0.0.1:5151): ")?)
            }
        }
    };

    let username = match username_arg {
        Some(u) => u,
        None => prompt("Username: ")?,
    };
    let password = rpassword::prompt_password("Password: ")
        .map_err(|e| AppError::Io(std::io::Error::other(e.to_string())))?;

    let body: api::AuthResp = match api::rest::login(
        &server_url,
        &api::AuthReq {
            username: username.clone(),
            password: password.clone(),
        },
    )
    .await
    {
        Ok(v) => v,
        Err(e) => return Err(AppError::Http(format!("login failed: {e}"))),
    };

    // Inspect token to determine role/child_id. If parent, prompt for child_id to register.
    let claims = decode_claims(&body.token)?;
    let target_child_id = match claims.role_str.as_deref() {
        Some(s) if s.eq_ignore_ascii_case("child") => claims
            .child_id
            .ok_or_else(|| AppError::Http("child token missing child_id".into()))?,
        Some(s) if s.eq_ignore_ascii_case("parent") => prompt("Child ID to register: ")?,
        _ => return Err(AppError::Http("unsupported role in token".into())),
    };

    // Register client to obtain device-scoped token, then write config
    let device_id = {
        let plat = crate::platform::detect_default().await.map_err(|e| {
            AppError::Io(std::io::Error::other(format!(
                "platform detect failed: {e}"
            )))
        })?;
        plat.device_id()
    };
    let reg = register_client(&server_url, &body.token, &target_child_id, &device_id).await?;
    // Save device token in keyring under the server_url only (single-user support)
    let entry = keyring_entry_for_login(&server_url)?;
    entry
        .set_password(&reg.token)
        .map_err(|e| AppError::Io(std::io::Error::other(e.to_string())))?;
    entry
        .get_password()
        .map_err(|e| AppError::Io(std::io::Error::other(e.to_string())))?;
    let cfg = crate::config::ClientConfig {
        server_url: server_url.clone(),
        child_id: reg.child_id,
        device_id: reg.device_id,
        interval_secs: 60,
        warn_before_lock_secs: 10,
    };
    let path = crate::config::default_config_path()
        .ok_or_else(|| AppError::Config("could not determine config dir".into()))?;
    crate::config::save_config(&path, &cfg)?;

    println!(
        "Saved token in keyring for {} and wrote config to {}",
        server_url,
        path.display()
    );
    Ok(())
}

fn prompt(msg: &str) -> Result<String, AppError> {
    print!("{}", msg);
    io::stdout().flush().ok();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf).map_err(AppError::Io)?;
    Ok(buf.trim().to_string())
}

type RegisterResp = api::ClientRegisterResp;

async fn register_client(
    server_url: &str,
    login_token: &str,
    child_id: &str,
    device_id: &str,
) -> Result<RegisterResp, AppError> {
    api::rest::child_register(server_url, child_id, device_id, login_token)
        .await
        .map_err(|e| AppError::Http(format!("registration failed: {e}")))
}

fn keyring_entry_for_login(server_url: &str) -> Result<keyring::Entry, AppError> {
    keyring::Entry::new("gamiscreen-client", server_url)
        .map_err(|e| AppError::Io(std::io::Error::other(e.to_string())))
}

#[derive(serde::Deserialize)]
struct JwtClaimsCheck {
    role: serde_json::Value,
    #[serde(default)]
    child_id: Option<String>,
}

struct DecodedClaims {
    role_str: Option<String>,
    child_id: Option<String>,
}

fn decode_claims(token: &str) -> Result<DecodedClaims, AppError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return Err(AppError::Http("invalid JWT format".into()));
    }
    let payload_b64 = parts[1];
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|e| AppError::Http(format!("invalid base64 payload: {e}")))
        .and_then(|bytes| {
            String::from_utf8(bytes).map_err(|e| AppError::Http(format!("invalid utf8: {e}")))
        })?;
    let claims: JwtClaimsCheck =
        serde_json::from_str(&payload).map_err(|e| AppError::Http(format!("invalid json: {e}")))?;
    let role_str = match claims.role.clone() {
        serde_json::Value::String(s) => Some(s),
        _ => None,
    };
    Ok(DecodedClaims {
        role_str,
        child_id: claims.child_id,
    })
}
