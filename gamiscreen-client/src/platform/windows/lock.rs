use crate::AppError;

/// Locks the current workstation immediately.
pub async fn lock_now() -> Result<(), AppError> {
    let ok = unsafe { windows_sys::Win32::System::Shutdown::LockWorkStation() };
    if ok == 0 {
        Err(AppError::Io(std::io::Error::other("LockWorkStation failed")))
    } else {
        Ok(())
    }
}

/// Detects whether the session is locked by checking the active input desktop.
pub async fn is_session_locked() -> Result<bool, AppError> {
    use tracing::warn;
    use windows_sys::Win32::System::StationsAndDesktops::{
        CloseDesktop, GetUserObjectInformationW, OpenInputDesktop, DESKTOP_READOBJECTS,
        DESKTOP_SWITCHDESKTOP, HDESK, UOI_NAME,
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
        let _ = GetUserObjectInformationW(hdesk, UOI_NAME, std::ptr::null_mut(), 0, &mut needed);
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
        // Treat any non-Default input desktop as locked (e.g., Winlogon)
        let locked = !name.eq_ignore_ascii_case("Default");
        Ok(locked)
    }
}

