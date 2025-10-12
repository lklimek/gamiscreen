//! Minimal REST client helpers for consumers (clients).

use super::endpoints as ep;
use super::*;
use once_cell::sync::Lazy;
use std::time::Duration;

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

static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        // Keep TCP connections alive at kernel level
        .tcp_keepalive(Some(Duration::from_secs(180)))
        // Enable and tune the connection pool
        .pool_max_idle_per_host(4)
        .pool_idle_timeout(Duration::from_secs(180))
        // Bound request duration
        .timeout(Duration::from_secs(180))
        .build()
        .expect("failed to build HTTP client")
});

fn mk_client() -> Result<reqwest::Client, RestError> {
    Ok(HTTP_CLIENT.clone())
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

pub async fn renew_token(base: &str, bearer: &str) -> Result<AuthResp, RestError> {
    let client = mk_client()?;
    let url = ep::auth_renew(base);
    let res = client
        .post(url)
        .bearer_auth(bearer)
        .send()
        .await
        .map_err(|e| RestError::Http(e.to_string()))?;
    handle_json(res).await
}

pub async fn child_register(
    base: &str,
    tenant_id: &str,
    child_id: &str,
    device_id: &str,
    bearer: &str,
) -> Result<ClientRegisterResp, RestError> {
    let client = mk_client()?;
    let url = ep::child_register(base, tenant_id, child_id);
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

pub async fn child_device_heartbeat_with_minutes(
    base: &str,
    tenant_id: &str,
    child_id: &str,
    device_id: &str,
    bearer: &str,
    minutes: &[i64],
) -> Result<HeartbeatResp, RestError> {
    let client = mk_client()?;
    let url = ep::child_device_heartbeat(base, tenant_id, child_id, device_id);
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
    tenant_id: &str,
    child_id: &str,
    bearer: &str,
    body: &RewardReq,
) -> Result<RewardResp, RestError> {
    let client = mk_client()?;
    let url = ep::child_reward(base, tenant_id, child_id);
    let res = client
        .post(url)
        .bearer_auth(bearer)
        .json(body)
        .send()
        .await
        .map_err(|e| RestError::Http(e.to_string()))?;
    handle_json(res).await
}

pub async fn list_children(
    base: &str,
    tenant_id: &str,
    bearer: &str,
) -> Result<Vec<ChildDto>, RestError> {
    let client = mk_client()?;
    let url = ep::children(base, tenant_id);
    let res = client
        .get(url)
        .bearer_auth(bearer)
        .send()
        .await
        .map_err(|e| RestError::Http(e.to_string()))?;
    handle_json(res).await
}

pub async fn list_tasks(
    base: &str,
    tenant_id: &str,
    bearer: &str,
) -> Result<Vec<TaskDto>, RestError> {
    let client = mk_client()?;
    let url = ep::tasks(base, tenant_id);
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
    tenant_id: &str,
    child_id: &str,
    bearer: &str,
) -> Result<RemainingDto, RestError> {
    let client = mk_client()?;
    let url = ep::child_remaining(base, tenant_id, child_id);
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
    tenant_id: &str,
    child_id: &str,
    bearer: &str,
) -> Result<Vec<TaskWithStatusDto>, RestError> {
    let client = mk_client()?;
    let url = ep::child_tasks(base, tenant_id, child_id);
    let res = client
        .get(url)
        .bearer_auth(bearer)
        .send()
        .await
        .map_err(|e| RestError::Http(e.to_string()))?;
    handle_json(res).await
}

pub async fn server_version(base: &str) -> Result<VersionInfoDto, RestError> {
    let client = mk_client()?;
    let url = ep::version(base);
    let res = client
        .get(url)
        .send()
        .await
        .map_err(|e| RestError::Http(e.to_string()))?;
    handle_json(res).await
}

pub async fn push_subscribe(
    base: &str,
    tenant_id: &str,
    child_id: &str,
    bearer: &str,
    req: &PushSubscribeReq,
) -> Result<PushSubscribeResp, RestError> {
    let client = mk_client()?;
    let url = ep::child_push_subscribe(base, tenant_id, child_id);
    let res = client
        .post(url)
        .bearer_auth(bearer)
        .json(req)
        .send()
        .await
        .map_err(|e| RestError::Http(e.to_string()))?;
    handle_json(res).await
}

pub async fn push_unsubscribe(
    base: &str,
    tenant_id: &str,
    child_id: &str,
    bearer: &str,
    req: &PushUnsubscribeReq,
) -> Result<(), RestError> {
    let client = mk_client()?;
    let url = ep::child_push_unsubscribe(base, tenant_id, child_id);
    let res = client
        .post(url)
        .bearer_auth(bearer)
        .json(req)
        .send()
        .await
        .map_err(|e| RestError::Http(e.to_string()))?;
    if res.status().is_success() {
        Ok(())
    } else {
        let status = res.status().as_u16();
        let body = res.text().await.unwrap_or_default();
        Err(RestError::Status { status, body })
    }
}
