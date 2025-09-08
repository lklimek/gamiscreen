mod acl;
pub mod auth;
mod config;

use crate::server::auth::AuthCtx;
use axum::http::{HeaderName, HeaderValue};
use axum::middleware;
use axum::response::Response as AxumResponse;
use axum::{
    Json, Router,
    extract::{Extension, Path, Query, State},
    http::{Method, StatusCode, header},
    routing::{get, post},
};
use bcrypt::verify;
pub use config::{AppConfig, Role, UserConfig};
use gamiscreen_shared::api;
use gamiscreen_shared::api::ChildDto;
use mime_guess::from_path;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, MutexGuard};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{Span, info_span};
use uuid::Uuid;

type ChildCacheMap =
    std::sync::Arc<Mutex<std::collections::HashMap<String, std::sync::Arc<Mutex<Option<i32>>>>>>;

#[derive(Clone)]
pub struct AppState {
    pub config: AppConfig,
    pub store: crate::storage::Store,
    // Cache of remaining minutes per child. None => needs recompute
    children_cache: ChildCacheMap,
}

impl AppState {
    pub fn new(config: AppConfig, store: crate::storage::Store) -> Self {
        Self {
            config,
            store,
            children_cache: Default::default(),
        }
    }

    async fn child_mutex(&self, child_id: &str) -> std::sync::Arc<Mutex<Option<i32>>> {
        let mut map = self.children_cache.lock().await;
        map.entry(child_id.to_string())
            .or_insert_with(Default::default)
            .clone()
    }

    pub async fn reset_remaining_minutes(&self, guard: &mut MinutesGuard<'_>) {
        guard.take();
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

#[derive(Clone, Debug)]
struct ReqId(pub String);

pub fn router(state: AppState) -> Router {
    let private = Router::new()
        .route("/api/children", get(api_list_children))
        .route("/api/tasks", get(api_list_tasks))
        .route("/api/children/{id}/remaining", get(api_remaining))
        .route("/api/children/{id}/reward", post(api_child_reward))
        .route("/api/children/{id}/reward", get(api_list_child_rewards))
        .route(
            "/api/children/{id}/device/{device_id}/heartbeat",
            post(api_device_heartbeat),
        )
        .route("/api/children/{id}/register", post(api_child_register))
        .route("/api/children/{id}/tasks", get(api_list_child_tasks))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            acl::enforce_acl,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_bearer,
        ))
        .layer(middleware::from_fn(set_auth_span_fields));

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
            child_id = tracing::field::Empty,
            device_id = tracing::field::Empty
        )
    });

    let app = Router::new()
        .route("/healthz", get(health))
        .route("/api/update/manifest", get(api_update_manifest))
        .route("/api/auth/login", post(api_auth_login))
        .merge(private)
        .fallback(get(serve_embedded))
        .with_state(state.clone())
        .layer(trace)
        .layer(middleware::from_fn(add_security_headers))
        .layer(middleware::from_fn(add_request_id));

    // Optionally add CORS for dev if configured

    if let Some(origin) = &state.config.dev_cors_origin {
        let hv = header::HeaderValue::from_str(origin)
            .unwrap_or(header::HeaderValue::from_static("http://localhost:5173"));
        let cors = CorsLayer::new()
            .allow_origin(hv)
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

async fn api_update_manifest() -> Result<axum::response::Response, AppError> {
    // Serve the embedded JSON from OUT_DIR produced by build.rs
    static DATA: &str = include_str!(concat!(env!("OUT_DIR"), "/update_manifest.json"));
    let mut resp = axum::response::Response::new(axum::body::Body::from(DATA.to_string()));
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    Ok(resp)
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
        let span = Span::current();
        span.record("username", tracing::field::display(&auth.username));
        span.record("role", tracing::field::debug(&auth.role));
        if let Some(cid) = &auth.child_id {
            span.record("child_id", tracing::field::display(cid));
        }
        if let Some(did) = &auth.device_id {
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

async fn api_child_reward(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthCtx>,
    Path(p): Path<ChildPathId>,
    Json(body): Json<api::RewardReq>,
) -> Result<Json<api::RewardResp>, AppError> {
    // Invalidate cache for this child; compute after DB update
    let child_mutex = state.child_mutex(&p.id).await;
    let mut child_guard = child_mutex.lock().await;
    state.reset_remaining_minutes(&mut child_guard).await;

    let mins = if let Some(m) = body.minutes {
        m
    } else if let Some(tid) = &body.task_id {
        match state
            .store
            .get_task_by_id(tid)
            .await
            .map_err(AppError::internal)?
        {
            Some(t) => t.minutes,
            None => return Err(AppError::bad_request(format!("unknown task_id: {}", tid))),
        }
    } else {
        return Err(AppError::bad_request("minutes or task_id required"));
    };
    if mins <= 0 {
        return Err(AppError::bad_request("minutes must be positive"));
    }

    state
        .store
        .add_reward_minutes(&p.id, mins, body.task_id.as_deref())
        .await
        .map_err(AppError::internal)?;
    if let Some(tid) = body.task_id.as_deref() {
        state
            .store
            .record_task_done(&p.id, tid, &auth.username)
            .await
            .map_err(AppError::internal)?;
    }
    let remaining = state.remaining_minutes(&p.id, &mut child_guard).await?;
    Ok(Json(api::RewardResp {
        remaining_minutes: remaining,
    }))
}

#[derive(Deserialize)]
struct PageOpts {
    page: Option<usize>,
    per_page: Option<usize>,
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
        .map(|(r, name)| api::RewardHistoryItemDto {
            time: chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
                r.created_at,
                chrono::Utc,
            )
            .to_rfc3339(),
            task_name: name,
            minutes: r.minutes,
        })
        .collect();
    Ok(Json(items))
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
    state.reset_remaining_minutes(&mut child_guard).await;
    state
        .store
        .process_usage_minutes(&p.id, &p.device_id, &body.minutes)
        .await
        .map_err(AppError::internal)?;
    // Update cache and return
    let remaining = state.remaining_minutes(&p.id, &mut child_guard).await?;
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
    let token = auth::issue_jwt_for_user(
        &state,
        &auth.username,
        Role::Child,
        Some(p.id.clone()),
        Some(device_id.clone()),
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
    )
    .await?;
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
