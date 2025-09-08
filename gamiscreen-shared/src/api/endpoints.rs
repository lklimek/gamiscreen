use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

fn base_join(base: &str, path: &str) -> String {
    let b = base.trim_end_matches('/');
    let p = path.trim_start_matches('/');
    format!("{}/{}", b, p)
}

fn enc(s: &str) -> String {
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

pub fn auth_login(base: &str) -> String {
    base_join(base, "/api/auth/login")
}
pub fn children(base: &str) -> String {
    base_join(base, "/api/children")
}
pub fn tasks(base: &str) -> String {
    base_join(base, "/api/tasks")
}
pub fn child_remaining(base: &str, child_id: &str) -> String {
    base_join(base, &format!("/api/children/{}/remaining", enc(child_id)))
}
pub fn child_tasks(base: &str, child_id: &str) -> String {
    base_join(base, &format!("/api/children/{}/tasks", enc(child_id)))
}
pub fn child_reward(base: &str, child_id: &str) -> String {
    base_join(base, &format!("/api/children/{}/reward", enc(child_id)))
}
pub fn child_register(base: &str, child_id: &str) -> String {
    base_join(base, &format!("/api/children/{}/register", enc(child_id)))
}
pub fn child_device_heartbeat(base: &str, child_id: &str, device_id: &str) -> String {
    base_join(
        base,
        &format!(
            "/api/children/{}/device/{}/heartbeat",
            enc(child_id),
            enc(device_id)
        ),
    )
}

pub fn update_manifest(base: &str) -> String {
    base_join(base, "/api/update/manifest")
}
