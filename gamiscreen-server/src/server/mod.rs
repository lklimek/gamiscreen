mod acl;
pub mod auth;
mod config;
mod push;

use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, header};
use axum::response::Response as AxumResponse;
use axum::response::sse::{Event, Sse};
use axum::routing::{get, post};
use axum::{Json, Router, middleware};
use bcrypt::verify;
pub use config::{AppConfig, Role, UserConfig};
use gamiscreen_shared::api::{ChildDto, ConfigResp};
use gamiscreen_shared::{api, jwt};
use mime_guess::from_path;
use push::PushService;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, MutexGuard, broadcast};
use tokio_util::sync::CancellationToken;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{Span, info_span};
use uuid::Uuid;

use crate::server::auth::AuthCtx;

const MAX_PUSH_SUBSCRIPTIONS_PER_CHILD: i64 = 10;

type ChildCacheMap =
    std::sync::Arc<Mutex<std::collections::HashMap<String, std::sync::Arc<Mutex<Option<i32>>>>>>;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub store: crate::storage::Store,
    // Cache of remaining minutes per child. None => needs recompute
    children_cache: ChildCacheMap,
    // Broadcast notifications to connected websocket clients
    notif_tx: broadcast::Sender<ServerEvent>,
    // Global shutdown token to allow canceling long-lived streams (e.g., SSE)
    pub shutdown: CancellationToken,
    push: Option<PushService>,
}

impl AppState {
    pub fn new(config: AppConfig, store: crate::storage::Store) -> Self {
        let (notif_tx, _rx) = broadcast::channel(64);
        let push = PushService::from_config(&config);
        Self {
            config,
            store,
            children_cache: Default::default(),
            notif_tx,
            shutdown: CancellationToken::new(),
            push,
        }
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    async fn child_mutex(&self, child_id: &str) -> std::sync::Arc<Mutex<Option<i32>>> {
        let mut map = self.children_cache.lock().await;
        map.entry(child_id.to_string())
            .or_insert_with(Default::default)
            .clone()
    }

    /// Invalidate and recompute remaining minutes for child, broadcasting update.
    pub async fn reset_remaining_minutes(
        &self,
        child_id: &str,
        guard: &mut MinutesGuard<'_>,
    ) -> Result<i32, AppError> {
        let prev = guard.take().unwrap_or_default();
        // now we have None in cache, so remaining_minutes will recompute
        let current = self.remaining_minutes(child_id, guard).await?;

        if current != prev {
            let event = ServerEvent::RemainingUpdated {
                child_id: child_id.to_string(),
                remaining_minutes: current,
            };
            self.dispatch_event(event);
        }

        Ok(current)
    }

    fn dispatch_event(&self, event: ServerEvent) {
        let _ = self.notif_tx.send(event.clone());
        if let Some(push) = &self.push {
            push.dispatch_event(self.store.clone(), event);
        }
    }

    pub async fn remaining_minutes(
        &self,
        child_id: &str,
        guard: &mut MinutesGuard<'_>,
    ) -> Result<i32, AppError> {
        if let Some(v) = **guard {
            return Ok(v);
        }

        // Compute and cache

        let v = self
            .store
            .compute_remaining(child_id)
            .await
            .map_err(AppError::internal)?;

        **guard = Some(v);
        Ok(v)
    }
}

use gamiscreen_shared::api::ServerEvent;

#[derive(Clone, Debug)]
struct ReqId(pub String);

pub fn router(state: AppState) -> Router {
    let tenant_scope = gamiscreen_shared::api::tenant_scope(&state.config.tenant_id);
    let api_v1_prefix = gamiscreen_shared::api::API_V1_PREFIX;
    let sse_path = format!("{}/sse", tenant_scope);
    let version_path = format!("{}/version", api_v1_prefix);
    let auth_login_path = format!("{}/auth/login", api_v1_prefix);
    let auth_renew_path = format!("{}/auth/renew", api_v1_prefix);

    let tenant_private = Router::new()
        .route("/children", get(api_list_children))
        .route("/tasks", get(api_list_tasks))
        .route("/notifications", get(api_list_notifications))
        .route("/notifications/count", get(api_notifications_count))
        .route(
            "/notifications/task-submissions/{id}/approve",
            post(api_approve_submission),
        )
        .route(
            "/notifications/task-submissions/{id}/discard",
            post(api_discard_submission),
        )
        .route("/children/{id}/remaining", get(api_remaining))
        .route("/children/{id}/reward", post(api_child_reward))
        .route("/children/{id}/reward", get(api_list_child_rewards))
        .route("/children/{id}/usage", get(api_list_child_usage))
        .route(
            "/children/{id}/device/{device_id}/heartbeat",
            post(api_device_heartbeat),
        )
        .route(
            "/children/{id}/push/subscriptions",
            post(api_push_subscribe),
        )
        .route(
            "/children/{id}/push/subscriptions/unsubscribe",
            post(api_push_unsubscribe),
        )
        .route("/children/{id}/register", post(api_child_register))
        .route("/children/{id}/tasks", get(api_list_child_tasks))
        .route(
            "/children/{id}/tasks/{task_id}/submit",
            post(api_submit_task),
        )
        .route("/config", get(api_config))
        .with_state(state.clone())
        // IMPORTANT: Last-added layer runs first on request. We want:
        // require_bearer -> enforce_acl -> set_auth_span_fields -> handler
        .layer(middleware::from_fn_with_state(
            state.clone(),
            acl::enforce_acl,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_bearer,
        ))
        .layer(middleware::from_fn(set_auth_span_fields));

    let private = Router::new().nest(tenant_scope.as_str(), tenant_private);

    // Trace with request context (method, path, request_id)
    let trace = TraceLayer::new_for_http().make_span_with(|req: &axum::http::Request<_>| {
        let request_id = req
            .extensions()
            .get::<ReqId>()
            .map(|r| r.0.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        info_span!(
            "request",
            method = %req.method(),
            path = %req.uri().path(),
            request_id = %request_id,
            username = tracing::field::Empty,
            role = tracing::field::Empty,
            tenant_id = tracing::field::Empty,
            child_id = tracing::field::Empty,
            device_id = tracing::field::Empty
        )
    });

    // Public SSE route for push notifications (token passed via query)
    let sse = Router::new()
        .route(&sse_path, get(sse_notifications))
        .with_state(state.clone());

    let auth_router = Router::new()
        .route(&auth_renew_path, post(api_auth_renew))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_bearer,
        ))
        .layer(middleware::from_fn(set_auth_span_fields));

    let app = Router::new()
        .route("/healthz", get(health))
        .route("/api/version", get(api_version))
        .route(&version_path, get(api_version))
        .route(&auth_login_path, post(api_auth_login))
        .merge(auth_router)
        .merge(sse)
        .merge(private)
        .fallback(get(serve_embedded))
        .with_state(state.clone())
        .layer(trace)
        .layer(middleware::from_fn(add_security_headers))
        .layer(middleware::from_fn(add_request_id));

    // Optionally add CORS for dev if configured

    if let Some(origin_cfg) = &state.config.dev_cors_origin {
        let mut origins: Vec<header::HeaderValue> = origin_cfg
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .filter_map(|s| header::HeaderValue::from_str(s).ok())
            .collect();
        let embedded_host = header::HeaderValue::from_static("https://gamiscreen.klimek.ws");
        if !origins.iter().any(|hv| hv == &embedded_host) {
            origins.push(embedded_host);
        }
        if origins.is_empty() {
            origins.push(header::HeaderValue::from_static("http://localhost:5151"));
        }
        let cors = CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);
        app.layer(cors)
    } else {
        app
    }
}

async fn health() -> &'static str {
    "ok"
}

async fn api_version() -> Result<Json<api::VersionInfoDto>, AppError> {
    let v = env!("CARGO_PKG_VERSION").to_string();
    Ok(Json(api::VersionInfoDto { version: v }))
}

async fn add_request_id(
    mut req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Result<AxumResponse, AppError> {
    let hdr = HeaderName::from_static("x-request-id");
    // Use provided x-request-id if present, else generate
    let rid = req
        .headers()
        .get(&hdr)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    // Put into request extensions for trace layer & handlers
    req.extensions_mut().insert(ReqId(rid.clone()));
    // Call next
    let mut resp = next.run(req).await;
    // Set header on response
    if let Ok(hv) = HeaderValue::from_str(&rid) {
        resp.headers_mut().insert(hdr, hv);
    }
    Ok(resp)
}

async fn add_security_headers(
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Result<AxumResponse, AppError> {
    let path = req.uri().path().to_string();
    let mut resp = next.run(req).await;

    // General security headers for all responses
    let headers = resp.headers_mut();
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("SAMEORIGIN"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("geolocation=(), microphone=(), camera=()"),
    );
    headers.insert(
        HeaderName::from_static("cross-origin-opener-policy"),
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("same-origin"),
    );
    // HSTS is only honored on HTTPS; harmless otherwise
    headers.insert(
        HeaderName::from_static("strict-transport-security"),
        HeaderValue::from_static("max-age=31536000; includeSubDomains"),
    );

    // Disable caching for API and health endpoints
    if path == "/healthz" || path.starts_with("/api/") || path == "/api" {
        headers.insert(
            HeaderName::from_static("cache-control"),
            HeaderValue::from_static("no-store, no-cache, must-revalidate, private"),
        );
        headers.insert(
            HeaderName::from_static("pragma"),
            HeaderValue::from_static("no-cache"),
        );
        headers.insert(
            HeaderName::from_static("expires"),
            HeaderValue::from_static("0"),
        );
    }

    Ok(resp)
}

async fn set_auth_span_fields(
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Result<AxumResponse, AppError> {
    if let Some(auth) = req.extensions().get::<AuthCtx>() {
        let claims = &auth.claims;
        let span = Span::current();
        span.record("username", tracing::field::display(&claims.sub));
        span.record("role", tracing::field::debug(&claims.role));
        span.record("tenant_id", tracing::field::display(&claims.tenant_id));
        if let Some(cid) = &claims.child_id {
            span.record("child_id", tracing::field::display(cid));
        }
        if let Some(did) = &claims.device_id {
            span.record("device_id", tracing::field::display(did));
        }
    }
    Ok(next.run(req).await)
}

async fn api_list_children(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthCtx>,
) -> Result<Json<Vec<ChildDto>>, AppError> {
    // ACL enforced by middleware
    let rows = state
        .store
        .list_children()
        .await
        .map_err(AppError::internal)?;
    let items = rows
        .into_iter()
        .map(|c| ChildDto {
            id: c.id,
            display_name: c.display_name,
        })
        .collect();
    Ok(Json(items))
}

async fn api_list_tasks(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthCtx>,
) -> Result<Json<Vec<api::TaskDto>>, AppError> {
    let rows = state.store.list_tasks().await.map_err(AppError::internal)?;
    let items = rows
        .into_iter()
        .map(|t| api::TaskDto {
            id: t.id,
            name: t.name,
            minutes: t.minutes,
        })
        .collect();
    Ok(Json(items))
}

async fn api_list_child_tasks(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthCtx>,
    Path(id): Path<String>,
) -> Result<Json<Vec<api::TaskWithStatusDto>>, AppError> {
    // ACL enforced by middleware
    let rows = state
        .store
        .list_tasks_with_last_done(&id)
        .await
        .map_err(AppError::internal)?;
    let items = rows
        .into_iter()
        .map(|(t, last)| api::TaskWithStatusDto {
            id: t.id,
            name: t.name,
            minutes: t.minutes,
            last_done: last.map(|dt| {
                chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc)
                    .to_rfc3339()
            }),
        })
        .collect();
    Ok(Json(items))
}

async fn api_remaining(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthCtx>,
    Path(id): Path<String>,
) -> Result<Json<api::RemainingDto>, AppError> {
    // ACL enforced by middleware
    let child_mutex = state.child_mutex(&id).await;
    let mut child_guard = child_mutex.lock().await;

    let remaining = state.remaining_minutes(&id, &mut child_guard).await?;
    Ok(Json(api::RemainingDto {
        child_id: id,
        remaining_minutes: remaining,
    }))
}

// New RESTful endpoints (with path ids)
#[derive(Deserialize)]
struct ChildPathId {
    id: String,
}

#[derive(Deserialize)]
struct ChildDevicePath {
    id: String,
    device_id: String,
}

#[derive(Deserialize)]
struct ChildTaskPath {
    id: String,
    task_id: String,
}

async fn api_child_reward(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthCtx>,
    Path(p): Path<ChildPathId>,
    Json(body): Json<api::RewardReq>,
) -> Result<Json<api::RewardResp>, AppError> {
    // Invalidate cache for this child; compute after DB update
    let child_mutex = state.child_mutex(&p.id).await;
    let mut child_guard = child_mutex.lock().await;

    // Determine minutes and description rules:
    // - If task_id is provided, copy task name into description and use task.minutes
    // - Else custom minutes must be provided; description defaults to 'Additional time' when missing/blank
    // Track whether minutes come from a predefined task or custom
    let (mins, desc_to_store): (i32, String) = if let Some(tid) = &body.task_id {
        match state
            .store
            .get_task_by_id(tid)
            .await
            .map_err(AppError::internal)?
        {
            Some(t) => {
                let mut desc = t.name;
                if let Some(note) = body
                    .description
                    .as_ref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                {
                    desc.push_str(" - ");
                    desc.push_str(&note);
                }
                (t.minutes, desc)
            }
            None => return Err(AppError::bad_request(format!("unknown task_id: {}", tid))),
        }
    } else if let Some(m) = body.minutes {
        let provided = body.description.as_deref().unwrap_or("").trim();
        let desc = if provided.is_empty() {
            "Additional time".to_string()
        } else {
            provided.to_string()
        };
        (m, desc)
    } else {
        return Err(AppError::bad_request("minutes or task_id required"));
    };
    // Validation: minutes must be non-zero (both task-derived and custom)
    if mins == 0 {
        return Err(AppError::bad_request("minutes must be non-zero"));
    }

    state
        .store
        .add_reward_minutes(
            &p.id,
            mins,
            body.task_id.as_deref(),
            Some(desc_to_store.as_str()),
        )
        .await
        .map_err(AppError::internal)?;
    if let Some(tid) = body.task_id.as_deref() {
        state
            .store
            .record_task_done(&p.id, tid, &auth.claims.sub)
            .await
            .map_err(AppError::internal)?;
    }
    let remaining = state
        .reset_remaining_minutes(&p.id, &mut child_guard)
        .await?;

    Ok(Json(api::RewardResp {
        remaining_minutes: remaining,
    }))
}

async fn api_push_subscribe(
    State(state): State<AppState>,
    Path(p): Path<ChildPathId>,
    Json(body): Json<api::PushSubscribeReq>,
) -> Result<Json<api::PushSubscribeResp>, AppError> {
    if body.endpoint.trim().is_empty() {
        return Err(AppError::bad_request("endpoint is required"));
    }
    if body.p256dh.trim().is_empty() {
        return Err(AppError::bad_request("p256dh is required"));
    }
    if body.auth.trim().is_empty() {
        return Err(AppError::bad_request("auth is required"));
    }

    let exists = state
        .store
        .child_exists(&p.id)
        .await
        .map_err(AppError::internal)?;
    if !exists {
        return Err(AppError::not_found(format!("child not found: {}", p.id)));
    }

    let tenant_id = state.config.tenant_id.as_str();

    let existing = state
        .store
        .get_push_subscription_by_endpoint(tenant_id, &body.endpoint)
        .await
        .map_err(AppError::internal)?;

    if existing
        .as_ref()
        .map(|sub| sub.child_id != p.id)
        .unwrap_or(true)
    {
        let count = state
            .store
            .push_subscription_count_for_child(tenant_id, &p.id)
            .await
            .map_err(AppError::internal)?;
        if count >= MAX_PUSH_SUBSCRIPTIONS_PER_CHILD {
            return Err(AppError::bad_request(format!(
                "subscription limit ({}) reached for child {}",
                MAX_PUSH_SUBSCRIPTIONS_PER_CHILD, p.id
            )));
        }
    }

    let record = state
        .store
        .upsert_push_subscription(tenant_id, &p.id, &body.endpoint, &body.p256dh, &body.auth)
        .await
        .map_err(AppError::internal)?;

    Ok(Json(api::PushSubscribeResp {
        subscription_id: record.id,
    }))
}

async fn api_push_unsubscribe(
    State(state): State<AppState>,
    Path(p): Path<ChildPathId>,
    Json(body): Json<api::PushUnsubscribeReq>,
) -> Result<StatusCode, AppError> {
    if body.endpoint.trim().is_empty() {
        return Err(AppError::bad_request("endpoint is required"));
    }
    state
        .store
        .delete_push_subscription(state.config.tenant_id.as_str(), &p.id, &body.endpoint)
        .await
        .map_err(AppError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn api_config(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthCtx>,
) -> Result<Json<ConfigResp>, AppError> {
    let push_key = state
        .config
        .push
        .as_ref()
        .filter(|cfg| cfg.enabled)
        .and_then(|cfg| cfg.vapid_public.as_ref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    Ok(Json(ConfigResp {
        push_public_key: push_key,
    }))
}

#[derive(Deserialize)]
struct PageOpts {
    page: Option<usize>,
    per_page: Option<usize>,
}

#[derive(Deserialize)]
struct UsageOpts {
    days: Option<u32>,
    bucket_minutes: Option<u32>,
}

async fn api_list_child_rewards(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthCtx>,
    Path(id): Path<String>,
    Query(opts): Query<PageOpts>,
) -> Result<Json<Vec<api::RewardHistoryItemDto>>, AppError> {
    let page = opts.page.unwrap_or(1);
    let per_page = opts.per_page.unwrap_or(10);
    let rows = state
        .store
        .list_rewards_for_child(&id, page, per_page)
        .await
        .map_err(AppError::internal)?;
    let items = rows
        .into_iter()
        .map(|r| api::RewardHistoryItemDto {
            time: chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
                r.created_at,
                chrono::Utc,
            )
            .to_rfc3339(),
            description: r.description,
            minutes: r.minutes,
        })
        .collect();
    Ok(Json(items))
}

async fn api_list_child_usage(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthCtx>,
    Path(id): Path<String>,
    Query(opts): Query<UsageOpts>,
) -> Result<Json<api::UsageSeriesDto>, AppError> {
    let now = chrono::Utc::now();
    let days = opts.days.unwrap_or(7).clamp(1, 90);
    let bucket_minutes = opts.bucket_minutes.unwrap_or(60).clamp(1, (24 * 60) as u32);

    let now_minute = now.timestamp() / 60;
    let end_minute = now_minute + 1;
    let span_minutes = (days as i64) * 24 * 60;
    let start_minute = end_minute - span_minutes;

    let usage_minutes = state
        .store
        .list_usage_minutes(&id, start_minute, end_minute)
        .await
        .map_err(AppError::internal)?;

    let bucket = bucket_minutes as i64;
    let start_bucket = (start_minute / bucket) * bucket;
    let end_bucket = ((end_minute + bucket - 1) / bucket) * bucket;
    let mut counts: std::collections::BTreeMap<i64, u32> = std::collections::BTreeMap::new();
    for minute in usage_minutes.iter().copied() {
        let bucket_start = (minute / bucket) * bucket;
        counts
            .entry(bucket_start)
            .and_modify(|c| *c += 1)
            .or_insert(1);
    }

    let mut buckets = Vec::new();
    let mut cursor = start_bucket;
    let mut total = 0u32;
    while cursor < end_bucket {
        let count = counts.remove(&cursor).unwrap_or(0);
        total = total.saturating_add(count);
        let ts = cursor
            .checked_mul(60)
            .and_then(|secs| chrono::DateTime::<chrono::Utc>::from_timestamp(secs, 0))
            .ok_or_else(|| AppError::internal("invalid usage timestamp"))?;
        let start_iso = ts.to_rfc3339();
        buckets.push(api::UsageBucketDto {
            start: start_iso,
            minutes: count,
        });
        cursor += bucket;
    }

    let series_start = start_minute
        .checked_mul(60)
        .and_then(|secs| chrono::DateTime::<chrono::Utc>::from_timestamp(secs, 0))
        .ok_or_else(|| AppError::internal("invalid usage timestamp"))?;
    let series_end = end_minute
        .checked_mul(60)
        .and_then(|secs| chrono::DateTime::<chrono::Utc>::from_timestamp(secs, 0))
        .ok_or_else(|| AppError::internal("invalid usage timestamp"))?;

    let dto = api::UsageSeriesDto {
        start: series_start.to_rfc3339(),
        end: series_end.to_rfc3339(),
        bucket_minutes,
        buckets,
        total_minutes: total,
    };

    Ok(Json(dto))
}

// Use shared DTOs
use gamiscreen_shared::api::{NotificationItemDto, NotificationsCountDto};

async fn api_notifications_count(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthCtx>,
) -> Result<Json<NotificationsCountDto>, AppError> {
    if auth.claims.role != Role::Parent {
        return Err(AppError::forbidden());
    }
    let c = state
        .store
        .pending_submissions_count()
        .await
        .map_err(AppError::internal)?;
    Ok(Json(NotificationsCountDto { count: c as u32 }))
}

#[derive(Deserialize)]
struct SseQuery {
    token: String,
}

async fn sse_notifications(
    State(state): State<AppState>,
    Query(q): Query<SseQuery>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>>, AppError> {
    // Validate token from query
    // NOTE: SSE auth does not consult or touch the sessions table; it only verifies the JWT.
    let claims = jwt::decode_and_verify(&q.token, state.config.jwt_secret.as_bytes())
        .map_err(|_| AppError::unauthorized())?;
    // Access control
    crate::server::acl::validate_ws_access_from_claims(&claims)?;

    use futures::StreamExt;
    use tokio_stream::wrappers::BroadcastStream;

    // Prepare a stream that first sends initial snapshot, then relays broadcasted events
    // Prepare initial snapshot
    let mut init_items: Vec<ServerEvent> = Vec::new();
    if claims.role == Role::Parent {
        if let Ok(c) = state.store.pending_submissions_count().await {
            init_items.push(ServerEvent::PendingCount { count: c as u32 });
        }
    } else if claims.role == Role::Child
        && let Some(cid) = claims.child_id.clone()
    {
        let child_mutex = state.child_mutex(&cid).await;
        let mut guard = child_mutex.lock().await;
        if let Ok(rem) = state.remaining_minutes(&cid, &mut guard).await {
            init_items.push(ServerEvent::RemainingUpdated {
                child_id: cid.clone(),
                remaining_minutes: rem,
            });
        }
    }

    let rx = state.notif_tx.subscribe();
    let bstream = BroadcastStream::new(rx)
        .filter_map(move |msg| {
            let claims2 = claims.clone();
            futures::future::ready(match msg {
                Ok(ev) => match (&claims.role, &ev) {
                    (Role::Parent, _) => Some(ev),
                    (Role::Child, ServerEvent::RemainingUpdated { child_id, .. }) => {
                        if let Some(cid) = &claims2.child_id {
                            if cid == child_id { Some(ev) } else { None }
                        } else {
                            None
                        }
                    }
                    (Role::Child, ServerEvent::PendingCount { .. }) => None,
                },
                Err(_) => None,
            })
        })
        .map(|ev| Ok(Event::default().data(serde_json::to_string(&ev).unwrap())));

    let init_stream = futures::stream::iter(
        init_items
            .into_iter()
            .map(|ev| Ok(Event::default().data(serde_json::to_string(&ev).unwrap()))),
    );
    let stream = init_stream
        .chain(bstream)
        .take_until(state.shutdown.clone().cancelled_owned());
    Ok(Sse::new(stream))
}

async fn api_list_notifications(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthCtx>,
) -> Result<Json<Vec<NotificationItemDto>>, AppError> {
    if auth.claims.role != Role::Parent {
        return Err(AppError::forbidden());
    }
    let rows = state
        .store
        .list_pending_submissions()
        .await
        .map_err(AppError::internal)?;
    let items = rows
        .into_iter()
        .map(|(s, c, t)| NotificationItemDto {
            id: s.id,
            kind: "task_submission".to_string(),
            child_id: c.id,
            child_display_name: c.display_name,
            task_id: t.id,
            task_name: t.name,
            submitted_at: chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
                s.submitted_at,
                chrono::Utc,
            )
            .to_rfc3339(),
        })
        .collect();
    Ok(Json(items))
}

async fn api_approve_submission(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthCtx>,
    Path(id): Path<i32>,
) -> Result<StatusCode, AppError> {
    if auth.claims.role != Role::Parent {
        return Err(AppError::forbidden());
    }
    let child_opt = state
        .store
        .approve_submission(id, &auth.claims.sub)
        .await
        .map_err(AppError::internal)?;
    // Invalidate remaining cache for that child so subsequent reads reflect new reward
    if let Some(child_id) = child_opt {
        let child_mutex = state.child_mutex(&child_id).await;
        let mut child_guard = child_mutex.lock().await;
        state
            .reset_remaining_minutes(&child_id, &mut child_guard)
            .await?;
        // Notify parents about updated count
        if let Ok(c) = state.store.pending_submissions_count().await {
            let event = ServerEvent::PendingCount { count: c as u32 };
            state.dispatch_event(event);
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn api_discard_submission(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthCtx>,
    Path(id): Path<i32>,
) -> Result<StatusCode, AppError> {
    state
        .store
        .discard_submission(id)
        .await
        .map_err(AppError::internal)?;
    if let Ok(c) = state.store.pending_submissions_count().await {
        let event = ServerEvent::PendingCount { count: c as u32 };
        state.dispatch_event(event);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn api_submit_task(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthCtx>,
    Path(p): Path<ChildTaskPath>,
) -> Result<StatusCode, AppError> {
    // child can submit only for own id
    if auth.claims.role != Role::Child {
        return Err(AppError::forbidden());
    }
    match &auth.claims.child_id {
        Some(cid) if cid == &p.id => {}
        _ => return Err(AppError::forbidden()),
    }
    // Ensure task exists
    match state
        .store
        .get_task_by_id(&p.task_id)
        .await
        .map_err(AppError::internal)?
    {
        Some(_) => {}
        None => return Err(AppError::bad_request("unknown task_id")),
    }
    state
        .store
        .submit_task(&p.id, &p.task_id)
        .await
        .map_err(AppError::internal)?;
    if let Ok(c) = state.store.pending_submissions_count().await {
        let event = ServerEvent::PendingCount { count: c as u32 };
        state.dispatch_event(event);
    }
    Ok(StatusCode::NO_CONTENT)
}

type MinutesGuard<'a> = MutexGuard<'a, Option<i32>>;

async fn api_device_heartbeat(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthCtx>,
    Path(p): Path<ChildDevicePath>,
    Json(body): Json<api::HeartbeatReq>,
) -> Result<Json<api::HeartbeatResp>, AppError> {
    // Lock the child ID to avoid concurrent updates
    let child_mutex = state.child_mutex(&p.id).await;
    let mut child_guard = child_mutex.lock().await;

    // Invalidate cache for this child; compute after DB update
    state
        .store
        .process_usage_minutes(&p.id, &p.device_id, &body.minutes)
        .await
        .map_err(AppError::internal)?;
    // Update cache and return
    let remaining = state
        .reset_remaining_minutes(&p.id, &mut child_guard)
        .await?;
    Ok(Json(api::HeartbeatResp {
        remaining_minutes: remaining,
    }))
}

async fn api_child_register(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthCtx>,
    Path(p): Path<ChildPathId>,
    Json(body): Json<api::ClientRegisterReq>,
) -> Result<Json<api::ClientRegisterResp>, AppError> {
    // Ensure child exists in DB
    let exists = state
        .store
        .child_exists(&p.id)
        .await
        .map_err(AppError::internal)?;
    if !exists {
        return Err(AppError::not_found(format!("child not found: {}", p.id)));
    }
    let device_id = body.device_id.clone();
    let child_username = state
        .config
        .users
        .iter()
        .find(|u| u.role == Role::Child && u.child_id.as_deref() == Some(p.id.as_str()))
        .map(|u| u.username.clone())
        .ok_or_else(|| {
            tracing::error!(child_id = %p.id, "register: no child user configured for id");
            AppError::internal("child login not configured")
        })?;
    let token = auth::issue_jwt_for_user(
        &state,
        &child_username,
        Role::Child,
        Some(p.id.clone()),
        Some(device_id.clone()),
        &auth.claims.tenant_id,
    )
    .await?;
    Ok(Json(api::ClientRegisterResp {
        token,
        child_id: p.id,
        device_id,
    }))
}

// JwtClaims moved to auth module

async fn api_auth_login(
    State(state): State<AppState>,
    Json(body): Json<api::AuthReq>,
) -> Result<Json<api::AuthResp>, AppError> {
    // Find user in config
    let user = state
        .config
        .users
        .iter()
        .find(|u| u.username == body.username)
        .ok_or_else(|| {
            tracing::warn!(username=%body.username, "login: unknown username");
            AppError::unauthorized()
        })?;
    if !verify(&body.password, &user.password_hash).map_err(|e| {
        tracing::error!(username=%body.username, error=%e, "login: bcrypt verify failed");
        AppError::internal(e)
    })? {
        tracing::warn!(username=%body.username, "login: invalid password");
        return Err(AppError::unauthorized());
    }
    // For child role, ensure child_id provided
    if user.role == Role::Child && user.child_id.is_none() {
        tracing::error!(username=%body.username, "login: child user missing child_id in config");
        return Err(AppError::internal("child user missing child_id"));
    }
    let token = auth::issue_jwt_for_user(
        &state,
        &user.username,
        user.role,
        user.child_id.clone(),
        None,
        &state.config.tenant_id,
    )
    .await?;
    Ok(Json(api::AuthResp { token }))
}

async fn api_auth_renew(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthCtx>,
) -> Result<Json<api::AuthResp>, AppError> {
    let claims = auth.claims;
    let token = auth::issue_jwt_for_user(
        &state,
        &claims.sub,
        claims.role,
        claims.child_id.clone(),
        claims.device_id.clone(),
        &claims.tenant_id,
    )
    .await?;

    match state.store.delete_session(&claims.jti).await {
        Ok(true) => {}
        Ok(false) => {
            tracing::warn!(jti = %claims.jti, "auth renew: previous session missing");
        }
        Err(e) => {
            tracing::error!(jti = %claims.jti, error = %e, "auth renew: failed to delete session");
            if let Ok(new_claims) = jwt::decode_unverified(&token) {
                let _ = state.store.delete_session(&new_claims.jti).await;
            }
            return Err(AppError::internal(e));
        }
    }

    Ok(Json(api::AuthResp { token }))
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Unauthorized,
    Forbidden,
    NotFound(String),
    Internal(String),
}

impl AppError {
    fn bad_request<T: Into<String>>(msg: T) -> Self {
        Self::BadRequest(msg.into())
    }
    fn unauthorized() -> Self {
        Self::Unauthorized
    }
    fn forbidden() -> Self {
        Self::Forbidden
    }
    fn not_found<T: Into<String>>(msg: T) -> Self {
        Self::NotFound(msg.into())
    }
    fn internal<E: std::fmt::Display>(e: E) -> Self {
        Self::Internal(e.to_string())
    }
}

impl axum::response::IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, msg, kind, detail) = match self {
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m, "bad_request", None),
            AppError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized".into(),
                "unauthorized",
                None,
            ),
            AppError::Forbidden => (StatusCode::FORBIDDEN, "forbidden".into(), "forbidden", None),
            AppError::NotFound(m) => (StatusCode::NOT_FOUND, m, "not_found", None),
            // Do not leak internal error details to clients, but log them
            AppError::Internal(m) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal server error".into(),
                "internal",
                Some(m),
            ),
        };
        // Log any error responses at ERROR level to file for troubleshooting
        if let Some(detail) = detail {
            tracing::error!(status = %status, kind = kind, message = %msg, detail = %detail, "request failed");
        } else {
            tracing::error!(status = %status, kind = kind, message = %msg, "request failed");
        }
        let body = axum::Json(ErrorBody { error: msg });
        (status, body).into_response()
    }
}

#[derive(RustEmbed)]
#[folder = "../gamiscreen-web/dist/"]
struct WebAssets;

async fn serve_embedded(
    uri: axum::http::Uri,
) -> Result<axum::response::Response, (StatusCode, String)> {
    let path = uri.path().trim_start_matches('/');
    // Do not serve frontend for API paths; return 404 so clients don't misinterpret as success.
    if path == "api" || path.starts_with("api/") {
        return Err((StatusCode::NOT_FOUND, "not found".to_string()));
    }
    let candidate = if path.is_empty() { "index.html" } else { path };
    let asset = WebAssets::get(candidate)
        .or_else(|| WebAssets::get("index.html"))
        .ok_or((StatusCode::NOT_FOUND, "asset not found".to_string()))?;

    let bytes = asset.data.into_owned();
    let mime = from_path(candidate).first_or_octet_stream();

    let mut resp = axum::response::Response::new(axum::body::Body::from(bytes));
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_str(mime.as_ref())
            .unwrap_or(header::HeaderValue::from_static("application/octet-stream")),
    );
    Ok(resp)
}
