use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::*;

/// Platform whose session is always locked; counts notification updates.
#[derive(Default)]
struct LockedPlatform {
    updates: AtomicUsize,
}

#[async_trait]
impl platform::Platform for LockedPlatform {
    async fn lock(&self) -> Result<(), AppError> {
        Ok(())
    }
    async fn is_session_locked(&self) -> Result<bool, AppError> {
        Ok(true)
    }
    async fn notify(&self, _: u64) {}
    async fn update_notification(&self, _: i64) {
        self.updates.fetch_add(1, Ordering::SeqCst);
    }
    async fn hide_notification(&self) {}
    fn device_id(&self) -> String {
        "device".into()
    }
    async fn install(&self, _: Option<String>) -> Result<(), AppError> {
        Ok(())
    }
    async fn uninstall(&self, _: Option<String>) -> Result<(), AppError> {
        Ok(())
    }
    fn replace_and_restart(&self, _: &std::path::Path, _: &std::path::Path, _: &[String]) -> ! {
        panic!("updates are not used by this test")
    }
}

const ALLOWED: &str =
    r#"{"child_id":"alice","remaining_minutes":30,"balance":0,"blocked_by_tasks":false}"#;
const BLOCKED: &str =
    r#"{"child_id":"alice","remaining_minutes":0,"balance":0,"blocked_by_tasks":true}"#;

/// Runs `main_loop` for a locked child session against `server_url`.
fn spawn_agent(
    cancel: &tokio_util::sync::CancellationToken,
    server_url: String,
    relocker: &ReLocker,
    platform: Arc<dyn platform::Platform>,
    pending_minutes: PendingMinutes,
) -> tokio::task::JoinHandle<()> {
    let ctx = MainLoopContext {
        cfg: ClientConfig { server_url },
        token: "test-token".into(),
        claims: JwtClaims {
            sub: "alice".into(),
            jti: "test-session".into(),
            exp: i64::MAX / 2,
            role: gamiscreen_shared::auth::Role::Child,
            child_id: Some("alice".into()),
            device_id: Some("device".into()),
            tenant_id: "family".into(),
        },
        child_id: "alice".into(),
        device_id: "device".into(),
        relocker: relocker.clone(),
        countdown_task: CountdownTask::new(60, 45, 300, platform.clone()),
        platform,
        pending_minutes,
    };
    let cancel = cancel.clone();
    tokio::spawn(async move { main_loop(cancel, ctx).await.unwrap() })
}

async fn stop(cancel: tokio_util::sync::CancellationToken, agent: tokio::task::JoinHandle<()>) {
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), agent)
        .await
        .unwrap()
        .unwrap();
}

/// Accepts the next client request and returns its stream and request head.
async fn next_request(listener: &TcpListener, wait: Duration) -> (TcpStream, String) {
    let (mut stream, _) = tokio::time::timeout(wait, listener.accept())
        .await
        .expect("a locked client must still request current status")
        .unwrap();
    let mut request = Vec::new();
    let mut chunk = [0u8; 1024];
    while !request.windows(4).any(|part| part == b"\r\n\r\n") {
        let n = stream.read(&mut chunk).await.unwrap();
        assert_ne!(n, 0);
        request.extend_from_slice(&chunk[..n]);
    }
    (stream, String::from_utf8(request).unwrap())
}

async fn respond(mut stream: TcpStream, status: &str, body: &str) {
    stream.write_all(format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()).as_bytes()).await.unwrap();
}

async fn wait_for_relocker(relocker: &ReLocker, active: bool) {
    tokio::time::timeout(Duration::from_secs(1), async {
        while relocker.handle.lock().await.is_some() != active {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("re-lock loop must become active={active}"));
}

fn empty_pending(dir: &tempfile::TempDir) -> PendingMinutes {
    PendingMinutes::load(dir.path().join("pending-minutes.log")).unwrap()
}

#[tokio::test]
async fn locked_client_refreshes_status_without_charging_usage_and_stops_relocking() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("pending-minutes.log");
    std::fs::write(&log_path, "100\n101\n").unwrap();
    let pending_minutes = PendingMinutes::load(log_path.clone()).unwrap();
    let platform: Arc<dyn platform::Platform> = Arc::new(LockedPlatform::default());
    let relocker = ReLocker::new(platform.clone());
    relocker.enable(Some(0)).await;
    let cancel = tokio_util::sync::CancellationToken::new();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let agent = spawn_agent(&cancel, url, &relocker, platform, pending_minutes);
    let (stream, request) = next_request(&listener, Duration::from_secs(1)).await;
    assert!(request.starts_with("GET /api/v1/family/family/children/alice/remaining "));
    respond(stream, "200 OK", ALLOWED).await;
    wait_for_relocker(&relocker, false).await;
    stop(cancel, agent).await;
    assert!(relocker.handle.lock().await.is_none());
    assert_eq!(std::fs::read_to_string(log_path).unwrap(), "100\n101\n");
}

/// A session that is already locked by an active re-lock loop must not raise a
/// "time is up" notification (toast/sound) on every locked status poll.
#[tokio::test]
async fn locked_client_does_not_renotify_while_still_blocked() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let counting = Arc::new(LockedPlatform::default());
    let platform: Arc<dyn platform::Platform> = counting.clone();
    let relocker = ReLocker::new(platform.clone());
    // Minutes ran out while unlocked: the re-lock loop is active and the session is locked.
    relocker.enable(Some(0)).await;
    let cancel = tokio_util::sync::CancellationToken::new();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let agent = spawn_agent(&cancel, url, &relocker, platform, empty_pending(&dir));
    let (stream, _) = next_request(&listener, Duration::from_secs(1)).await;
    respond(stream, "200 OK", BLOCKED).await;
    let _ = tokio::time::timeout(Duration::from_secs(1), async {
        while counting.updates.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(
        relocker.handle.lock().await.is_some(),
        "a blocked status must keep the re-lock loop active"
    );
    stop(cancel, agent).await;
    assert_eq!(
        counting.updates.load(Ordering::SeqCst),
        0,
        "locked status poll must not re-show the lock notification"
    );
}

#[tokio::test]
async fn locked_client_starts_relocking_when_blocked_without_active_loop() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let platform: Arc<dyn platform::Platform> = Arc::new(LockedPlatform::default());
    let relocker = ReLocker::new(platform.clone());
    let cancel = tokio_util::sync::CancellationToken::new();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let agent = spawn_agent(&cancel, url, &relocker, platform, empty_pending(&dir));
    let (stream, _) = next_request(&listener, Duration::from_secs(1)).await;
    respond(stream, "200 OK", BLOCKED).await;
    wait_for_relocker(&relocker, true).await;
    stop(cancel, agent).await;
    relocker.shutdown().await;
}

#[tokio::test]
async fn locked_client_polls_server_url_without_scheme() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let platform: Arc<dyn platform::Platform> = Arc::new(LockedPlatform::default());
    let relocker = ReLocker::new(platform.clone());
    relocker.enable(Some(0)).await;
    let cancel = tokio_util::sync::CancellationToken::new();
    let url = listener.local_addr().unwrap().to_string();
    let agent = spawn_agent(&cancel, url, &relocker, platform, empty_pending(&dir));
    let (stream, _) = next_request(&listener, Duration::from_secs(1)).await;
    respond(stream, "200 OK", ALLOWED).await;
    wait_for_relocker(&relocker, false).await;
    stop(cancel, agent).await;
}

/// The offline failsafe protects unlocked sessions; failed polls while the session is
/// already locked must not arm it, or the first unlock after an outage is re-locked.
#[tokio::test(start_paused = true)]
async fn locked_status_failures_do_not_trip_offline_failsafe() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let platform: Arc<dyn platform::Platform> = Arc::new(LockedPlatform::default());
    let relocker = ReLocker::new(platform.clone());
    let cancel = tokio_util::sync::CancellationToken::new();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let agent = spawn_agent(&cancel, url, &relocker, platform, empty_pending(&dir));
    let wait = Duration::from_secs(2 * HEARTBEAT_INTERVAL_SECS);
    for _ in 0..5 {
        let (stream, _) = next_request(&listener, wait).await;
        respond(stream, "500 Internal Server Error", "{}").await;
    }
    // The sixth poll starts only after the fifth failure has been handled.
    let (stream, _) = next_request(&listener, wait).await;
    let armed = relocker.handle.lock().await.is_some();
    respond(stream, "500 Internal Server Error", "{}").await;
    stop(cancel, agent).await;
    relocker.shutdown().await;
    assert!(
        !armed,
        "failed polls while locked must not enable the offline re-lock failsafe"
    );
}
