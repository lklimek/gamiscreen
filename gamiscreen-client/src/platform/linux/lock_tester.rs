use tracing::{error, info};
use crate::platform::linux::lock::{LockMethod, lock_using_method};

pub async fn run_lock_cmd(method: LockMethod) {
    info!("lock tester starting");
    detect_backends().await;

    match method {
        LockMethod::All => run_all_methods_interactive().await,
        m => run_single_method(m).await,
    }
}

async fn run_single_method(method: LockMethod) {
    info!(?method, "testing method");
    if let Err(e) = lock_using_method(method).await {
        error!(error=%e, ?method, "lock attempt failed");
    }
    report_lock_status().await;
}

async fn run_all_methods_interactive() {
    use LockMethod as M;
    let mut working: Vec<&'static str> = Vec::new();

    let tests: &[(M, &str)] = &[
        (M::Gnome, "GNOME ScreenSaver (session bus)"),
        (M::Fdo, "org.freedesktop.ScreenSaver (session bus)"),
        (M::Login1Manager, "login1 Manager.LockSessions (system bus)"),
        (M::Login1Session, "login1 Session.Lock (system bus; current)"),
        (M::Loginctl, "loginctl lock-session (command)"),
        (M::XdgScreensaver, "xdg-screensaver lock (command; X11)"),
    ];

    for (m, label) in tests {
        info!(method = *label, "testing");
        if let Err(e) = lock_using_method(*m).await {
            error!(error=%e, method = *label, "lock attempt failed");
        }
        report_lock_status().await;
        if prompt_yes_no(&format!("Did the screen lock for: {} ? [y/N] ", label)) {
            working.push(*label);
        }
    }

    if working.is_empty() {
        println!("Summary: no methods confirmed working.");
    } else {
        println!("Summary: confirmed working methods:");
        for w in working {
            println!("- {}", w);
        }
    }

    info!("lock tester finished");
}

fn prompt_yes_no(prompt: &str) -> bool {
    use std::io::Write;
    print!("{}", prompt);
    let _ = std::io::stdout().flush();
    let mut buf = String::new();
    let _ = std::io::stdin().read_line(&mut buf);
    matches!(buf.chars().next().map(|c| c.to_ascii_lowercase()), Some('y'))
}



async fn detect_backends() {
    // session bus: GNOME and org.freedesktop.ScreenSaver
    if let Ok(conn) = zbus::Connection::session().await {
        if let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await {
            let has_gnome = dbus
                .name_has_owner(
                    zbus_names::OwnedBusName::try_from("org.gnome.ScreenSaver")
                        .unwrap()
                        .into(),
                )
                .await
                .unwrap_or(false);
            let has_fdo = dbus
                .name_has_owner(
                    zbus_names::OwnedBusName::try_from("org.freedesktop.ScreenSaver")
                        .unwrap()
                        .into(),
                )
                .await
                .unwrap_or(false);
            info!(session_bus = true, has_gnome, has_fdo, "session bus screensaver services");
        }
    }

    // system bus: login1
    if let Ok(conn) = zbus::Connection::system().await {
        if let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await {
            let has_login1 = dbus
                .name_has_owner(
                    zbus_names::OwnedBusName::try_from("org.freedesktop.login1")
                        .unwrap()
                        .into(),
                )
                .await
                .unwrap_or(false);
            info!(system_bus = true, has_login1, "system bus login1 service");
        }
    }
}

async fn report_lock_status() {
    match crate::platform::linux::lock::is_session_locked().await {
        Ok(b) => info!(locked = b, "lock status after attempt"),
        Err(e) => error!(error=%e, "could not determine lock status"),
    }
}
