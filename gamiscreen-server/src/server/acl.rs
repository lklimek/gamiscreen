use super::{AppError, Role, auth::AuthCtx};
use crate::shared::path::{child_and_device_from_path, child_id_from_path};
use axum::response::Response;
use axum::{
    extract::State,
    http::{Method, Request},
    middleware::Next,
};
// use http_body_util::BodyExt; // not used

pub async fn enforce_acl(
    State(_state): State<super::AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, AppError> {
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    let Some(auth) = req.extensions().get::<AuthCtx>().cloned() else {
        return Err(AppError::unauthorized());
    };

    // Default deny: mark allowed when a rule applies
    let mut allowed = false;

    // Parent-only endpoints
    if path == "/api/children" && method == Method::GET {
        if auth.role != Role::Parent {
            return Err(AppError::forbidden());
        }
        allowed = true;
    }

    // Tasks (global list): allow both roles
    if path == "/api/tasks" && method == Method::GET {
        allowed = true;
    }

    // Remaining: allow parent for any id; children only for self
    if method == Method::GET && path.starts_with("/api/children/") && path.ends_with("/remaining") {
        if auth.role != Role::Parent {
            let Some(child) = child_id_from_path(&path) else {
                return Err(AppError::forbidden());
            };
            match &auth.child_id {
                Some(id) if id == &child => {}
                _ => return Err(AppError::forbidden()),
            }
        }
        allowed = true;
    }

    // Child tasks listing: allow parent for any id; children only for self
    if method == Method::GET && path.starts_with("/api/children/") && path.ends_with("/tasks") {
        if auth.role != Role::Parent {
            let Some(child) = child_id_from_path(&path) else {
                return Err(AppError::forbidden());
            };
            match &auth.child_id {
                Some(id) if id == &child => {}
                _ => return Err(AppError::forbidden()),
            }
        }
        allowed = true;
    }

    // Child rewards listing: allow parent for any id; children only for self
    if method == Method::GET && path.starts_with("/api/children/") && path.ends_with("/reward") {
        if auth.role != Role::Parent {
            let Some(child) = child_id_from_path(&path) else {
                return Err(AppError::forbidden());
            };
            match &auth.child_id {
                Some(id) if id == &child => {}
                _ => return Err(AppError::forbidden()),
            }
        }
        allowed = true;
    }

    // Rewards (new REST path): parent-only on any child id
    if method == Method::POST && path.starts_with("/api/children/") && path.ends_with("/reward") {
        if auth.role != Role::Parent {
            return Err(AppError::forbidden());
        }
        allowed = true;
    }

    // Register device (new REST path): parent any id; child only for own id
    if method == Method::POST && path.starts_with("/api/children/") && path.ends_with("/register") {
        if auth.role != Role::Parent {
            let Some(child) = child_id_from_path(&path) else {
                return Err(AppError::forbidden());
            };
            match &auth.child_id {
                Some(id) if id == &child => {}
                _ => return Err(AppError::forbidden()),
            }
        }
        allowed = true;
    }

    // Heartbeat (new REST path): child only and must match both child_id and device_id
    if method == Method::POST && path.starts_with("/api/children/") && path.ends_with("/heartbeat")
    {
        if auth.role != Role::Child {
            tracing::warn!(role=?auth.role, "ACL heartbeat: non-child role");
            return Err(AppError::forbidden());
        }
        // expected: /api/children/{id}/device/{device_id}/heartbeat
        if let Some((child, device)) = child_and_device_from_path(&path) {
            match (&auth.child_id, &auth.device_id) {
                (Some(cid), Some(did)) if cid == &child && did == &device => {}
                other => {
                    return Err(AppError::forbidden());
                }
            }
            allowed = true;
        } else {
            tracing::warn!(%path, "ACL heartbeat: cannot parse child/device from path");
            return Err(AppError::forbidden());
        }
    }

    // Old endpoints removed: /api/reward, /api/heartbeat, /api/client/register

    if !allowed {
        tracing::warn!(
            method = %method,
            path = %path,
            username = %auth.username,
            role = ?auth.role,
            token_child = ?auth.child_id,
            token_device = ?auth.device_id,
            "ACL: no rule matched; denying"
        );
        return Err(AppError::forbidden());
    }
    Ok(next.run(req).await)
}
