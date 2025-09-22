use percent_encoding::percent_decode_str;

use crate::api;

/// Extracts `{id}` from `/api/v1/family/{tenant}/children/{id}/...`.
/// Returns a percent-decoded owned [`String`].
pub fn child_id_from_path(path: &str, tenant_id: &str) -> Option<String> {
    let prefix = format!("{}/children/", api::tenant_scope(tenant_id));
    let rest = path.strip_prefix(&prefix)?;
    let seg = rest.split('/').next()?;
    if seg.is_empty() {
        None
    } else {
        Some(percent_decode_str(seg).decode_utf8_lossy().to_string())
    }
}

/// Extracts `(child_id, device_id)` from
/// `/api/v1/family/{tenant}/children/{id}/device/{device_id}/...`.
/// Returns percent-decoded owned [`String`] pairs.
pub fn child_and_device_from_path(path: &str, tenant_id: &str) -> Option<(String, String)> {
    let prefix = format!("{}/children/", api::tenant_scope(tenant_id));
    let rest = path.strip_prefix(&prefix)?;
    let mut it = rest.split('/');
    let child_enc = it.next()?;
    if child_enc.is_empty() {
        return None;
    }
    let kw = it.next()?;
    if kw != "device" {
        return None;
    }
    let device_enc = it.next()?;
    if device_enc.is_empty() {
        return None;
    }
    let child = percent_decode_str(child_enc)
        .decode_utf8_lossy()
        .to_string();
    let device = percent_decode_str(device_enc)
        .decode_utf8_lossy()
        .to_string();
    Some((child, device))
}
