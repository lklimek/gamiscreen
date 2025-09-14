use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::warn;

use crate::AppError;

use super::Platform;

pub mod install;
pub mod lock;
pub mod notify;

/// Windows implementation of the cross-platform interface.
pub struct WindowsPlatform {
    notifier: Arc<Mutex<notify::Notifier>>, // simple logging-based notifier for now
}

impl WindowsPlatform {
    pub fn new() -> Self {
        Self {
            notifier: Arc::new(Mutex::new(notify::Notifier::new())),
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
        lock::lock_now().await
    }

    async fn is_session_locked(&self) -> Result<bool, AppError> {
        lock::is_session_locked().await
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

    fn replace_and_restart(&self, staged_src: &Path, current_exe: &Path, _args: &[String]) -> ! {
        // Prepare a .new file next to the current exe
        let parent = current_exe.parent().unwrap_or_else(|| Path::new("."));
        let fname = current_exe
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("gamiscreen-client.exe");
        let new_path = parent.join(format!("{}.new", fname));
        // Copy staged to new_path (overwrite if exists)
        if let Err(e) = std::fs::copy(staged_src, &new_path) {
            tracing::warn!(error=%e, "Windows: failed to copy staged update");
            std::process::exit(0);
        }
        // Create a small .bat script to swap files after this process exits.
        // Assume we always run as a Windows Service; control via SCM.
        let bat_path = parent.join(format!("update-runner-{}.bat", std::process::id()));
        let svc = install::TASK_NAME; // assume service name equals install task name
        // Service-aware update: stop service, wait for STOPPED, move new, start service
        let script = format!(
            concat!(
                "@echo off\r\n",
                "sc stop \"{}\" > NUL\r\n",
                ":waitstopped\r\n",
                "for /f \"tokens=3\" %%A in ('sc query \"{}\" ^| findstr STATE') do set state=%%A\r\n",
                "if /I not \"%state%\"==\"STOPPED\" (timeout /t 1 /nobreak > NUL & goto waitstopped)\r\n",
                "move /y \"{}\" \"{}\" > NUL\r\n",
                "sc start \"{}\" > NUL\r\n",
                "del \"%~f0\"\r\n",
            ),
            svc,
            svc,
            new_path.display(),
            current_exe.display(),
            svc
        );
        if let Err(e) = std::fs::write(&bat_path, script) {
            tracing::warn!(error=%e, "Windows: failed to write update script");
            std::process::exit(0);
        }
        // Launch the script detached and exit
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        const DETACHED_PROCESS: u32 = 0x00000008;
        let flags = CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS;
        let _ = std::process::Command::new("cmd.exe")
            .arg("/C")
            .arg(&bat_path)
            .creation_flags(flags)
            .spawn();
        std::process::exit(0);
    }

    async fn install(&self, user: Option<String>) -> Result<(), AppError> {
        // Ignore provided user on Windows and install for current user
        if let Some(u) = user {
            let cur = std::env::var("USERNAME").unwrap_or_default();
            if !u.is_empty() && u.to_lowercase() != cur.to_lowercase() {
                warn!(requested=%u, current=%cur, "Windows install ignores --user; installing for current user");
            }
        }
        install::install_for_current_user(true).await
    }

    async fn uninstall(&self, user: Option<String>) -> Result<(), AppError> {
        if let Some(u) = user {
            let cur = std::env::var("USERNAME").unwrap_or_default();
            if !u.is_empty() && u.to_lowercase() != cur.to_lowercase() {
                warn!(requested=%u, current=%cur, "Windows uninstall ignores --user; uninstalling for current user");
            }
        }
        install::uninstall_for_current_user().await
    }
}

// No arg escaping required for service start; SCM does not accept args here.

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
