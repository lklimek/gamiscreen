use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::warn;

use super::Platform;
use crate::AppError;

pub mod lock;
pub mod notify;
pub mod service;
pub mod service_cli;

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

    async fn update_notification(&self, remaining_secs: i64) {
        self.notifier.lock().await.update(remaining_secs).await;
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

    fn replace_and_restart(&self, staged_src: &Path, current_exe: &Path, args: &[String]) -> ! {
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
        // Create a small .bat script to swap files after this process exits and
        // relaunch the app directly (Scheduled Task friendly; no SCM involved).
        let bat_path = parent.join(format!("update-runner-{}.bat", std::process::id()));
        // Quote exe and args for cmd.exe; escape embedded quotes by doubling them
        let exe_quoted = format!("\"{}\"", current_exe.display());
        let mut args_quoted = String::new();
        for a in args {
            let mut s = a.replace('"', "\"\"");
            s.insert(0, '"');
            s.push('"');
            args_quoted.push(' ');
            args_quoted.push_str(&s);
        }
        let script = format!(
            concat!(
                "@echo off\r\n",
                "setlocal enableextensions\r\n",
                "set PID={}\r\n",
                ":waitproc\r\n",
                "tasklist /FI \"PID eq %PID%\" | find \"%PID%\" > NUL\r\n",
                "if not errorlevel 1 (timeout /t 1 /nobreak > NUL & goto waitproc)\r\n",
                "move /y \"{}\" \"{}\" > NUL\r\n",
                "start \"\" {}{}\r\n",
                "endlocal\r\n",
                "del \"%~f0\"\r\n",
            ),
            std::process::id(),
            new_path.display(),
            current_exe.display(),
            exe_quoted,
            args_quoted,
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

    async fn install(&self, _user: Option<String>) -> Result<(), AppError> {
        warn!("Windows per-user install path invoked; directing to service commands");
        Err(AppError::Config(
            "Windows per-user install has been removed; use `gamiscreen-client service install` instead.".into(),
        ))
    }

    async fn uninstall(&self, _user: Option<String>) -> Result<(), AppError> {
        warn!("Windows per-user uninstall path invoked; directing to service commands");
        Err(AppError::Config(
            "Windows per-user uninstall has been removed; use the Windows service commands instead.".into(),
        ))
    }
}

// On Windows we relaunch directly, so we handle basic arg quoting above.

/// Returns the current user's SID as a string (e.g., "S-1-5-21-...")
fn current_user_sid_string() -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
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
