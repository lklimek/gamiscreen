//! URL builders for all REST API endpoints.
//!
//! Each function takes a `base` URL (e.g. `"https://example.com"`) and any
//! required path parameters, returning the fully qualified endpoint URL.
//! Path segments are percent-encoded to handle special characters safely.

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

use super::{API_V1_PREFIX, tenant_scope};

fn base_join(base: &str, path: &str) -> String {
    let b = base.trim_end_matches('/');
    let p = path.trim_start_matches('/');
    format!("{}/{}", b, p)
}

fn enc(s: &str) -> String {
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

/// `POST` -- authenticate a parent and obtain a session token.
pub fn auth_login(base: &str) -> String {
    base_join(base, &format!("{}/auth/login", API_V1_PREFIX))
}

/// `POST` -- renew an existing session token before it expires.
pub fn auth_renew(base: &str) -> String {
    base_join(base, &format!("{}/auth/renew", API_V1_PREFIX))
}

/// `GET` -- list all children in a family.
pub fn children(base: &str, tenant_id: &str) -> String {
    base_join(base, &format!("{}/children", tenant_scope(tenant_id)))
}

/// `GET` -- list all task definitions for a family.
pub fn tasks(base: &str, tenant_id: &str) -> String {
    base_join(base, &format!("{}/tasks", tenant_scope(tenant_id)))
}

/// `GET` -- fetch a child's current remaining minutes, balance, and blocked state.
pub fn child_remaining(base: &str, tenant_id: &str, child_id: &str) -> String {
    base_join(
        base,
        &format!(
            "{}/children/{}/remaining",
            tenant_scope(tenant_id),
            enc(child_id)
        ),
    )
}

/// `GET` -- list tasks with per-child completion status.
pub fn child_tasks(base: &str, tenant_id: &str, child_id: &str) -> String {
    base_join(
        base,
        &format!(
            "{}/children/{}/tasks",
            tenant_scope(tenant_id),
            enc(child_id)
        ),
    )
}

/// `POST` -- grant a reward (task completion or ad-hoc) to a child.
pub fn child_reward(base: &str, tenant_id: &str, child_id: &str) -> String {
    base_join(
        base,
        &format!(
            "{}/children/{}/reward",
            tenant_scope(tenant_id),
            enc(child_id)
        ),
    )
}

/// `GET` -- fetch aggregated screen-time usage for a child.
pub fn child_usage(base: &str, tenant_id: &str, child_id: &str) -> String {
    base_join(
        base,
        &format!(
            "{}/children/{}/usage",
            tenant_scope(tenant_id),
            enc(child_id)
        ),
    )
}

/// `POST` -- register a device client for a child and obtain a device token.
pub fn child_register(base: &str, tenant_id: &str, child_id: &str) -> String {
    base_join(
        base,
        &format!(
            "{}/children/{}/register",
            tenant_scope(tenant_id),
            enc(child_id)
        ),
    )
}

/// `POST` -- submit a heartbeat batch from a device, reporting active-use minutes.
pub fn child_device_heartbeat(
    base: &str,
    tenant_id: &str,
    child_id: &str,
    device_id: &str,
) -> String {
    base_join(
        base,
        &format!(
            "{}/children/{}/device/{}/heartbeat",
            tenant_scope(tenant_id),
            enc(child_id),
            enc(device_id)
        ),
    )
}

/// `GET` -- retrieve the server's version information.
pub fn version(base: &str) -> String {
    base_join(base, &format!("{}/version", API_V1_PREFIX))
}

/// `POST` -- create a Web Push subscription for a child's browser.
pub fn child_push_subscribe(base: &str, tenant_id: &str, child_id: &str) -> String {
    base_join(
        base,
        &format!(
            "{}/children/{}/push/subscriptions",
            tenant_scope(tenant_id),
            enc(child_id)
        ),
    )
}

/// `POST` -- remove a Web Push subscription by endpoint URL.
pub fn child_push_unsubscribe(base: &str, tenant_id: &str, child_id: &str) -> String {
    base_join(
        base,
        &format!(
            "{}/children/{}/push/subscriptions/unsubscribe",
            tenant_scope(tenant_id),
            enc(child_id)
        ),
    )
}

/// `GET` -- fetch tenant-level configuration (e.g. VAPID public key).
pub fn tenant_config(base: &str, tenant_id: &str) -> String {
    base_join(base, &format!("{}/config", tenant_scope(tenant_id)))
}
