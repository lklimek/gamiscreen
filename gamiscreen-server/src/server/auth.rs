use axum::{http::Request, response::Response};
use axum::{http::header, middleware::Next};
use chrono::{Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use tracing::error;

use super::{AppError, AppState, Role};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JwtClaims {
    pub sub: String,
    pub jti: String,
    pub exp: i64,
    pub role: Role,
    pub child_id: Option<String>,
    pub device_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AuthCtx {
    pub username: String,
    pub role: Role,
    pub child_id: Option<String>,
    pub device_id: Option<String>,
    pub jti: String,
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

    let decoding = DecodingKey::from_secret(state.config.jwt_secret.as_bytes());
    let validation = Validation::new(Algorithm::HS256);
    let data = match decode::<JwtClaims>(token, &decoding, &validation) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(error=%e, "auth: jwt decode failed");
            return unauthorized();
        }
    };
    let jti = data.claims.jti.clone();
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

    let auth = AuthCtx {
        username: data.claims.sub,
        role: data.claims.role,
        child_id: data.claims.child_id,
        device_id: data.claims.device_id,
        jti,
    };
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
    };
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(state.config.jwt_secret.as_bytes()),
    )
    .map_err(|e| {
        error!(username, error=%e, "login/register: jwt encode failed");
        AppError::internal(e)
    })?;
    Ok(token)
}
