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

pub fn auth_login(base: &str) -> String {
    base_join(base, &format!("{}/auth/login", API_V1_PREFIX))
}
pub fn auth_renew(base: &str) -> String {
    base_join(base, &format!("{}/auth/renew", API_V1_PREFIX))
}
pub fn children(base: &str, tenant_id: &str) -> String {
    base_join(base, &format!("{}/children", tenant_scope(tenant_id)))
}
pub fn tasks(base: &str, tenant_id: &str) -> String {
    base_join(base, &format!("{}/tasks", tenant_scope(tenant_id)))
}
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

pub fn version(base: &str) -> String {
    base_join(base, &format!("{}/version", API_V1_PREFIX))
}

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

pub fn tenant_config(base: &str, tenant_id: &str) -> String {
    base_join(base, &format!("{}/config", tenant_scope(tenant_id)))
}
