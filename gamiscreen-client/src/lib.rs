use std::collections::BTreeSet;
use std::time::Duration;

use gamiscreen_server::shared::api::{self};
use tokio::time::{Instant, sleep};
use tracing::{debug, error, info, warn};

pub mod cli;
pub mod config;
pub mod login;
pub mod notify;
pub mod platform;

pub use cli::{Cli, Command};
pub use config::{ClientConfig, load_config, resolve_config_path};
use notify::default_backend;
pub use platform::linux::lock::{
    LockBackend, detect_lock_backend, enforce_lock_backend, is_session_locked,
};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("config error: {0}")]
    Config(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("dbus error: {0}")]
    Dbus(String),
    #[error("keyring error: {0}")]
    Keyring(String),
}

// API types come from gamiscreen-server::shared::api

fn init_tracing() {
    let env_filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .compact()
        .init();
}

fn keyring_entry(server_url: &str) -> Result<keyring::Entry, AppError> {
    let service = "gamiscreen-client";
    keyring::Entry::new(service, &crate::config::normalize_server_url(server_url))
        .map_err(|e| AppError::Keyring(e.to_string()))
}

pub async fn run(cli: Cli) -> Result<(), AppError> {
    init_tracing();

    if let Some(cmd) = &cli.command {
        match cmd {
            Command::Login { server, username } => {
                return login::login(server.clone(), username.clone(), cli.config.clone()).await;
            }
            Command::Install { user } => {
                return platform::linux::install::install_all(user.clone()).await;
            }
            Command::Uninstall { user } => {
                return platform::linux::install::uninstall_all(user.clone()).await;
            }
        }
    }

    let cfg_path = resolve_config_path(cli.config)?;
    let cfg = load_config(&cfg_path)?;
    info!(path=?cfg_path, "loaded config");

    let backend = detect_lock_backend(&cfg).await?;
    info!(?backend, "lock backend selected");

    // Load token from keyring using normalized server_url as the account key
    let key = crate::config::normalize_server_url(&cfg.server_url);
    let token = read_token_from_keyring(&key)?;

    let mut failures: u32 = 0;
    let mut unsent_minutes: BTreeSet<i64> = BTreeSet::new();
    let fail_fuse_secs = 300u64; // 5 minutes

    // Countdown task controller (graceful cancel via signal)
    let countdown_task = CountdownTask::new(cfg.interval_secs, cfg.warn_before_lock_secs);

    loop {
        let start = std::time::Instant::now();
        // If the session is locked, skip accounting and heartbeats for this loop.
        let session_locked = is_session_locked().await;
        tracing::debug!(?session_locked, "session lock status checked");
        if let Ok(true) = &session_locked {
            // Cancel any pending countdown notification
            countdown_task.cancel().await;
            info!("session locked; skipping heartbeat and accounting for this interval");
            let elapsed = start.elapsed();
            let interval = Duration::from_secs(cfg.interval_secs);
            if elapsed < interval {
                sleep(interval - elapsed).await;
            }
            continue;
        }
        // Enqueue minutes since last seen (inclusive of current minute)
        let now_min: i64 = chrono::Utc::now().timestamp() / 60;
        unsent_minutes.insert(now_min);

        match send_pending(&cfg, &token, &mut unsent_minutes).await {
            Ok(Some(rem)) => {
                info!(remaining = rem, "heartbeat ok");
                failures = 0;
                if rem >= 1 {
                    countdown_task.tick(rem as u64).await;
                } else {
                    // ensure any pending notification is closed if we are at/past zero
                    countdown_task.cancel().await;
                }
                if rem <= 0 {
                    warn!("minutes exhausted; enforcing screen lock");
                    if let Err(e) = enforce_lock_backend(&backend).await {
                        error!(error=%e, "failed to enforce lock");
                    }
                }
            }
            Ok(None) => { /* nothing to send */ }
            Err(e) => {
                failures = failures.saturating_add(1);
                error!(error=%e, failures=failures, "heartbeat failed");
                let elapsed_fail_secs = cfg.interval_secs.saturating_mul(failures as u64);
                if elapsed_fail_secs >= fail_fuse_secs {
                    warn!(
                        "server unreachable threshold exceeded; enforcing screen lock as failsafe"
                    );
                    if let Err(e2) = enforce_lock_backend(&backend).await {
                        error!(error=%e2, "failed to enforce lock");
                    }
                    failures = 0;
                }
            }
        }

        let elapsed = start.elapsed();
        let interval = Duration::from_secs(cfg.interval_secs);
        if elapsed < interval {
            sleep(interval - elapsed).await;
        }
    }
}

async fn send_pending(
    cfg: &ClientConfig,
    token: &str,
    unsent_minutes: &mut BTreeSet<i64>,
) -> Result<Option<i32>, AppError> {
    if unsent_minutes.is_empty() {
        return Ok(None);
    }
    let base = crate::config::normalize_server_url(&cfg.server_url);
    let minutes: Vec<i64> = unsent_minutes.iter().copied().collect();
    let resp = api::rest::child_device_heartbeat_with_minutes(
        &base,
        &cfg.child_id,
        &cfg.device_id,
        token,
        &minutes,
    )
    .await
    .map_err(|e| AppError::Http(format!("heartbeat error: {e}")))?;
    unsent_minutes.clear();
    Ok(Some(resp.remaining_minutes))
}

fn read_token_from_keyring(server_url: &str) -> Result<String, AppError> {
    let entry = keyring_entry(server_url)?;
    entry
        .get_password()
        .map_err(|e| AppError::Keyring(e.to_string()))
}

struct CountdownTask {
    /// new Instant when we should display notification
    when_tx: tokio::sync::mpsc::Sender<tokio::time::Instant>,
    /// how many seconds before lock we should notify
    notify_secs: u64,
    /// heartbeat interval in seconds
    interval_secs: u64,
}

impl CountdownTask {
    fn new(interval_secs: u64, warn_before_lock_secs: u64) -> Self {
        let far_in_future = Instant::now() + Duration::from_secs(interval_secs * 1000);

        let (tx, mut rx) = tokio::sync::mpsc::channel(5);
        tokio::spawn(async move {
            let mut notifier = default_backend();

            let mut when_notify = far_in_future;
            // wait until deadline and show countdown; countdown will stop when new deadline arrives
            loop {
                tokio::select! {
                    new_when = rx.recv() => {
                        let Some(when) = new_when else {
                            info!("countdown task: channel closed; exiting");
                            break;
                        };
                        when_notify = when;

                        // we got new msg - it means previous notification is obsolete
                        debug!(?when_notify, "countdown task: new deadline received; closing previous notification");
                        notifier.close().await;
                        // continue to next loop iteration to wait for next deadline or timeout
                    }

                    _= tokio::time::sleep_until(when_notify) => {
                        // countdown finished, we notify
                        debug!("countdown task: deadline reached; showing countdown notification");
                        when_notify = far_in_future; // reset to far future to avoid repeated notifications
                        notifier.show_countdown(warn_before_lock_secs).await;
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
            // Most likely we are already in countdown or past it; no need to restart
            return;
        }

        let when =
            tokio::time::Instant::now() + Duration::from_secs(left_mins * 60 - self.notify_secs);
        self.when_tx
            .send(when)
            .await
            .expect("countdown task receiver dropped; this should not happen");
    }

    async fn cancel(&self) {
        // Send far-future deadline to ensure any visible notification is closed
        let when = Instant::now() + Duration::from_secs(self.interval_secs * 1000);
        let _ = self.when_tx.send(when).await;
    }
}
