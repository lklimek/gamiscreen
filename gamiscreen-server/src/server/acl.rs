use super::{AppError, AppState, auth::AuthCtx};
use axum::response::Response;
use axum::{
    extract::{OriginalUri, State},
    http::{Method, Request},
    middleware::Next,
};
use gamiscreen_shared::auth::Role;
use gamiscreen_shared::jwt::JwtClaims;
use percent_encoding::percent_decode_str;

pub async fn enforce_acl(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, AppError> {
    let path = req
        .extensions()
        .get::<OriginalUri>()
        .map(|orig| orig.0.path().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());
    let method = req.method().clone();
    let Some(auth) = req.extensions().get::<AuthCtx>() else {
        return Err(AppError::unauthorized());
    };
    let claims = &auth.claims;

    let segs = segmented(&path);
    let tenant_prefix = ["api", "v1", "family", state.config.tenant_id.as_str()];
    if !segs.as_slice().starts_with(&tenant_prefix) {
        tracing::warn!(?segs, "ACL: path outside tenant scope");
        return Err(AppError::forbidden());
    }
    let rest = &segs[tenant_prefix.len()..];

    let decision = match claims.role {
        Role::Parent => allow_parent(&method, rest),
        Role::Child => allow_child(&method, rest, claims),
    };

    if let Err(err) = decision {
        tracing::warn!(
            method = %method,
            path = %path,
            username = %claims.sub,
            role = ?claims.role,
            token_child = ?claims.child_id,
            token_device = ?claims.device_id,
            "ACL: no rule matched; denying"
        );
        return Err(err);
    }

    Ok(next.run(req).await)
}

fn allow_parent(method: &Method, rest: &[&str]) -> Result<(), AppError> {
    match rest {
        ["children"] if *method == Method::GET => Ok(()),
        ["tasks"] if *method == Method::GET => Ok(()),
        ["notifications"] if *method == Method::GET => Ok(()),
        ["notifications", "count"] if *method == Method::GET => Ok(()),
        ["notifications", "task-submissions", id, action]
            if *method == Method::POST
                && (action == &"approve" || action == &"discard")
                && id.parse::<i32>().is_ok() =>
        {
            Ok(())
        }
        ["children", _, "remaining"] if *method == Method::GET => Ok(()),
        ["children", _, "usage"] if *method == Method::GET => Ok(()),
        ["children", _, "reward"] if *method == Method::GET || *method == Method::POST => Ok(()),
        ["children", _, "tasks"] if *method == Method::GET => Ok(()),
        ["children", _, "register"] if *method == Method::POST => Ok(()),
        ["children", _, "push", "subscriptions"] if *method == Method::POST => Ok(()),
        ["children", _, "push", "subscriptions", "unsubscribe"] if *method == Method::POST => {
            Ok(())
        }
        _ => Err(AppError::forbidden()),
    }
}

fn allow_child(method: &Method, rest: &[&str], claims: &JwtClaims) -> Result<(), AppError> {
    match rest {
        ["tasks"] if *method == Method::GET => Ok(()),
        ["children", child, "remaining"] if *method == Method::GET => ensure_child(claims, child),
        ["children", child, "usage"] if *method == Method::GET => ensure_child(claims, child),
        ["children", child, "tasks"] if *method == Method::GET => ensure_child(claims, child),
        ["children", child, "reward"] if *method == Method::GET => ensure_child(claims, child),
        ["children", child, "tasks", _, "submit"] if *method == Method::POST => {
            ensure_child(claims, child)
        }
        ["children", child, "register"] if *method == Method::POST => ensure_child(claims, child),
        ["children", child, "device", device, "heartbeat"] if *method == Method::POST => {
            ensure_child(claims, child)?;
            ensure_device(claims, device)
        }
        ["children", child, "push", "subscriptions"] if *method == Method::POST => {
            ensure_child(claims, child)
        }
        ["children", child, "push", "subscriptions", "unsubscribe"] if *method == Method::POST => {
            ensure_child(claims, child)
        }
        _ => Err(AppError::forbidden()),
    }
}

fn segmented(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

fn decode(seg: &str) -> String {
    percent_decode_str(seg).decode_utf8_lossy().to_string()
}

fn ensure_child(claims: &JwtClaims, seg: &str) -> Result<(), AppError> {
    let expected = claims.child_id.as_ref().ok_or_else(AppError::forbidden)?;
    let provided = decode(seg);
    if expected == &provided {
        Ok(())
    } else {
        Err(AppError::forbidden())
    }
}

fn ensure_device(claims: &JwtClaims, seg: &str) -> Result<(), AppError> {
    let expected = claims.device_id.as_ref().ok_or_else(AppError::forbidden)?;
    let provided = decode(seg);
    if expected == &provided {
        Ok(())
    } else {
        Err(AppError::forbidden())
    }
}

/// Validate WebSocket access based on JWT claims.
/// Parents are allowed. Children are allowed if a child_id is present.
pub fn validate_ws_access_from_claims(claims: &JwtClaims) -> Result<(), AppError> {
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
