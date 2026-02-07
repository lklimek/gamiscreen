use eventsource_stream::{Event, EventStreamError, Eventsource};
use futures_util::StreamExt;
use gamiscreen_shared::jwt::JwtClaims;
use reqwest::header::ACCEPT;
use tokio::sync::broadcast;
use tokio::task::JoinSet;

use crate::AppError;

/// Clone-friendly SSE hub with multi-consumer broadcast of server events.
#[derive(Clone)]
pub struct SseHub {
    tx: broadcast::Sender<gamiscreen_shared::api::ServerEvent>,
    _joinset: std::sync::Arc<tokio::sync::Mutex<JoinSet<()>>>,
}

impl SseHub {
    /// Creates a new SSE hub, starts the worker and records its handle in a JoinSet.
    pub fn new(server_base: &str, token: &str, claims: &JwtClaims) -> Result<Self, AppError> {
        let base = crate::config::normalize_server_url(server_base);
        if base.is_empty() {
            return Err(AppError::Config("SSE: server_base empty".into()));
        }
        let url = to_sse_url(&base, &claims.tenant_id, token)?;

        let (tx, _) = broadcast::channel(64);
        let mut js = JoinSet::new();
        let url_cloned = url.clone();
        let tx_cloned = tx.clone();

        js.spawn(async move {
            let client = reqwest::Client::new();
            let mut backoff_secs = 1u64;
            let mut last_event_id: Option<String> = None;
            let mut server_retry: Option<std::time::Duration> = None;
            loop {
                let mut builder = client
                    .get(url_cloned.clone())
                    .header(ACCEPT, "text/event-stream");
                if let Some(id) = last_event_id.as_deref() {
                    builder = builder.header("Last-Event-ID", id);
                }

                match builder.send().await {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            tracing::warn!(status=%resp.status(), "SSE: non-success response");
                        } else {
                            tracing::info!("SSE: connected");
                            let mut stream = resp.bytes_stream().eventsource();
                            while let Some(ev) = stream.next().await {
                                match ev {
                                    Ok(Event { data, id, retry, .. }) => {
                                        if !id.is_empty() {
                                            last_event_id = Some(id);
                                        }
                                        if let Some(retry) = retry {
                                            server_retry = Some(retry);
                                        }
                                        if !data.is_empty() {
                                            match serde_json::from_str::<gamiscreen_shared::api::ServerEvent>(&data) {
                                                Ok(val) => {
                                                    tracing::trace!(event=?val, "SSE: received event");
                                                    let _ = tx_cloned.send(val);
                                                }
                                                Err(e) => tracing::warn!(error=%e, "SSE: failed to parse event"),
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        match e {
                                            EventStreamError::Utf8(err) => {
                                                tracing::warn!(error=%err, "SSE: invalid utf8")
                                            }
                                            EventStreamError::Parser(err) => {
                                                tracing::warn!(error=%err, "SSE: parse error")
                                            }
                                            EventStreamError::Transport(err) => {
                                                tracing::warn!(error=%err, "SSE: transport error")
                                            }
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error=%e, "SSE: connect failed");
                    }
                }

                let sleep_for = server_retry.unwrap_or_else(|| std::time::Duration::from_secs(backoff_secs));
                tokio::time::sleep(sleep_for).await;
                if server_retry.is_some() {
                    server_retry = None;
                    backoff_secs = 1;
                } else {
                    backoff_secs = std::cmp::min(backoff_secs * 2, 30);
                }
            }
        });

        Ok(Self {
            tx,
            _joinset: std::sync::Arc::new(tokio::sync::Mutex::new(js)),
        })
    }

    /// Subscribe to events.
    pub fn subscribe(&self) -> broadcast::Receiver<gamiscreen_shared::api::ServerEvent> {
        self.tx.subscribe()
    }
}

fn to_sse_url(http_base: &str, tenant_id: &str, token: &str) -> Result<String, AppError> {
    let mut u = url::Url::parse(http_base)
        .map_err(|e| AppError::Config(format!("invalid server_url: {e}")))?;
    // keep http/https
    let mut path = u.path().trim_end_matches('/').to_string();
    let scope = gamiscreen_shared::api::tenant_scope(tenant_id);
    path.push_str(&format!("{}/sse", scope));
    u.set_path(&path);
    let mut qp = u.query_pairs_mut();
    qp.clear();
    qp.append_pair("token", token);
    drop(qp);
    Ok(u.into())
}
