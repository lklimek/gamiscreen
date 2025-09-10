use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::{Bytes, protocol::Message};

use crate::AppError;
use gamiscreen_shared::api::ServerEvent;

/// Spawn a background WebSocket listener that connects to the server and listens for RemainingUpdated events.
/// When remaining_minutes > 0, disables the re-locker.
pub(crate) fn spawn_ws_listener(server_base: &str, token: &str, relocker: super::ReLocker) {
    let base = crate::config::normalize_server_url(server_base);
    if base.is_empty() {
        tracing::warn!("WS: server_base empty; skipping websocket listener");
        return;
    }
    let url = match to_ws_url(&base, token) {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!(error=%e, "WS: failed to build URL; skipping");
            return;
        }
    };

    tokio::spawn(async move {
        let mut backoff_secs = 1u64;
        loop {
            match tokio_tungstenite::connect_async(url.as_str()).await {
                Ok((mut ws, _)) => {
                    tracing::info!("WS: connected");
                    // Optionally request initial ping
                    let _ = ws.send(Message::Ping(Bytes::new())).await;
                    while let Some(msg) = ws.next().await {
                        match msg {
                            Ok(Message::Text(txt)) => {
                                if let Ok(ev) = serde_json::from_str::<ServerEvent>(&txt) {
                                    if let ServerEvent::RemainingUpdated {
                                        remaining_minutes, ..
                                    } = ev
                                    {
                                        if remaining_minutes > 0 {
                                            relocker.disable().await;
                                        }
                                    }
                                }
                            }
                            Ok(Message::Close(_)) => {
                                tracing::info!("WS: server closed connection");
                                break;
                            }
                            Ok(_) => {}
                            Err(e) => {
                                tracing::warn!(error=%e, "WS read error");
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error=%e, "WS: connect failed");
                }
            }
            // reconnect with backoff (cap at 30s)
            tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
            backoff_secs = std::cmp::min(backoff_secs * 2, 30);
        }
    });
}

fn to_ws_url(http_base: &str, token: &str) -> Result<url::Url, AppError> {
    let mut u = url::Url::parse(http_base)
        .map_err(|e| AppError::Config(format!("invalid server_url: {e}")))?;
    let scheme = u.scheme().to_string();
    let ws_scheme = match scheme.as_str() {
        "http" => "ws",
        "https" => "wss",
        other => {
            return Err(AppError::Config(format!(
                "unsupported scheme for WS: {other}"
            )));
        }
    };
    u.set_scheme(ws_scheme).ok();
    // Append path /api/ws
    let mut path = u.path().trim_end_matches('/').to_string();
    path.push_str("/api/ws");
    u.set_path(&path);
    // Add token query
    let mut qp = u.query_pairs_mut();
    qp.clear();
    qp.append_pair("token", token);
    drop(qp);
    Ok(u)
}
