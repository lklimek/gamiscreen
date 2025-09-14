use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::AppError;

use super::Platform;

/// Windows implementation of the cross-platform interface.
pub struct WindowsPlatform {
    notifier: Arc<Mutex<Notifier>>, // simple logging-based notifier for now
}

impl WindowsPlatform {
    pub fn new() -> Self {
        Self {
            notifier: Arc::new(Mutex::new(Notifier::new())),
        }
    }
}

impl Default for WindowsPlatform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Platform for WindowsPlatform {
    async fn lock(&self) -> Result<(), AppError> {
        // SAFETY: Calling the system API to lock the current workstation.
        // This is a synchronous call; wrap directly since it returns immediately.
        let ok = unsafe { windows_sys::Win32::System::Shutdown::LockWorkStation() };
        if ok == 0 {
            Err(AppError::Io(std::io::Error::other(
                "LockWorkStation failed",
            )))
        } else {
            Ok(())
        }
    }

    async fn is_session_locked(&self) -> Result<bool, AppError> {
        // Determine lock state by checking the active input desktop name.
        // When the workstation is locked, the input desktop is typically "Winlogon".
        // When unlocked and at the user's desktop, it's usually "Default".
        use windows_sys::Win32::System::StationsAndDesktops::{
            CloseDesktop, DESKTOP_READOBJECTS, DESKTOP_SWITCHDESKTOP, GetUserObjectInformationW,
            HDESK, OpenInputDesktop, UOI_NAME,
        };

        unsafe {
            let hdesk: HDESK = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS | DESKTOP_SWITCHDESKTOP);
            if hdesk.is_null() {
                // Could not query; assume unlocked rather than erroring hard.
                warn!("Windows: OpenInputDesktop failed; assuming unlocked");
                return Ok(false);
            }

            let mut needed: u32 = 0;
            // First call to get required buffer size (in bytes)
            let _ =
                GetUserObjectInformationW(hdesk, UOI_NAME, std::ptr::null_mut(), 0, &mut needed);
            if needed == 0 {
                let _ = CloseDesktop(hdesk);
                warn!("Windows: GetUserObjectInformationW returned zero length; assuming unlocked");
                return Ok(false);
            }
            // Allocate UTF-16 buffer (needed is in bytes)
            let len_u16 = (needed as usize).div_ceil(2); // round up
            let mut buf: Vec<u16> = vec![0u16; len_u16];
            let ok = GetUserObjectInformationW(
                hdesk,
                UOI_NAME,
                buf.as_mut_ptr() as *mut _ as *mut _,
                needed,
                &mut needed,
            );
            let _ = CloseDesktop(hdesk);
            if ok == 0 {
                warn!("Windows: GetUserObjectInformationW failed; assuming unlocked");
                return Ok(false);
            }
            // Convert to Rust String; trim trailing NULs
            let mut end = 0usize;
            while end < buf.len() && buf[end] != 0 {
                end += 1;
            }
            let name = String::from_utf16_lossy(&buf[..end]);
            debug!(desktop_name=%name, "Windows input desktop queried");
            // Treat any non-Default input desktop as locked (e.g., Winlogon)
            let locked = !name.eq_ignore_ascii_case("Default");
            Ok(locked)
        }
    }

    async fn notify(&self, total_secs: u64) {
        self.notifier.lock().await.show_countdown(total_secs).await;
    }

    async fn update_notification(&self, seconds_left: u64) {
        self.notifier.lock().await.update(seconds_left).await;
    }

    async fn hide_notification(&self) {
        self.notifier.lock().await.close().await;
    }

    fn device_id(&self) -> String {
        // Prefer stable SID-based identity; include computer name to distinguish devices
        if let Some(sid) = current_user_sid_string() {
            let computer = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "pc".to_string());
            return format!("win-{}-{}", computer, sid);
        }
        // Fallback
        let username = std::env::var("USERNAME").unwrap_or_else(|_| "user".to_string());
        let computer = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "pc".to_string());
        format!("win-{}-{}", computer, username)
    }

    async fn install(&self, user: Option<String>) -> Result<(), AppError> {
        // Ignore provided user on Windows and install for current user
        if let Some(u) = user {
            let cur = std::env::var("USERNAME").unwrap_or_default();
            if !u.is_empty() && u.to_lowercase() != cur.to_lowercase() {
                warn!(requested=%u, current=%cur, "Windows install ignores --user; installing for current user");
            }
        }
        install_for_current_user(true).await
    }

    async fn uninstall(&self, user: Option<String>) -> Result<(), AppError> {
        if let Some(u) = user {
            let cur = std::env::var("USERNAME").unwrap_or_default();
            if !u.is_empty() && u.to_lowercase() != cur.to_lowercase() {
                warn!(requested=%u, current=%cur, "Windows uninstall ignores --user; uninstalling for current user");
            }
        }
        uninstall_for_current_user().await
    }
}

/// Simple notifier placeholder for Windows: logs to tracing for now.
#[derive(Debug, Default)]
struct Notifier;

impl Notifier {
    fn new() -> Self {
        Self
    }

    async fn show_countdown(&mut self, total_secs: u64) {
        info!("[COUNTDOWN] {} s do wylogowania (Windows)", total_secs);
    }

    async fn update(&mut self, seconds_left: u64) {
        debug!("[COUNTDOWN UPDATE] {} s left (Windows)", seconds_left);
    }

    async fn close(&mut self) {
        debug!("[COUNTDOWN CLOSED] (Windows)");
    }
}

/// Returns the current user's SID as a string (e.g., "S-1-5-21-...")
fn current_user_sid_string() -> Option<String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TokenUser};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut needed: u32 = 0;
        // First call to get required buffer size
        let _ = GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        if needed == 0 {
            CloseHandle(token);
            return None;
        }
        let mut buf: Vec<u8> = vec![0u8; needed as usize];
        if GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr() as *mut _,
            needed,
            &mut needed,
        ) == 0
        {
            CloseHandle(token);
            return None;
        }
        CloseHandle(token);

        #[repr(C)]
        #[allow(non_snake_case)]
        struct SID_AND_ATTRIBUTES {
            Sid: *mut core::ffi::c_void,
            Attributes: u32,
        }
        #[repr(C)]
        #[allow(non_snake_case)]
        struct TOKEN_USER_RS {
            User: SID_AND_ATTRIBUTES,
        }

        let tu = &*(buf.as_ptr() as *const TOKEN_USER_RS);
        let mut sid_str_ptr: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(tu.User.Sid, &mut sid_str_ptr) == 0 || sid_str_ptr.is_null() {
            return None;
        }
        // Convert PWSTR to Rust String
        let mut len = 0usize;
        while *sid_str_ptr.add(len) != 0 {
            len += 1;
        }
        let slice = core::slice::from_raw_parts(sid_str_ptr, len);
        let sid = String::from_utf16_lossy(slice);
        let _ = LocalFree(sid_str_ptr as HLOCAL);
        Some(sid)
    }
}

const TASK_NAME: &str = "GamiScreen Client";

async fn install_for_current_user(start_now: bool) -> Result<(), AppError> {
    use tokio::process::Command;
    use tracing::info;

    let exe = std::env::current_exe().map_err(AppError::Io)?;
    let exe_str = exe.display().to_string();
    // schtasks expects quotes around the full path; we also pass -- to ensure default agent run
    let tr = format!("\"{}\"", exe_str);

    // Create or update the task (use /F)
    let status = Command::new("schtasks")
        .args([
            "/Create",
            "/F",
            "/SC",
            "ONLOGON",
            "/RL",
            "LIMITED",
            "/TN",
            TASK_NAME,
            "/TR",
            &tr,
        ])
        .status()
        .await
        .map_err(AppError::Io)?;
    if !status.success() {
        return Err(AppError::Io(std::io::Error::other(format!(
            "schtasks /Create failed with status {}",
            status
        ))));
    }

    info!(task=TASK_NAME, path=%exe_str, "Windows Scheduled Task installed for current user");

    if start_now {
        // Best-effort: start it immediately in current session
        let _ = Command::new("schtasks")
            .args(["/Run", "/TN", TASK_NAME])
            .status()
            .await;
    }
    Ok(())
}

async fn uninstall_for_current_user() -> Result<(), AppError> {
    use tokio::process::Command;
    let status = Command::new("schtasks")
        .args(["/Delete", "/F", "/TN", TASK_NAME])
        .status()
        .await
        .map_err(AppError::Io)?;
    if !status.success() {
        // Treat missing task as success
        warn!(task=TASK_NAME, status=%status, "schtasks /Delete failed or task missing; continuing");
    }
    Ok(())
}
