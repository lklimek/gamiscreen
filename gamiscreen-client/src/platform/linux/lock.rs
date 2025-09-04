use std::process::Stdio;

use tokio::process::Command;
use tracing::info;

use crate::{AppError, config::ClientConfig};
use zbus::proxy::Proxy;
use zbus_names::OwnedBusName;

#[derive(Clone, Debug)]
pub enum LockBackend {
    Gnome,
    Login1,
    CommandOverride(Vec<String>),
}

pub async fn detect_lock_backend(cfg: &ClientConfig) -> Result<LockBackend, AppError> {
    if let Some(custom) = &cfg.lock_cmd {
        info!("using lock_cmd override");
        return Ok(LockBackend::CommandOverride(custom.clone()));
    }

    // Check session bus for GNOME screensaver
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
        info!("detected org.gnome.ScreenSaver on session bus");
        return Ok(LockBackend::Gnome);
    }

    // Check system bus for login1
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
        info!("detected org.freedesktop.login1 on system bus");
        return Ok(LockBackend::Login1);
    }

    Err(AppError::Dbus(
        "no supported DBus lock interface detected and no lock_cmd set".into(),
    ))
}

pub async fn enforce_lock_backend(backend: &LockBackend) -> Result<(), AppError> {
    match backend {
        LockBackend::Gnome => lock_via_gnome_screensaver().await,
        LockBackend::Login1 => lock_via_login1().await,
        LockBackend::CommandOverride(cmd) => lock_via_command(cmd).await,
    }
}

async fn lock_via_gnome_screensaver() -> Result<(), AppError> {
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

async fn lock_via_login1() -> Result<(), AppError> {
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

async fn lock_via_command(cmd: &Vec<String>) -> Result<(), AppError> {
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
