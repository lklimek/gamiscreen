use super::{auth::AuthCtx, AppError, Role};
use super::auth;
use percent_encoding::percent_decode_str;
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

    // Helper to split the path into normalized segments (without leading/trailing empty parts)
    fn segments(p: &str) -> Vec<&str> {
        p.split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<&str>>()
    }
    // Helper to percent-decode a single segment
    fn decode(seg: &str) -> String {
        percent_decode_str(seg).decode_utf8_lossy().to_string()
    }
    let segs = segments(&path);

    // Default deny: mark allowed when a rule applies
    let mut allowed = false;

    // Parent-only endpoints
    if segs.as_slice() == ["api", "children"] && method == Method::GET {
        if auth.role != Role::Parent {
            return Err(AppError::forbidden());
        }
        allowed = true;
    }

    // Tasks (global list): allow both roles
    if segs.as_slice() == ["api", "tasks"] && method == Method::GET {
        allowed = true;
    }

    // Remaining: allow parent for any id; children only for self
    if method == Method::GET && segs.len() == 4 && segs[0] == "api" && segs[1] == "children" && segs[3] == "remaining" {
        if auth.role != Role::Parent {
            let child = decode(segs[2]);
            match &auth.child_id {
                Some(id) if id == &child => {}
                _ => return Err(AppError::forbidden()),
            }
        }
        allowed = true;
    }

    // Child tasks listing: allow parent for any id; children only for self
    if method == Method::GET && segs.len() == 4 && segs[0] == "api" && segs[1] == "children" && segs[3] == "tasks" {
        if auth.role != Role::Parent {
            let child = decode(segs[2]);
            match &auth.child_id {
                Some(id) if id == &child => {}
                _ => return Err(AppError::forbidden()),
            }
        }
        allowed = true;
    }

    // Child rewards listing: allow parent for any id; children only for self
    if method == Method::GET && segs.len() == 4 && segs[0] == "api" && segs[1] == "children" && segs[3] == "reward" {
        if auth.role != Role::Parent {
            let child = decode(segs[2]);
            match &auth.child_id {
                Some(id) if id == &child => {}
                _ => return Err(AppError::forbidden()),
            }
        }
        allowed = true;
    }

    // Rewards (new REST path): parent-only on any child id
    if method == Method::POST && segs.len() == 4 && segs[0] == "api" && segs[1] == "children" && segs[3] == "reward" {
        if auth.role != Role::Parent {
            return Err(AppError::forbidden());
        }
        allowed = true;
    }

    // Notifications: parent-only
    if segs.as_slice() == ["api", "notifications"] && method == Method::GET {
        if auth.role != Role::Parent { return Err(AppError::forbidden()); }
        allowed = true;
    }
    if segs.as_slice() == ["api", "notifications", "count"] && method == Method::GET {
        if auth.role != Role::Parent { return Err(AppError::forbidden()); }
        allowed = true;
    }
    if method == Method::POST
        && segs.len() == 5
        && segs[0] == "api"
        && segs[1] == "notifications"
        && segs[2] == "task-submissions"
        && (segs[4] == "approve" || segs[4] == "discard")
        && segs[3].parse::<i32>().is_ok()
    {
        if auth.role != Role::Parent { return Err(AppError::forbidden()); }
        allowed = true;
    }

    // Child task submit: child-only for own id
    if method == Method::POST
        && segs.len() == 6
        && segs[0] == "api"
        && segs[1] == "children"
        && segs[3] == "tasks"
        && segs[5] == "submit"
    {
        if auth.role != Role::Child { return Err(AppError::forbidden()); }
        let child = decode(segs[2]);
        match &auth.child_id {
            Some(id) if id == &child => allowed = true,
            _ => return Err(AppError::forbidden()),
        }
    }

    // Register device (new REST path): parent any id; child only for own id
    if method == Method::POST && segs.len() == 4 && segs[0] == "api" && segs[1] == "children" && segs[3] == "register" {
        if auth.role != Role::Parent {
            let child = decode(segs[2]);
            match &auth.child_id {
                Some(id) if id == &child => {}
                _ => return Err(AppError::forbidden()),
            }
        }
        allowed = true;
    }

    // Heartbeat (new REST path): child only and must match both child_id and device_id
    if method == Method::POST
        && segs.len() == 6
        && segs[0] == "api"
        && segs[1] == "children"
        && segs[3] == "device"
        && segs[5] == "heartbeat"
    {
        if auth.role != Role::Child {
            tracing::warn!(role=?auth.role, "ACL heartbeat: non-child role");
            return Err(AppError::forbidden());
        }
        let child = decode(segs[2]);
        let device = decode(segs[4]);
        match (&auth.child_id, &auth.device_id) {
            (Some(cid), Some(did)) if cid == &child && did == &device => allowed = true,
            _ => return Err(AppError::forbidden()),
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

/// Validate WebSocket access based on JWT claims.
/// Parents are allowed. Children are allowed if a child_id is present.
pub fn validate_ws_access_from_claims(claims: &auth::JwtClaims) -> Result<(), AppError> {
    match claims.role {
        Role::Parent => Ok(()),
        Role::Child => {
            if claims.child_id.is_some() {
                Ok(())
            } else {
                Err(AppError::forbidden())
            }
        }
    }
}
