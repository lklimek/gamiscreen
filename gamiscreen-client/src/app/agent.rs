use std::collections::BTreeSet;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use directories::ProjectDirs;
use gamiscreen_shared::api::rest::RestError;
use gamiscreen_shared::api::{self};
use gamiscreen_shared::jwt::{self, JwtClaims};
use tokio::time::{Instant, Sleep, sleep};
use tracing::{debug, error, info, warn};

use crate::config::ClientConfig;
use crate::{AppError, platform, sse, update};

const RELOCK_POLL_INTERVAL: Duration = Duration::from_secs(5);
const RELOCK_INITIAL_DELAY_SECS: u64 = 60;
const RELOCK_DELAY_DECREMENT_PER_MINUTE: u64 = 10;
pub const HEARTBEAT_INTERVAL_SECS: u64 = 60;
pub const WARN_BEFORE_LOCK_SECS: u64 = 45;
pub const CAUTION_BEFORE_LOCK_SECS: u64 = 5 * 60;

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

    let countdown_task = CountdownTask::new(
        HEARTBEAT_INTERVAL_SECS,
        WARN_BEFORE_LOCK_SECS,
        CAUTION_BEFORE_LOCK_SECS,
        plat.clone(),
    );
    let pending_log_path = pending_minutes_path()?;
    tracing::trace!(path=?pending_log_path, "using pending minutes log path");
    let pending_minutes = PendingMinutes::load(pending_log_path)?;
    if pending_minutes.has_entries() {
        info!(
            restored_entries = pending_minutes.len(),
            "restored pending usage minutes from log"
        );
    }

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
                pending_minutes,
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
    pending_minutes: PendingMinutes,
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
        mut pending_minutes,
    } = ctx;
    let mut failures: u32 = 0;
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
        if let Err(e) = pending_minutes.insert(now_min) {
            warn!(error=%e, "failed to append minute to pending log");
        }

        match send_pending(
            &cfg.server_url,
            &claims.tenant_id,
            &child_id,
            &device_id,
            &token,
            &mut pending_minutes,
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
                    relocker.enable(Some(rem)).await;
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
                    relocker.enable(None).await;
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
    pending_minutes: &mut PendingMinutes,
) -> Result<Option<i32>, AppError> {
    if pending_minutes.is_empty() {
        return Ok(None);
    }
    let base = crate::config::normalize_server_url(server_url);
    let minutes = pending_minutes.snapshot();
    let resp = api::rest::child_device_heartbeat_with_minutes(
        &base, tenant_id, child_id, device_id, token, &minutes,
    )
    .await
    .map_err(|e| AppError::Http(format!("heartbeat error: {e}")))?;
    pending_minutes.mark_sent(&minutes)?;
    Ok(Some(resp.remaining_minutes))
}

fn read_token_from_keyring(server_url: &str) -> Result<String, AppError> {
    let entry = crate::keyring_entry(server_url)?;
    entry
        .get_password()
        .map_err(|e| AppError::Keyring(e.to_string()))
}

struct PendingMinutes {
    path: PathBuf,
    minutes: BTreeSet<i64>,
}

impl PendingMinutes {
    fn load(path: PathBuf) -> Result<Self, AppError> {
        let minutes = match std::fs::read_to_string(&path) {
            Ok(contents) => contents
                .lines()
                .filter_map(|line| {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        return None;
                    }
                    match trimmed.parse::<i64>() {
                        Ok(v) => Some(v),
                        Err(e) => {
                            warn!(error=%e, "pending minutes log contained invalid line");
                            None
                        }
                    }
                })
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
            Err(e) => return Err(AppError::Io(e)),
        };
        Ok(Self { path, minutes })
    }

    fn has_entries(&self) -> bool {
        !self.minutes.is_empty()
    }

    fn len(&self) -> usize {
        self.minutes.len()
    }

    fn is_empty(&self) -> bool {
        self.minutes.is_empty()
    }

    fn snapshot(&self) -> Vec<i64> {
        self.minutes.iter().copied().collect()
    }

    fn insert(&mut self, minute: i64) -> Result<(), AppError> {
        if self.minutes.insert(minute) {
            self.save()?;
        }
        Ok(())
    }

    fn mark_sent(&mut self, sent: &[i64]) -> Result<(), AppError> {
        if sent.is_empty() {
            return Ok(());
        }

        let mut changed = false;
        for minute in sent {
            if self.minutes.remove(minute) {
                changed = true;
            }
        }
        if changed {
            if let Err(e) = self.save() {
                // put minutes back so we don't lose data if save fails
                for minute in sent {
                    self.minutes.insert(*minute);
                }
                return Err(e);
            }
        }
        Ok(())
    }

    fn save(&self) -> Result<(), AppError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(AppError::Io)?;
        }
        let mut contents = String::new();
        for minute in &self.minutes {
            contents.push_str(&format!("{minute}\n"));
        }
        std::fs::write(&self.path, contents).map_err(AppError::Io)
    }
}

fn pending_minutes_path() -> Result<PathBuf, AppError> {
    let dirs = ProjectDirs::from("ws.klimek.gamiscreen", "gamiscreen", "gamiscreen")
        .ok_or_else(|| AppError::Config("could not determine data directory".into()))?;
    Ok(dirs.data_local_dir().join("pending-minutes.log"))
}

struct CountdownTask {
    cmd_tx: tokio::sync::mpsc::Sender<CountdownCommand>,
}

impl CountdownTask {
    fn new(
        interval_secs: u64,
        warn_before_lock_secs: u64,
        caution_before_lock_secs: u64,
        platform: Arc<dyn platform::Platform>,
    ) -> Self {
        let far_in_future = Instant::now() + Duration::from_secs(interval_secs * 1000);

        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let platform_for_task = platform.clone();
        tokio::spawn(async move {
            let mut schedule: Option<CountdownSchedule> = None;
            let mut timer: Pin<Box<Sleep>> = Box::pin(tokio::time::sleep_until(far_in_future));
            loop {
                tokio::select! {
                    new_when = rx.recv() => {
                        match new_when {
                            Some(CountdownCommand::Update { left_secs }) => {
                                let now = Instant::now();
                                let action = if let Some(ref mut sched) = schedule {
                                    sched.update(now, left_secs)
                                } else {
                                    let (sched, init_action) = CountdownSchedule::new(
                                        now,
                                        left_secs,
                                        warn_before_lock_secs,
                                        caution_before_lock_secs,
                                    );
                                    schedule = Some(sched);
                                    init_action
                                };

                                match action {
                                    Some(CountdownAction::Caution { display_secs }) => {
                                        platform
                                            .update_notification(display_secs as i64)
                                            .await;
                                    }
                                    Some(CountdownAction::Clear) => {
                                        platform.hide_notification().await;
                                    }
                                    None => {}
                                }

                                if let Some(ref sched) = schedule {
                                    if let Some(next_deadline) = sched.next_deadline(now) {
                                        timer.as_mut().reset(next_deadline);
                                    } else {
                                        timer.as_mut().reset(far_in_future);
                                    }
                                } else {
                                    timer.as_mut().reset(far_in_future);
                                }
                            }
                            Some(CountdownCommand::Cancel) => {
                                schedule = None;
                                platform.hide_notification().await;
                                timer.as_mut().reset(far_in_future);
                            }
                            None => {
                                info!("countdown task: channel closed; exiting");
                                platform.hide_notification().await;
                                break;
                            }
                        }
                    }

                    _ = &mut timer => {
                        if let Some(sched) = schedule.as_mut() {
                            let now = Instant::now();
                            match sched.fire(&platform_for_task, now).await {
                                Some(next_deadline) => {
                                    timer.as_mut().reset(next_deadline);
                                }
                                None => {
                                    timer.as_mut().reset(far_in_future);
                                    schedule = None;
                                }
                            }
                        } else {
                            timer.as_mut().reset(far_in_future);
                        }
                    }
                }
            }
        });

        Self { cmd_tx: tx }
    }

    async fn tick(&self, left_mins: u64) {
        let left_secs = left_mins * 60;
        if let Err(e) = self
            .cmd_tx
            .send(CountdownCommand::Update { left_secs })
            .await
        {
            tracing::warn!(error=%e, "countdown task: failed to send new deadline");
        }
    }

    async fn cancel(&self) {
        if let Err(e) = self.cmd_tx.send(CountdownCommand::Cancel).await {
            tracing::warn!(error=%e, "countdown task: failed to cancel deadline");
        }
    }
}

enum CountdownCommand {
    Update { left_secs: u64 },
    Cancel,
}

struct CountdownSchedule {
    final_at: Instant,
    warn_at: Instant,
    warn_secs: u64,
    caution_secs: u64,
    caution_notified: bool,
    warn_notified: bool,
}

impl CountdownSchedule {
    fn new(
        now: Instant,
        left_secs: u64,
        warn_secs: u64,
        caution_secs: u64,
    ) -> (Self, Option<CountdownAction>) {
        let mut sched = Self {
            final_at: now,
            warn_at: now,
            warn_secs,
            caution_secs,
            caution_notified: false,
            warn_notified: false,
        };
        let action = sched.update(now, left_secs);
        (sched, action)
    }

    fn update(&mut self, now: Instant, left_secs: u64) -> Option<CountdownAction> {
        self.final_at = now + Duration::from_secs(left_secs);
        self.warn_at = if left_secs > self.warn_secs {
            self.final_at - Duration::from_secs(self.warn_secs)
        } else {
            now
        };

        let mut action = None;

        if left_secs > self.caution_secs {
            if self.caution_notified {
                self.caution_notified = false;
                action = Some(CountdownAction::Clear);
            } else {
                self.caution_notified = false;
            }
        }

        if left_secs > self.warn_secs {
            self.warn_notified = false;
        }

        if left_secs <= self.caution_secs && !self.caution_notified {
            self.caution_notified = true;
            action = Some(CountdownAction::Caution {
                display_secs: left_secs,
            });
        }

        action
    }

    fn next_deadline(&self, now: Instant) -> Option<Instant> {
        if !self.warn_notified {
            Some(self.warn_at.max(now))
        } else {
            None
        }
    }

    async fn fire(
        &mut self,
        platform: &Arc<dyn platform::Platform>,
        now: Instant,
    ) -> Option<Instant> {
        if !self.warn_notified && self.warn_at <= now {
            self.warn_notified = true;
            platform.notify(self.warn_secs).await;
        }
        self.next_deadline(now)
    }
}

enum CountdownAction {
    Caution { display_secs: u64 },
    Clear,
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

    async fn enable(&self, remaining_minutes: Option<i32>) {
        let remaining_secs = remaining_minutes.map(|rem| rem as i64 * 60);

        let mut h = self.handle.lock().await;
        if h.is_some() {
            drop(h);
            if let Some(secs) = remaining_secs {
                self.platform.update_notification(secs).await;
            }
            return;
        }

        let platform = self.platform.clone();
        let remaining_secs_for_task = remaining_secs;
        let handle = tokio::spawn(async move {
            let initial_lock_at = Instant::now();
            if let Err(e) = platform.lock().await {
                tracing::error!(error=%e, "initial re-lock attempt failed");
            }
            let mut negative_notified = false;
            loop {
                match platform.is_session_locked().await {
                    Ok(false) => {
                        if let Some(secs) = remaining_secs_for_task {
                            if secs <= 0 && !negative_notified {
                                platform.update_notification(secs).await;
                                negative_notified = true;
                            }
                        }
                        let wait = Self::relock_delay(initial_lock_at);
                        if !wait.is_zero() {
                            tokio::time::sleep(wait).await;
                        }
                        if let Err(e) = platform.lock().await {
                            tracing::error!(error=%e, "re-lock attempt failed");
                        }
                    }
                    Ok(true) => {
                        negative_notified = false;
                    }
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
        let handle = {
            let mut h = self.handle.lock().await;
            h.take()
        };
        if let Some(handle) = handle {
            handle.abort();
        }
        self.platform.hide_notification().await;
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
                            relocker.enable(Some(remaining_minutes)).await;
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
