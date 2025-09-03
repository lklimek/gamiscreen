//! Minimal REST client helpers for consumers (e.g., platform agents).
//! Feature-gated by `rest-client` to avoid pulling reqwest in the server binary.

use super::endpoints as ep;
use super::*;

pub use reqwest::StatusCode;

#[derive(Debug, thiserror::Error)]
pub enum RestError {
    #[error("http: {0}")]
    Http(String),
    #[error("status {status}: {body}")]
    Status { status: u16, body: String },
    #[error("serde: {0}")]
    Serde(String),
}

fn mk_client() -> Result<reqwest::Client, RestError> {
    reqwest::Client::builder()
        .build()
        .map_err(|e| RestError::Http(e.to_string()))
}

async fn handle_json<T: for<'de> serde::Deserialize<'de>>(
    res: reqwest::Response,
) -> Result<T, RestError> {
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(RestError::Status {
            status: status.as_u16(),
            body,
        });
    }
    res.json::<T>()
        .await
        .map_err(|e| RestError::Serde(e.to_string()))
}

pub async fn login(base: &str, req: &AuthReq) -> Result<AuthResp, RestError> {
    let client = mk_client()?;
    let url = ep::auth_login(base);
    let res = client
        .post(url)
        .json(req)
        .send()
        .await
        .map_err(|e| RestError::Http(e.to_string()))?;
    handle_json(res).await
}

pub async fn child_register(
    base: &str,
    child_id: &str,
    device_id: &str,
    bearer: &str,
) -> Result<ClientRegisterResp, RestError> {
    let client = mk_client()?;
    let url = ep::child_register(base, child_id);
    let body = ClientRegisterReq {
        child_id: None,
        device_id: device_id.to_string(),
    };
    let res = client
        .post(url)
        .bearer_auth(bearer)
        .json(&body)
        .send()
        .await
        .map_err(|e| RestError::Http(e.to_string()))?;
    handle_json(res).await
}

// Convenience that takes minutes explicitly
pub async fn child_device_heartbeat_with_minutes(
    base: &str,
    child_id: &str,
    device_id: &str,
    bearer: &str,
    minutes: &[i64],
) -> Result<HeartbeatResp, RestError> {
    let client = mk_client()?;
    let url = ep::child_device_heartbeat(base, child_id, device_id);
    let body = HeartbeatReq {
        minutes: minutes.to_vec(),
    };
    let res = client
        .post(url)
        .bearer_auth(bearer)
        .json(&body)
        .send()
        .await
        .map_err(|e| RestError::Http(e.to_string()))?;
    handle_json(res).await
}

pub async fn child_reward(
    base: &str,
    child_id: &str,
    bearer: &str,
    body: &RewardReq,
) -> Result<RewardResp, RestError> {
    let client = mk_client()?;
    let url = ep::child_reward(base, child_id);
    let res = client
        .post(url)
        .bearer_auth(bearer)
        .json(body)
        .send()
        .await
        .map_err(|e| RestError::Http(e.to_string()))?;
    handle_json(res).await
}

pub async fn list_children(base: &str, bearer: &str) -> Result<Vec<ChildDto>, RestError> {
    let client = mk_client()?;
    let url = ep::children(base);
    let res = client
        .get(url)
        .bearer_auth(bearer)
        .send()
        .await
        .map_err(|e| RestError::Http(e.to_string()))?;
    handle_json(res).await
}

pub async fn list_tasks(base: &str, bearer: &str) -> Result<Vec<TaskDto>, RestError> {
    let client = mk_client()?;
    let url = ep::tasks(base);
    let res = client
        .get(url)
        .bearer_auth(bearer)
        .send()
        .await
        .map_err(|e| RestError::Http(e.to_string()))?;
    handle_json(res).await
}

pub async fn child_remaining(
    base: &str,
    child_id: &str,
    bearer: &str,
) -> Result<RemainingDto, RestError> {
    let client = mk_client()?;
    let url = ep::child_remaining(base, child_id);
    let res = client
        .get(url)
        .bearer_auth(bearer)
        .send()
        .await
        .map_err(|e| RestError::Http(e.to_string()))?;
    handle_json(res).await
}

pub async fn child_tasks(
    base: &str,
    child_id: &str,
    bearer: &str,
) -> Result<Vec<TaskWithStatusDto>, RestError> {
    let client = mk_client()?;
    let url = ep::child_tasks(base, child_id);
    let res = client
        .get(url)
        .bearer_auth(bearer)
        .send()
        .await
        .map_err(|e| RestError::Http(e.to_string()))?;
    handle_json(res).await
}
