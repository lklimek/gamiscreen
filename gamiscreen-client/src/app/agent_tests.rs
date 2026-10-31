use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

struct LockedPlatform;

#[async_trait]
impl platform::Platform for LockedPlatform {
    async fn lock(&self) -> Result<(), AppError> {
        Ok(())
    }
    async fn is_session_locked(&self) -> Result<bool, AppError> {
        Ok(true)
    }
    async fn notify(&self, _: u64) {}
    async fn update_notification(&self, _: i64) {}
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

#[tokio::test]
async fn locked_client_refreshes_status_without_charging_usage_and_stops_relocking() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cfg = ClientConfig {
        server_url: format!("http://{}", listener.local_addr().unwrap()),
    };
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("pending-minutes.log");
    std::fs::write(&log_path, "100\n101\n").unwrap();
    let pending_minutes = PendingMinutes::load(log_path.clone()).unwrap();
    let platform: Arc<dyn platform::Platform> = Arc::new(LockedPlatform);
    let relocker = ReLocker::new(platform.clone());
    relocker.enable(Some(0)).await;
    let cancel = tokio_util::sync::CancellationToken::new();
    let agent = tokio::spawn(main_loop(
        cancel.clone(),
        MainLoopContext {
            cfg,
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
        },
    ));
    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(1), listener.accept())
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
    assert!(
        String::from_utf8(request)
            .unwrap()
            .starts_with("GET /api/v1/family/family/children/alice/remaining ")
    );
    let body =
        r#"{"child_id":"alice","remaining_minutes":30,"balance":0,"blocked_by_tasks":false}"#;
    stream.write_all(format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(), body).as_bytes()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while relocker.handle.lock().await.is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("allowed status must stop the active re-lock loop");
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), agent)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(relocker.handle.lock().await.is_none());
    assert_eq!(std::fs::read_to_string(log_path).unwrap(), "100\n101\n");
}
