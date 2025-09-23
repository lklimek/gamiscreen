use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::{self, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::auth::Role;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct JwtClaims {
    pub sub: String,
    pub jti: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub exp: i64,
    pub role: Role,
    pub child_id: Option<String>,
    pub device_id: Option<String>,
    pub tenant_id: String,
}

#[derive(Debug, Error)]
pub enum JwtError {
    #[error("invalid token: {0}")]
    Decode(String),
    #[error("missing tenant in token")]
    MissingTenant,
    #[error("encoding failed: {0}")]
    Encode(String),
}

pub fn decode_unverified(token: &str) -> Result<JwtClaims, JwtError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return Err(JwtError::Decode("invalid JWT format".into()));
    }
    let payload_b64 = parts[1];
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|e| JwtError::Decode(format!("invalid base64 payload: {e}")))?;
    serde_json::from_slice::<JwtClaims>(&payload_bytes)
        .map_err(|e| JwtError::Decode(format!("invalid json payload: {e}")))
}

pub fn decode_and_verify(token: &str, secret: &[u8]) -> Result<JwtClaims, JwtError> {
    let key = DecodingKey::from_secret(secret);
    let validation = Validation::new(Algorithm::HS256);
    jsonwebtoken::decode::<JwtClaims>(token, &key, &validation)
        .map(|data| data.claims)
        .map_err(|e| JwtError::Decode(e.to_string()))
}

pub fn encode(token: &JwtClaims, secret: &[u8]) -> Result<String, JwtError> {
    jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        token,
        &EncodingKey::from_secret(secret),
    )
    .map_err(|e| JwtError::Encode(e.to_string()))
}

pub fn tenant_id_from_token(token: &str) -> Result<String, JwtError> {
    let claims = decode_unverified(token)?;
    Ok(claims.tenant_id.clone())
}
