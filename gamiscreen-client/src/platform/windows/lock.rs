use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use tracing::{debug, error, info, trace, warn};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::HMENU;

use super::util::to_wide_null;
use crate::AppError;

/// Locks the current workstation immediately.
pub async fn lock_now() -> Result<(), AppError> {
    info!("Windows: lock requested via LockWorkStation");
    let ok = unsafe { windows_sys::Win32::System::Shutdown::LockWorkStation() };
    if ok == 0 {
        let err = std::io::Error::last_os_error();
        error!(os_error = err.raw_os_error(), %err, "LockWorkStation failed");
        Err(AppError::Io(err))
    } else {
        info!("Windows: workstation lock succeeded");
        Ok(())
    }
}

/// Event-based lock state via WM_WTSSESSION_CHANGE notifications.
pub async fn is_session_locked() -> Result<bool, AppError> {
    ensure_session_watcher();
    let state = SESSION_LOCKED.get().expect("watcher init");
    let locked = state.load(Ordering::Acquire);
    trace!(locked, "Windows: queried session lock state");
    Ok(locked)
}

static SESSION_LOCKED: OnceLock<Arc<AtomicBool>> = OnceLock::new();
static WATCHER_THREAD: OnceLock<std::thread::JoinHandle<()>> = OnceLock::new();

fn ensure_session_watcher() {
    // Use get_or_init to avoid TOCTOU race where two threads could both spawn
    // a watcher and leak an Arc (review finding: HIGH race condition).
    SESSION_LOCKED.get_or_init(|| {
        let state = Arc::new(AtomicBool::new(false));
        let state_clone = Arc::clone(&state);
        info!("Windows: starting session watcher thread");
        let handle = std::thread::spawn(move || run_session_watcher(state_clone));
        if WATCHER_THREAD.set(handle).is_err() {
            warn!("Windows: session watcher thread handle already set unexpectedly");
        }
        state
    });
}

extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use windows_sys::Win32::System::RemoteDesktop::WTSGetActiveConsoleSessionId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, GWLP_USERDATA, GetWindowLongPtrW,
    };
    const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
    const WTS_SESSION_LOCK: usize = 0x7;
    const WTS_SESSION_UNLOCK: usize = 0x8;
    if msg == WM_WTSSESSION_CHANGE {
        // SAFETY: The GWLP_USERDATA pointer was set to Arc::into_raw(state) in
        // run_session_watcher (line ~157). The Arc is intentionally leaked at the end
        // of run_session_watcher (line ~204) to guarantee this pointer remains valid
        // for the lifetime of the message loop. The null check below guards against
        // the window receiving messages before SetWindowLongPtrW completes.
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const AtomicBool;
        if !ptr.is_null() {
            // Only react to events coming from the active console session
            let event_session_id: u32 = lparam as u32;
            let active_id = unsafe { WTSGetActiveConsoleSessionId() };
            if active_id == u32::MAX {
                // No active console session; ignore but log
                trace!(
                    event_session_id,
                    "WTS: no active console session; ignoring event"
                );
                return 0;
            }
            if event_session_id != active_id {
                trace!(
                    event_session_id,
                    active_id, "WTS: event for other session; ignoring"
                );
                return 0;
            }
            let locked = match wparam {
                WTS_SESSION_LOCK => true,
                WTS_SESSION_UNLOCK => false,
                _ => return 0,
            };
            // SAFETY: `ptr` is a valid Arc::into_raw pointer (see SAFETY comment above).
            // AtomicBool::store is lock-free and safe for concurrent access.
            unsafe { (*ptr).store(locked, Ordering::Release) };
            debug!(
                locked,
                session_id = active_id,
                "WTS: session lock state updated"
            );
            return 0;
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn run_session_watcher(state: Arc<AtomicBool>) {
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::HINSTANCE;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::RemoteDesktop::{
        NOTIFY_FOR_ALL_SESSIONS, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DispatchMessageW, GWLP_USERDATA,
        GetMessageW, GetWindowLongPtrW, MSG, RegisterClassW, SetWindowLongPtrW, TranslateMessage,
        WNDCLASSW, WS_OVERLAPPEDWINDOW,
    };

    unsafe {
        let hinstance: HINSTANCE = GetModuleHandleW(null_mut());
        if hinstance.is_null() {
            warn!("Windows: GetModuleHandleW returned NULL; session watcher not started");
            return;
        }
        let class_name = to_wide_null("GamiScreenSessWnd");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: null_mut(),
            lpszClassName: class_name.as_ptr(),
        };
        if RegisterClassW(&wc) == 0 {
            warn!("Windows: RegisterClassW failed; session watcher not started");
            return;
        }
        info!("Windows: session watcher window class registered");
        let hwnd: HWND = CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            0 as HWND,
            0 as HMENU,
            hinstance,
            null_mut(),
        );
        if hwnd.is_null() {
            warn!("Windows: CreateWindowExW failed; session watcher not started");
            return;
        }
        info!(?hwnd, "Windows: session watcher window created");
        // Stash pointer to AtomicBool in window user data for use in wnd_proc.
        // Check SetWindowLongPtrW return: 0 with non-zero GetLastError means failure.
        let raw_ptr = Arc::into_raw(state);
        let prev = SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw_ptr as isize);
        if prev == 0 {
            let err_code = windows_sys::Win32::Foundation::GetLastError();
            if err_code != 0 {
                warn!(
                    os_error = err_code,
                    "SetWindowLongPtrW failed; reclaiming Arc and destroying window"
                );
                // Reclaim the Arc to avoid a leak
                let _ = Arc::from_raw(raw_ptr);
                windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
                return;
            }
        }
        // Register for session change notifications for all sessions
        let ok = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_ALL_SESSIONS);
        if ok == 0 {
            warn!("Windows: WTSRegisterSessionNotification failed; will not receive lock events");
        } else {
            info!("Windows: session watcher registered for WTS notifications");
        }

        // Message loop
        let mut msg: MSG = core::mem::zeroed();
        info!("Windows: session watcher message loop running");
        loop {
            let r = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
            if r > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else if r == 0 {
                debug!("Windows: GetMessageW received WM_QUIT; exiting watcher");
                break;
            } else {
                warn!("Windows: GetMessageW failed; exiting watcher");
                break;
            }
        }

        // Unregister session notifications before destroying the window
        WTSUnRegisterSessionNotification(hwnd);

        // Destroy the window to stop further message delivery to wnd_proc
        // before cleaning up the user data pointer.
        windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);

        // Intentionally leak the Arc<AtomicBool>. The session watcher is
        // process-lifetime and runs until the service exits, at which point
        // the OS reclaims all memory. Reclaiming here would risk a
        // use-after-free if a stale message arrives between DestroyWindow
        // returning and Arc::from_raw completing.

        info!("Windows: session watcher thread exiting");
    }
}
