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
pub use platform::linux::lock::{LockBackend, detect_lock_backend, enforce_lock_backend};

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
    let mut last_accounted_minute: Option<i64> = None;
    let fail_fuse_secs = 300u64; // 5 minutes

    // Countdown task controller (graceful cancel via signal)
    let countdown_task = CountdownTask::new(cfg.warn_before_lock_secs);

    loop {
        let start = std::time::Instant::now();
        match send_heartbeat(&cfg, &token, &mut last_accounted_minute).await {
            Ok(rem) => {
                info!(remaining = rem, "heartbeat ok");
                failures = 0;
                countdown_task.tick(rem as u64).await;
                if rem <= 0 {
                    warn!("minutes exhausted; enforcing screen lock");
                    if let Err(e) = enforce_lock_backend(&backend).await {
                        error!(error=%e, "failed to enforce lock");
                    }
                    sleep(Duration::from_secs(10)).await;
                }
            }
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

async fn send_heartbeat(
    cfg: &ClientConfig,
    token: &str,
    last_accounted_minute: &mut Option<i64>,
) -> Result<i32, AppError> {
    let base = crate::config::normalize_server_url(&cfg.server_url);
    let now_min: i64 = chrono::Utc::now().timestamp() / 60;
    // safety cap of 24h to avoid huge payloads after long outages
    let start_min = match *last_accounted_minute {
        Some(prev) => (prev + 1).max(now_min - 60 * 24),
        None => now_min,
    };
    let mut minutes = Vec::new();
    for m in start_min..=now_min {
        minutes.push(m);
    }
    let resp = api::rest::child_device_heartbeat_with_minutes(
        &base,
        &cfg.child_id,
        &cfg.device_id,
        token,
        &minutes,
    )
    .await
    .map_err(|e| AppError::Http(format!("heartbeat error: {e}")))?;
    *last_accounted_minute = Some(now_min);
    Ok(resp.remaining_minutes)
}

fn read_token_from_keyring(server_url: &str) -> Result<String, AppError> {
    let entry = keyring_entry(server_url)?;
    entry
        .get_password()
        .map_err(|e| AppError::Keyring(e.to_string()))
}

struct CountdownTask {
    // new Instant when we should display notification
    when_tx: tokio::sync::mpsc::Sender<tokio::time::Instant>,
    // how many seconds before lock we should notify
    notify_secs: u64,
}

impl CountdownTask {
    fn new(warn_before_lock_secs: u64) -> Self {
        let far_in_future = Instant::now() + Duration::from_secs(60 * 60 * 24 * 365 * 10); // 10 years

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
        }
    }

    async fn tick(&self, left_mins: u64) {
        if left_mins * 60 - self.notify_secs <= 0 {
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
}
