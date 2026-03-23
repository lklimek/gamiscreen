use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Foundation::GetLastError;

pub const SERVICE_NAME: &str = "GamiScreenAgent";
pub const SERVICE_DISPLAY_NAME: &str = "GamiScreen Agent";

pub fn to_wide_null(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

pub fn last_error() -> std::io::Error {
    std::io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
}

/// Verify that this process is running in the expected Windows session.
///
/// Returns `Ok(())` if the session matches or the check cannot be performed.
/// Returns `Err` if the process is in a different session than expected.
pub fn verify_session_id(expected: u32) -> Result<(), crate::AppError> {
    let mut actual_session: u32 = 0;
    let pid = std::process::id();
    // SAFETY: ProcessIdToSessionId is a safe FFI call that writes to a valid &mut u32.
    let ok = unsafe {
        windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId(pid, &mut actual_session)
    };
    if ok == 0 {
        let os_error = unsafe { GetLastError() };
        tracing::error!(
            pid,
            os_error,
            "ProcessIdToSessionId failed; refusing to run without verified session"
        );
        return Err(crate::AppError::Io(std::io::Error::from_raw_os_error(
            os_error as i32,
        )));
    }
    if actual_session != expected {
        tracing::error!(
            expected,
            actual = actual_session,
            "session ID mismatch — refusing to run in wrong session"
        );
        return Err(crate::AppError::Config(format!(
            "session ID mismatch: expected {expected}, got {actual_session}"
        )));
    }
    Ok(())
}
