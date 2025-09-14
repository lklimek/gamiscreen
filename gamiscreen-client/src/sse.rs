use futures_util::StreamExt;
use reqwest_eventsource::{Event, EventSource};
use tokio::{sync::broadcast, task::JoinSet};

use crate::AppError;

/// Clone-friendly SSE hub with multi-consumer broadcast of server events.
#[derive(Clone)]
pub struct SseHub {
    tx: broadcast::Sender<gamiscreen_shared::api::ServerEvent>,
    _joinset: std::sync::Arc<tokio::sync::Mutex<JoinSet<()>>>,
}

impl SseHub {
    /// Creates a new SSE hub, starts the worker and records its handle in a JoinSet.
    pub fn new(server_base: &str, token: &str) -> Result<Self, AppError> {
        let base = crate::config::normalize_server_url(server_base);
        if base.is_empty() {
            return Err(AppError::Config("SSE: server_base empty".into()));
        }
        let url = to_sse_url(&base, token)?;

        let (tx, _) = broadcast::channel(64);
        let mut js = JoinSet::new();
        let url_cloned = url.clone();
        let tx_cloned = tx.clone();

        js.spawn(async move {
            let client = reqwest::Client::new();
            let mut backoff_secs = 1u64;
            loop {
                let builder = client.get(url_cloned.clone());
                match EventSource::new(builder) {
                    Ok(mut es) => {
                        tracing::info!("SSE: connected");
                        while let Some(ev) = es.next().await {
                            match ev {
                                Ok(Event::Message(msg)) => {
                                    if !msg.data.is_empty() {
                                        match serde_json::from_str::<gamiscreen_shared::api::ServerEvent>(&msg.data) {
                                            Ok(val) => {                                                
                                                tracing::trace!(event=?val, "SSE: received event");
                                                let _ = tx_cloned.send(val); }
                                            Err(e) => tracing::warn!(error=%e, "SSE: failed to parse event"),
                                        }
                                    }
                                }
                                Ok(Event::Open) => {}
                                Err(e) => {
                                    tracing::warn!(error=%e, "SSE read error");
                                    es.close();
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error=%e, "SSE: connect failed");
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                backoff_secs = std::cmp::min(backoff_secs * 2, 30);
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

fn to_sse_url(http_base: &str, token: &str) -> Result<String, AppError> {
    let mut u = url::Url::parse(http_base)
        .map_err(|e| AppError::Config(format!("invalid server_url: {e}")))?;
    // keep http/https
    let mut path = u.path().trim_end_matches('/').to_string();
    path.push_str("/api/sse");
    u.set_path(&path);
    let mut qp = u.query_pairs_mut();
    qp.clear();
    qp.append_pair("token", token);
    drop(qp);
    Ok(u.into())
}
