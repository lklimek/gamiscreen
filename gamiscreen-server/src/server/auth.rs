use axum::{http::Request, response::Response};
use axum::{http::header, middleware::Next};
use chrono::{Duration, Utc};
use tracing::error;

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

    let claims = match jwt::decode_and_verify(token, state.config.jwt_secret.as_bytes()) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error=%e, "auth: jwt decode failed");
            return unauthorized();
        }
    };
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
    state
        .store
        .touch_session(&jti)
        .await
        .map_err(AppError::internal)?;

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
) -> Result<String, AppError> {
    let jti = uuid::Uuid::new_v4().to_string();
    state
        .store
        .create_session(&jti, username)
        .await
        .map_err(|e| {
            error!(username, error=%e, "login/register: create_session failed");
            AppError::internal(e)
        })?;
    let exp = (Utc::now() + Duration::days(30)).timestamp();
    let claims = JwtClaims {
        sub: username.to_string(),
        jti: jti.clone(),
        exp,
        role,
        child_id,
        device_id,
        tenant_id: state.config.tenant_id.clone(),
    };
    let token = jwt::encode(&claims, state.config.jwt_secret.as_bytes()).map_err(|e| {
        error!(username, error=%e, "login/register: jwt encode failed");
        AppError::internal(e)
    })?;
    Ok(token)
}
