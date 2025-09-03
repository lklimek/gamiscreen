/// Helpers for parsing REST-style paths used by the server.
/// Not a full router; just tiny extractors for ACL and related middleware.

/// Extracts `{id}` from `/api/children/{id}/...`.
/// Returns a percent-decoded owned String.
pub fn child_id_from_path(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/api/children/")?;
    let seg = rest.split('/').next()?;
    if seg.is_empty() {
        None
    } else {
        Some(
            percent_encoding::percent_decode_str(seg)
                .decode_utf8_lossy()
                .to_string(),
        )
    }
}

/// Extracts `(child_id, device_id)` from `/api/children/{id}/device/{device_id}/...`.
/// Extracts `(child_id, device_id)` from `/api/children/{id}/device/{device_id}/...`.
/// Returns percent-decoded owned Strings.
pub fn child_and_device_from_path(path: &str) -> Option<(String, String)> {
    let rest = path.strip_prefix("/api/children/")?;
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
    let child = percent_encoding::percent_decode_str(child_enc)
        .decode_utf8_lossy()
        .to_string();
    let device = percent_encoding::percent_decode_str(device_enc)
        .decode_utf8_lossy()
        .to_string();
    Some((child, device))
}
