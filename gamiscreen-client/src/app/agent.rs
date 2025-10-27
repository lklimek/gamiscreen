use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gamiscreen_shared::api::{self, rest::RestError};
use gamiscreen_shared::jwt::{self, JwtClaims};
use tokio::time::{Instant, sleep};
use tracing::{debug, error, info, warn};

use crate::config::ClientConfig;
use crate::{AppError, platform, sse, update};

const RELOCK_POLL_INTERVAL: Duration = Duration::from_secs(5);
const RELOCK_INITIAL_DELAY_SECS: u64 = 60;
const RELOCK_DELAY_DECREMENT_PER_MINUTE: u64 = 10;
pub const HEARTBEAT_INTERVAL_SECS: u64 = 60;
pub const WARN_BEFORE_LOCK_SECS: u64 = 45;

/// Entry point for the interactive agent in the current session.
pub async fn run(config_path: Option<PathBuf>) -> Result<(), AppError> {
    let (cfg_path, cfg) = ClientConfig::find_and_load(config_path)?;
    info!(path=?cfg_path, "loaded config");

    if let Err(e) = update::maybe_self_update(&cfg).await {
        warn!(error=%e, "auto-update failed; continuing with current binary");
    }

    let plat = platform::detect(&cfg).await?;
    #[cfg(target_os = "windows")]
    info!("platform selected: windows");
    #[cfg(not(target_os = "windows"))]
    info!("platform selected: linux");

    let relocker = ReLocker::new(plat.clone());

    let key = crate::config::normalize_server_url(&cfg.server_url);
    let mut token = read_token_from_keyring(&key)?;
    let mut claims = jwt::decode_unverified(&token)
        .map_err(|e| AppError::Http(format!("invalid token: {e}")))?;

    match api::rest::renew_token(&cfg.server_url, &token).await {
        Ok(resp) => {
            let new_token = resp.token;
            let new_claims = jwt::decode_unverified(&new_token)
                .map_err(|e| AppError::Http(format!("invalid renewed token: {e}")))?;
            let entry = crate::keyring_entry(&cfg.server_url)?;
            entry
                .set_password(&new_token)
                .map_err(|e| AppError::Keyring(e.to_string()))?;
            info!("renewed auth token from server");
            token = new_token;
            claims = new_claims;
        }
        Err(RestError::Status { status: 401, .. }) => {
            return Err(AppError::Http(
                "token renewal failed with unauthorized; please log in again".into(),
            ));
        }
        Err(e) => {
            warn!(error=%e, "token renewal failed; continuing with existing token");
        }
    }

    let child_id = claims
        .child_id
        .clone()
        .ok_or_else(|| AppError::Config("device token missing child_id".into()))?;
    let device_id = claims
        .device_id
        .clone()
        .ok_or_else(|| AppError::Config("device token missing device_id".into()))?;

    let hub = match sse::SseHub::new(&cfg.server_url, &token, &claims) {
        Ok(h) => Some(h),
        Err(e) => {
            warn!(error=%e, "SSE hub init failed; continuing without SSE");
            None
        }
    };
    if let Some(h) = &hub {
        relocker.attach_sse(h).await;
    }

    let countdown_task =
        CountdownTask::new(HEARTBEAT_INTERVAL_SECS, WARN_BEFORE_LOCK_SECS, plat.clone());

    let cancel = tokio_util::sync::CancellationToken::new();
    let cfg_cloned = cfg.clone();
    let token_cloned = token.clone();
    let relocker_cloned = relocker.clone();
    let plat_cloned = plat.clone();
    let cancel_child = cancel.child_token();
    let claims_cloned = claims.clone();
    let child_id_cloned = child_id.clone();
    let device_id_cloned = device_id.clone();
    let mut handle = tokio::spawn(async move {
        let _ = main_loop(
            cancel_child,
            MainLoopContext {
                cfg: cfg_cloned,
                token: token_cloned,
                claims: claims_cloned,
                child_id: child_id_cloned,
                device_id: device_id_cloned,
                relocker: relocker_cloned,
                platform: plat_cloned,
                countdown_task,
            },
        )
        .await;
    });

    tokio::select! {
        _ = shutdown_signal() => {
            info!("shutdown signal received; requesting main loop to stop");
            cancel.cancel();
        }
        _ = &mut handle => {
            info!("main loop finished");
        }
    }

    if !handle.is_finished() {
        let _ = tokio::time::timeout(Duration::from_secs(3), handle).await;
    }
    relocker.shutdown().await;
    Ok(())
}

struct MainLoopContext {
    cfg: ClientConfig,
    token: String,
    claims: JwtClaims,
    child_id: String,
    device_id: String,
    relocker: ReLocker,
    platform: Arc<dyn platform::Platform>,
    countdown_task: CountdownTask,
}

async fn main_loop(
    cancel: tokio_util::sync::CancellationToken,
    ctx: MainLoopContext,
) -> Result<(), AppError> {
    let MainLoopContext {
        cfg,
        token,
        claims,
        child_id,
        device_id,
        relocker,
        platform,
        countdown_task,
    } = ctx;
    let mut failures: u32 = 0;
    let mut unsent_minutes: BTreeSet<i64> = BTreeSet::new();
    let fail_fuse_secs = HEARTBEAT_INTERVAL_SECS * 5;
    let interval = Duration::from_secs(HEARTBEAT_INTERVAL_SECS);

    loop {
        if cancel.is_cancelled() {
            break;
        }

        let start = std::time::Instant::now();
        let session_locked = platform.is_session_locked().await;
        debug!(?session_locked, "session lock status checked");
        if let Ok(true) = &session_locked {
            countdown_task.cancel().await;
            info!("session locked; skipping heartbeat and accounting for this interval");
            let elapsed = start.elapsed();
            if elapsed < interval {
                tokio::select! {
                    _ = cancel.cancelled() => { break; }
                    _ = sleep(interval - elapsed) => {}
                }
            }
            continue;
        }

        let now_min: i64 = chrono::Utc::now().timestamp() / 60;
        unsent_minutes.insert(now_min);

        match send_pending(
            &cfg.server_url,
            &claims.tenant_id,
            &child_id,
            &device_id,
            &token,
            &mut unsent_minutes,
        )
        .await
        {
            Ok(Some(rem)) => {
                info!(remaining = rem, "heartbeat ok");
                failures = 0;
                if rem >= 1 {
                    countdown_task.tick(rem as u64).await;
                    relocker.disable().await;
                } else {
                    countdown_task.cancel().await;
                }
                if rem <= 0 {
                    warn!("minutes exhausted; enabling re-lock loop");
                    relocker.enable().await;
                }
            }
            Ok(None) => {}
            Err(e) => {
                failures = failures.saturating_add(1);
                error!(error=%e, failures=failures, "heartbeat failed");
                let elapsed_fail_secs = HEARTBEAT_INTERVAL_SECS.saturating_mul(failures as u64);
                if elapsed_fail_secs >= fail_fuse_secs {
                    warn!(
                        "server unreachable threshold exceeded; enabling re-lock loop as failsafe"
                    );
                    relocker.enable().await;
                    failures = 0;
                }
            }
        }

        let elapsed = start.elapsed();
        if elapsed < interval {
            tokio::select! {
                _ = cancel.cancelled() => { break; }
                _ = sleep(interval - elapsed) => {}
            }
        }
    }

    countdown_task.cancel().await;
    relocker.shutdown().await;
    Ok(())
}

async fn send_pending(
    server_url: &str,
    tenant_id: &str,
    child_id: &str,
    device_id: &str,
    token: &str,
    unsent_minutes: &mut BTreeSet<i64>,
) -> Result<Option<i32>, AppError> {
    if unsent_minutes.is_empty() {
        return Ok(None);
    }
    let base = crate::config::normalize_server_url(server_url);
    let minutes: Vec<i64> = unsent_minutes.iter().copied().collect();
    let resp = api::rest::child_device_heartbeat_with_minutes(
        &base, tenant_id, child_id, device_id, token, &minutes,
    )
    .await
    .map_err(|e| AppError::Http(format!("heartbeat error: {e}")))?;
    unsent_minutes.clear();
    Ok(Some(resp.remaining_minutes))
}

fn read_token_from_keyring(server_url: &str) -> Result<String, AppError> {
    let entry = crate::keyring_entry(server_url)?;
    entry
        .get_password()
        .map_err(|e| AppError::Keyring(e.to_string()))
}

struct CountdownTask {
    when_tx: tokio::sync::mpsc::Sender<tokio::time::Instant>,
    notify_secs: u64,
    interval_secs: u64,
}

impl CountdownTask {
    fn new(
        interval_secs: u64,
        warn_before_lock_secs: u64,
        platform: Arc<dyn platform::Platform>,
    ) -> Self {
        let far_in_future = Instant::now() + Duration::from_secs(interval_secs * 1000);

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        tokio::spawn(async move {
            let mut when_notify = far_in_future;
            loop {
                tokio::select! {
                    new_when = rx.recv() => {
                        let Some(when) = new_when else {
                            info!("countdown task: channel closed; exiting");
                            break;
                        };
                        when_notify = when;

                        debug!(?when_notify, "countdown task: new deadline received; closing previous notification");
                        platform.hide_notification().await;
                    }

                    _ = tokio::time::sleep_until(when_notify) => {
                        debug!("countdown task: deadline reached; showing countdown notification");
                        when_notify = far_in_future;
                        platform.notify(warn_before_lock_secs).await;
                    }
                }
            }
        });

        Self {
            when_tx: tx,
            notify_secs: warn_before_lock_secs,
            interval_secs,
        }
    }

    async fn tick(&self, left_mins: u64) {
        if left_mins * 60 <= self.notify_secs {
            return;
        }

        let when =
            tokio::time::Instant::now() + Duration::from_secs(left_mins * 60 - self.notify_secs);
        if let Err(e) = self.when_tx.send(when).await {
            tracing::warn!(error=%e, "countdown task: failed to send new deadline");
        }
    }

    async fn cancel(&self) {
        let when = Instant::now() + Duration::from_secs(self.interval_secs * 1000);
        let _ = self.when_tx.send(when).await;
    }
}

#[derive(Clone)]
struct ReLocker {
    platform: Arc<dyn platform::Platform>,
    handle: std::sync::Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    sse_task: std::sync::Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl ReLocker {
    fn new(platform: Arc<dyn platform::Platform>) -> Self {
        Self {
            platform,
            handle: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            sse_task: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    async fn enable(&self) {
        let mut h = self.handle.lock().await;
        if h.is_some() {
            return;
        }
        let platform = self.platform.clone();
        let handle = tokio::spawn(async move {
            let initial_lock_at = Instant::now();
            if let Err(e) = platform.lock().await {
                tracing::error!(error=%e, "initial re-lock attempt failed");
            }
            loop {
                match platform.is_session_locked().await {
                    Ok(false) => {
                        let wait = Self::relock_delay(initial_lock_at);
                        if !wait.is_zero() {
                            tokio::time::sleep(wait).await;
                        }
                        if let Err(e) = platform.lock().await {
                            tracing::error!(error=%e, "re-lock attempt failed");
                        }
                    }
                    Ok(true) => {}
                    Err(e) => {
                        tracing::warn!(error=%e, "re-lock: failed to query lock state");
                    }
                }
                tokio::time::sleep(RELOCK_POLL_INTERVAL).await;
            }
        });
        *h = Some(handle);
    }

    async fn disable(&self) {
        let mut h = self.handle.lock().await;
        if let Some(handle) = h.take() {
            handle.abort();
        }
    }

    async fn attach_sse(&self, hub: &sse::SseHub) {
        let mut guard = self.sse_task.lock().await;
        if guard.is_some() {
            return;
        }
        let mut rx = hub.subscribe();
        let relocker = self.clone();
        let handle = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(gamiscreen_shared::api::ServerEvent::RemainingUpdated {
                        remaining_minutes,
                        ..
                    }) => {
                        if remaining_minutes > 0 {
                            relocker.disable().await;
                        } else {
                            relocker.enable().await;
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(missed=%n, "SSE relocker subscriber lagged; resyncing");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            tracing::warn!(
                "SSE relocker subscriber exiting; no longer responding to server events"
            );
        });
        *guard = Some(handle);
    }

    async fn shutdown(&self) {
        self.disable().await;
        let mut s = self.sse_task.lock().await;
        if let Some(h) = s.take() {
            h.abort();
        }
    }

    fn relock_delay(initial_lock_at: Instant) -> Duration {
        let elapsed_secs = initial_lock_at.elapsed().as_secs();
        let elapsed_minutes = elapsed_secs / 60;
        let decrement = elapsed_minutes.saturating_mul(RELOCK_DELAY_DECREMENT_PER_MINUTE);
        let remaining = RELOCK_INITIAL_DELAY_SECS.saturating_sub(decrement);
        Duration::from_secs(remaining)
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sigint = signal(SignalKind::interrupt()).expect("listen SIGINT");
        let mut sigterm = signal(SignalKind::terminate()).expect("listen SIGTERM");
        tokio::select! {
            _ = sigint.recv() => {
                info!("shutdown: received SIGINT");
            }
            _ = sigterm.recv() => {
                info!("shutdown: received SIGTERM");
            }
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.expect("listen for ctrl_c");
        info!("shutdown: received ctrl_c");
    }
}
