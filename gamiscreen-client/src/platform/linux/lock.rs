use std::process::Stdio;

use tokio::process::Command;
use tracing::{info, warn};

use crate::{AppError, config::ClientConfig};
use clap::ValueEnum;
use zbus::proxy::Proxy;
use zbus_names::{InterfaceName, OwnedBusName};

#[derive(Clone, Debug)]
pub enum LockBackend {
    Gnome,
    /// Lock all GUI sessions for the current user via login1 Session.Lock
    Login1UserSessions,
    CommandOverride(Vec<String>),
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum LockMethod {
    All,
    Gnome,
    Fdo,
    Login1Manager,
    Login1Session,
    /// Enumerate user's GUI sessions and call Session.Lock for each
    Login1UserSessions,
    Loginctl,
    XdgScreensaver,
}

pub async fn detect_lock_backend(_cfg: &ClientConfig) -> Result<LockBackend, AppError> {
    // Prefer login1 per-session locking (robust under games/Wayland).
    if let Ok(conn) = zbus::Connection::system().await
        && let Ok(proxy) = zbus::fdo::DBusProxy::new(&conn).await
        && proxy
            .name_has_owner(
                OwnedBusName::try_from("org.freedesktop.login1")
                    .unwrap()
                    .into(),
            )
            .await
            .unwrap_or(false)
    {
        info!("detected org.freedesktop.login1 on system bus; selecting login1 per-session");
        return Ok(LockBackend::Login1UserSessions);
    }

    Err(AppError::Dbus(
        "org.freedesktop.login1 not available on system bus".into(),
    ))
}

pub async fn enforce_lock_backend(backend: &LockBackend) -> Result<(), AppError> {
    match backend {
        LockBackend::Gnome => lock_via_gnome_screensaver().await,
        LockBackend::Login1UserSessions => lock_via_login1_user_sessions().await,
        LockBackend::CommandOverride(cmd) => lock_via_command(cmd).await,
    }
}

pub async fn lock_using_method(method: LockMethod) -> Result<(), AppError> {
    match method {
        LockMethod::All => Err(AppError::Config("'All' is not a concrete method".into())),
        LockMethod::Gnome => lock_via_gnome_screensaver().await,
        LockMethod::Fdo => lock_via_fdo_screensaver().await,
        LockMethod::Login1Manager => lock_via_login1_manager().await,
        LockMethod::Login1Session => lock_via_login1_session().await,
        LockMethod::Login1UserSessions => lock_via_login1_user_sessions().await,
        LockMethod::Loginctl => lock_via_command(&["loginctl".into(), "lock-session".into()]).await,
        LockMethod::XdgScreensaver => {
            lock_via_command(&["xdg-screensaver".into(), "lock".into()]).await
        }
    }
}

/// Detect if the current user session is locked.
/// Tries GNOME ScreenSaver first, then freedesktop.org ScreenSaver, then falls back to login1's LockedHint.
pub async fn is_session_locked() -> Result<bool, AppError> {
    tracing::debug!("checking if session is locked");
    // Try via GNOME ScreenSaver on the session bus
    if let Ok(conn) = zbus::Connection::session().await
        && let Ok(proxy) = zbus::fdo::DBusProxy::new(&conn).await
        && proxy
            .name_has_owner(
                OwnedBusName::try_from("org.gnome.ScreenSaver")
                    .unwrap()
                    .into(),
            )
            .await
            .unwrap_or(false)
    {
        // Prefer calling GetActive() which returns a bool
        let proxy = Proxy::new(
            &conn,
            "org.gnome.ScreenSaver",
            "/org/gnome/ScreenSaver",
            "org.gnome.ScreenSaver",
        )
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;
        let msg = proxy
            .call_method("GetActive", &())
            .await
            .map_err(|e| AppError::Dbus(e.to_string()))?;
        let body = msg.body();
        if let Ok(active) = body.deserialize::<bool>() {
            tracing::debug!(active, "org.gnome.ScreenSaver GetActive returned");
            return Ok(active);
        }
        warn!("org.gnome.ScreenSaver GetActive returned unexpected body; assuming unlocked");
        return Ok(false);
    }

    // Try via freedesktop.org ScreenSaver on the session bus
    if let Ok(conn) = zbus::Connection::session().await
        && let Ok(proxy) = zbus::fdo::DBusProxy::new(&conn).await
        && proxy
            .name_has_owner(
                OwnedBusName::try_from("org.freedesktop.ScreenSaver")
                    .unwrap()
                    .into(),
            )
            .await
            .unwrap_or(false)
    {
        let proxy = Proxy::new(
            &conn,
            "org.freedesktop.ScreenSaver",
            "/org/freedesktop/ScreenSaver",
            "org.freedesktop.ScreenSaver",
        )
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;
        let msg = proxy
            .call_method("GetActive", &())
            .await
            .map_err(|e| AppError::Dbus(e.to_string()))?;
        let body = msg.body();
        if let Ok(active) = body.deserialize::<bool>() {
            tracing::debug!(active, "org.freedesktop.ScreenSaver GetActive returned");
            return Ok(active);
        }
        warn!("org.freedesktop.ScreenSaver GetActive returned unexpected body; assuming unlocked");
        return Ok(false);
    }

    // Fallback to login1 LockedHint via system bus
    if let Ok(conn) = zbus::Connection::system().await {
        // Manager: GetSessionByPID(self_pid)
        let mgr = Proxy::new(
            &conn,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;
        let pid: u32 = std::process::id();
        let msg = mgr
            .call_method("GetSessionByPID", &(pid))
            .await
            .map_err(|e| AppError::Dbus(e.to_string()))?;
        // Deserialize message body into object path
        let body = msg.body();
        if let Ok(path) = body.deserialize::<zbus::zvariant::OwnedObjectPath>() {
            let props = zbus::fdo::PropertiesProxy::builder(&conn)
                .destination("org.freedesktop.login1")
                .map_err(|e| AppError::Dbus(e.to_string()))?
                .path(path.as_str())
                .map_err(|e| AppError::Dbus(e.to_string()))?
                .build()
                .await
                .map_err(|e| AppError::Dbus(e.to_string()))?;
            let iface = InterfaceName::try_from("org.freedesktop.login1.Session").unwrap();
            let val = props
                .get(iface, "LockedHint")
                .await
                .map_err(|e| AppError::Dbus(e.to_string()))?;
            // Try to convert to bool; default to false on mismatch
            if let Ok(b) = bool::try_from(val) {
                tracing::debug!(
                    locked_hint = b,
                    "org.freedesktop.login1.Session LockedHint read successfully"
                );
                return Ok(b);
            }
        }
    }

    Ok(false)
}

pub async fn lock_via_gnome_screensaver() -> Result<(), AppError> {
    let conn = zbus::Connection::session()
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;
    let proxy = Proxy::new(
        &conn,
        "org.gnome.ScreenSaver",
        "/org/gnome/ScreenSaver",
        "org.gnome.ScreenSaver",
    )
    .await
    .map_err(|e| AppError::Dbus(e.to_string()))?;
    proxy
        .call_method("Lock", &())
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;
    Ok(())
}

pub async fn lock_via_login1_manager() -> Result<(), AppError> {
    let conn = zbus::Connection::system()
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;
    let proxy = Proxy::new(
        &conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await
    .map_err(|e| AppError::Dbus(e.to_string()))?;
    proxy
        .call_method("LockSessions", &())
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;
    Ok(())
}

pub async fn lock_via_login1_session() -> Result<(), AppError> {
    use zbus::zvariant::OwnedObjectPath;

    let conn = zbus::Connection::system()
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;

    let mgr = Proxy::new(
        &conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await
    .map_err(|e| AppError::Dbus(e.to_string()))?;

    let pid: u32 = std::process::id();
    let msg = mgr
        .call_method("GetSessionByPID", &(pid))
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;
    let body = msg.body();
    let path: OwnedObjectPath = body
        .deserialize()
        .map_err(|e| AppError::Dbus(e.to_string()))?;

    let sess = Proxy::new(
        &conn,
        "org.freedesktop.login1",
        path.as_str(),
        "org.freedesktop.login1.Session",
    )
    .await
    .map_err(|e| AppError::Dbus(e.to_string()))?;
    sess.call_method("Lock", &())
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;
    Ok(())
}

/// Lock all GUI sessions (x11/wayland/mir) for the current user via login1.
/// Mirrors the approach used in timekpr-next: enumerate the user's sessions and
/// invoke org.freedesktop.login1.Session.Lock() on each GUI session.
pub async fn lock_via_login1_user_sessions() -> Result<(), AppError> {
    use zbus::zvariant::OwnedObjectPath;

    let conn = zbus::Connection::system()
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;

    // Manager proxy
    let mgr = Proxy::new(
        &conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await
    .map_err(|e| AppError::Dbus(e.to_string()))?;

    // Resolve the current user's login1.User object path
    let uid: u32 = nix::unistd::geteuid().as_raw();
    let msg = mgr
        .call_method("GetUser", &(uid))
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;
    let user_path: OwnedObjectPath = msg
        .body()
        .deserialize()
        .map_err(|e| AppError::Dbus(e.to_string()))?;

    // Read the Sessions property: array of (string id, object path)
    let user_props = zbus::fdo::PropertiesProxy::builder(&conn)
        .destination("org.freedesktop.login1")
        .map_err(|e| AppError::Dbus(e.to_string()))?
        .path(user_path.as_str())
        .map_err(|e| AppError::Dbus(e.to_string()))?
        .build()
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;

    let iface = InterfaceName::try_from("org.freedesktop.login1.User").unwrap();
    let val = user_props
        .get(iface, "Sessions")
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;

    // Try to decode into Vec<(String, OwnedObjectPath)>
    let sessions: Vec<(String, OwnedObjectPath)> =
        <Vec<(String, OwnedObjectPath)>>::try_from(val).unwrap_or_default();

    // Allowed GUI session types
    const GUI_TYPES: &[&str] = &["x11", "wayland", "mir"];

    // Iterate and lock each GUI session
    for (_sid, path) in sessions.into_iter() {
        // Read session Type
        let sess_props = zbus::fdo::PropertiesProxy::builder(&conn)
            .destination("org.freedesktop.login1")
            .map_err(|e| AppError::Dbus(e.to_string()))?
            .path(path.as_str())
            .map_err(|e| AppError::Dbus(e.to_string()))?
            .build()
            .await
            .map_err(|e| AppError::Dbus(e.to_string()))?;
        let sess_iface = InterfaceName::try_from("org.freedesktop.login1.Session").unwrap();
        let vtype = sess_props
            .get(sess_iface, "Type")
            .await
            .map_err(|e| AppError::Dbus(e.to_string()))?;
        let stype = String::try_from(vtype).unwrap_or_default();
        // Only lock GUI sessions
        if !GUI_TYPES.contains(&stype.as_str()) {
            continue;
        }
        tracing::debug!(session_path = %path.as_str(), session_type = %stype, "locking GUI session");
        let sess = Proxy::new(
            &conn,
            "org.freedesktop.login1",
            path.as_str(),
            "org.freedesktop.login1.Session",
        )
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;
        let _ = sess.call_method("Lock", &()).await; // best effort per session
    }

    Ok(())
}

pub async fn lock_via_fdo_screensaver() -> Result<(), AppError> {
    let conn = zbus::Connection::session()
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;
    let proxy = Proxy::new(
        &conn,
        "org.freedesktop.ScreenSaver",
        "/org/freedesktop/ScreenSaver",
        "org.freedesktop.ScreenSaver",
    )
    .await
    .map_err(|e| AppError::Dbus(e.to_string()))?;
    proxy
        .call_method("Lock", &())
        .await
        .map_err(|e| AppError::Dbus(e.to_string()))?;
    Ok(())
}

async fn lock_via_command(cmd: &[String]) -> Result<(), AppError> {
    let (program, args) = cmd
        .split_first()
        .ok_or_else(|| AppError::Config("lock_cmd empty".into()))?;
    info!(program=%program, args=?args, "running screen lock command (override)");
    let status = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?;
    if !status.success() {
        return Err(AppError::Io(std::io::Error::other(format!(
            "lock command failed with status {status}"
        ))));
    }
    Ok(())
}
