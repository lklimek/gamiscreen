use axum::http::{Request, header};
use axum::middleware::Next;
use axum::response::Response;
use chrono::{Duration, Utc};
use gamiscreen_shared::auth::Role;
use gamiscreen_shared::jwt::{self, JwtClaims};
use tracing::{error, warn};

use super::{AppError, AppState};

/// How many days of inactivity before a user session is considered expired.
const USER_SESSION_IDLE_DAYS: i64 = 14;
/// How many days before mandatory re-login for users.
const USER_TOKEN_TTL_DAYS: i64 = 30;
/// How many days of inactivity before a device session is considered expired.
const DEVICE_SESSION_IDLE_DAYS: i64 = 30;
/// How many days before mandatory re-login for devices.
const DEVICE_TOKEN_TTL_DAYS: i64 = 2 * DEVICE_SESSION_IDLE_DAYS;

#[derive(Clone, Debug)]
pub struct AuthCtx {
    pub claims: JwtClaims,
}

pub async fn require_bearer(
    axum::extract::State(state): axum::extract::State<AppState>,
    mut req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, AppError> {
    let unauthorized = || Err(AppError::unauthorized());
    let header_val = match req.headers().get(header::AUTHORIZATION) {
        Some(v) => v,
        None => return unauthorized(),
    };
    let header_str = header_val.to_str().map_err(|_| AppError::unauthorized())?;
    let prefix = "Bearer ";
    if !header_str.starts_with(prefix) {
        return unauthorized();
    }
    let token = &header_str[prefix.len()..];

    let claims = match jwt::decode_and_verify(token, state.config.jwt_secret.as_bytes()) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error=%e, "auth: jwt decode failed");
            return unauthorized();
        }
    };

    validate_claims(&state, &claims).map_err(|e| {
        tracing::warn!(error=?e, username=%claims.sub, "auth: validate_claims failed");
        // Invalid token, log out the user
        AppError::unauthorized()
    })?;

    if claims.tenant_id != state.config.tenant_id {
        tracing::warn!(
            token_tenant=%claims.tenant_id,
            config_tenant=%state.config.tenant_id,
            "auth: tenant mismatch"
        );
        return unauthorized();
    }
    let jti = claims.jti.clone();
    let idle_days = if claims.device_id.is_some() {
        DEVICE_SESSION_IDLE_DAYS
    } else {
        USER_SESSION_IDLE_DAYS
    };
    let cutoff = Utc::now() - Duration::days(idle_days);
    match state
        .store
        .touch_session_with_cutoff(&jti, cutoff.naive_utc())
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            tracing::warn!(
                jti = %jti,
                username = %claims.sub,
                cutoff = %cutoff,
                idle_days = idle_days,
                "auth: session missing or expired (last_used_at < cutoff)"
            );
            return unauthorized();
        }
        Err(e) => {
            error!(jti = %jti, error=%e, "auth: touch_session_with_cutoff failed");
            return Err(AppError::internal(e));
        }
    }
    let auth = AuthCtx { claims };
    req.extensions_mut().insert(auth);
    Ok(next.run(req).await)
}

pub async fn issue_jwt_for_user(
    state: &AppState,
    username: &str,
    role: Role,
    child_id: Option<String>,
    device_id: Option<String>,
    tenant_id: &str,
) -> Result<String, AppError> {
    let jti = uuid::Uuid::new_v4().to_string();
    let ttl_days = if device_id.is_some() {
        DEVICE_TOKEN_TTL_DAYS
    } else {
        USER_TOKEN_TTL_DAYS
    };
    let exp = (Utc::now() + Duration::days(ttl_days)).timestamp();
    let claims = JwtClaims {
        sub: username.to_string(),
        jti: jti.clone(),
        exp,
        role,
        child_id,
        device_id,
        tenant_id: tenant_id.to_string(),
    };

    validate_claims(state, &claims)?;

    state
        .store
        .create_session(&jti, username)
        .await
        .map_err(|e| {
            error!(username, error=%e, "login/register: create_session failed");
            AppError::internal(e)
        })?;
    let token = jwt::encode(&claims, state.config.jwt_secret.as_bytes()).map_err(|e| {
        error!(username, error=%e, "login/register: jwt encode failed");
        AppError::internal(e)
    })?;
    Ok(token)
}

fn validate_claims(state: &AppState, claims: &JwtClaims) -> Result<(), AppError> {
    if claims.tenant_id != state.config.tenant_id {
        warn!(
            username = %claims.sub,
            requested_tenant = %claims.tenant_id,
            configured_tenant = %state.config.tenant_id,
            "issue_jwt: tenant mismatch"
        );
        return Err(AppError::forbidden());
    }
    let user = state
        .config
        .users
        .iter()
        .find(|u| u.username == claims.sub)
        .ok_or_else(|| {
            warn!(username = %claims.sub, "issue_jwt: unknown user");
            AppError::forbidden()
        })?;

    match claims.role {
        Role::Parent => {
            if user.role != Role::Parent {
                warn!(
                    username = %claims.sub,
                    requested_role = ?claims.role,
                    actual_role = ?user.role,
                    "issue_jwt: role mismatch"
                );
                return Err(AppError::forbidden());
            }
            if claims.child_id.is_some() || claims.device_id.is_some() {
                warn!(
                    username = %claims.sub,
                    "issue_jwt: parent token must not include child or device"
                );
                return Err(AppError::forbidden());
            }
        }
        Role::Child => {
            if user.role != Role::Child {
                warn!(
                    username = %claims.sub,
                    requested_role = ?claims.role,
                    actual_role = ?user.role,
                    "issue_jwt: role mismatch"
                );
                return Err(AppError::forbidden());
            }
            let child_id = claims.child_id.as_deref().ok_or_else(|| {
                warn!(username = %claims.sub, "issue_jwt: child token missing child_id");
                AppError::forbidden()
            })?;
            let expected_child = user.child_id.as_deref().ok_or_else(|| {
                warn!(
                    username = %claims.sub,
                    "issue_jwt: user missing child binding in config"
                );
                AppError::forbidden()
            })?;
            if expected_child != child_id {
                warn!(
                    username = %claims.sub,
                    expected = expected_child,
                    requested = child_id,
                    "issue_jwt: child mismatch"
                );
                return Err(AppError::forbidden());
            }
            if !state.config.children.iter().any(|c| c.id == child_id) {
                warn!(child_id, "issue_jwt: child not configured");
                return Err(AppError::not_found(format!(
                    "child not found: {}",
                    child_id
                )));
            }
            if let Some(device_id) = claims.device_id.as_deref()
                && device_id.trim().is_empty()
            {
                return Err(AppError::bad_request("device_id cannot be empty"));
            }
        }
    }

    Ok(())
}
