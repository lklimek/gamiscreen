use futures_util::StreamExt;
use reqwest_eventsource::{Event, EventSource};

use crate::AppError;

/// Spawn a background SSE listener that connects to the server and listens for RemainingUpdated events.
/// When remaining_minutes > 0, disables the re-locker.
pub(crate) fn spawn_sse_listener(server_base: &str, token: &str, relocker: super::ReLocker) {
    let base = crate::config::normalize_server_url(server_base);
    if base.is_empty() {
        tracing::warn!("SSE: server_base empty; skipping listener");
        return;
    }
    let url = match to_sse_url(&base, token) {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!(error=%e, "SSE: failed to build URL; skipping");
            return;
        }
    };

    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut backoff_secs = 1u64;
        loop {
            let builder = client.get(url.clone());
            match EventSource::new(builder) {
                Ok(mut es) => {
                    tracing::info!("SSE: connected");
                    while let Some(ev) = es.next().await {
                        match ev {
                            Ok(Event::Message(msg)) => {
                                if !msg.data.is_empty() {
                                    if let Ok(val) = serde_json::from_str::<gamiscreen_shared::api::ServerEvent>(&msg.data) {
                                        if let gamiscreen_shared::api::ServerEvent::RemainingUpdated { remaining_minutes, .. } = val {
                                            if remaining_minutes > 0 { relocker.disable().await; }
                                        }
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
}

fn to_sse_url(http_base: &str, token: &str) -> Result<String, AppError> {
    let mut u = url::Url::parse(http_base).map_err(|e| AppError::Config(format!("invalid server_url: {e}")))?;
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

