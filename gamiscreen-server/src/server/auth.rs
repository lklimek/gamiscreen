use axum::{http::Request, response::Response};
use axum::{http::header, middleware::Next};
use chrono::{Duration, Utc};
use tracing::{error, warn};

use gamiscreen_shared::auth::Role;
use gamiscreen_shared::jwt::{self, JwtClaims};

use super::{AppError, AppState};

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

    let mut claims = match jwt::decode_and_verify(token, state.config.jwt_secret.as_bytes()) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error=%e, "auth: jwt decode failed");
            return unauthorized();
        }
    };
    // TODO: enforce tenant_id presence in tokens in v0.8+
    if claims.tenant_id.is_empty() {
        tracing::debug!("auth: token missing tenant_id; defaulting to configured tenant");
        claims.tenant_id = state.config.tenant_id.clone();
    }

    if claims.tenant_id != state.config.tenant_id {
        tracing::warn!(
            token_tenant=%claims.tenant_id,
            config_tenant=%state.config.tenant_id,
            "auth: tenant mismatch"
        );
        return unauthorized();
    }
    let jti = claims.jti.clone();
    let session = state.store.get_session(&jti).await.map_err(|e| {
        tracing::error!(error=%e, jti=%jti, "auth: get_session failed");
        AppError::internal(e)
    })?;
    let Some(sess) = session else {
        return unauthorized();
    };
    let last = sess.last_used_at;
    let cutoff = Utc::now() - Duration::days(7);
    if last < cutoff.naive_utc() {
        return unauthorized();
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
    let exp = (Utc::now() + Duration::days(30)).timestamp();
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
            if let Some(device_id) = claims.device_id.as_deref() {
                if device_id.trim().is_empty() {
                    return Err(AppError::bad_request("device_id cannot be empty"));
                }
            }
        }
    }

    Ok(())
}
