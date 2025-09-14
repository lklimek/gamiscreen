use crate::AppError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use tracing::{debug, warn};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::HMENU;

/// Locks the current workstation immediately.
pub async fn lock_now() -> Result<(), AppError> {
    let ok = unsafe { windows_sys::Win32::System::Shutdown::LockWorkStation() };
    if ok == 0 {
        Err(AppError::Io(std::io::Error::other(
            "LockWorkStation failed",
        )))
    } else {
        Ok(())
    }
}

/// Event-based lock state via WM_WTSSESSION_CHANGE notifications.
pub async fn is_session_locked() -> Result<bool, AppError> {
    ensure_session_watcher();
    let state = SESSION_LOCKED.get().expect("watcher init");
    Ok(state.load(Ordering::Relaxed))
}

static SESSION_LOCKED: OnceLock<Arc<AtomicBool>> = OnceLock::new();
static WATCHER_THREAD: OnceLock<std::thread::JoinHandle<()>> = OnceLock::new();

fn ensure_session_watcher() {
    if SESSION_LOCKED.get().is_some() {
        return;
    }
    let state = Arc::new(AtomicBool::new(false));
    let state_clone = state.clone();
    let handle = std::thread::spawn(move || run_session_watcher(state_clone));
    SESSION_LOCKED.set(state).ok();
    WATCHER_THREAD.set(handle).ok();
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
        // Retrieve Arc<AtomicBool> pointer from user data
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const AtomicBool;
        if !ptr.is_null() {
            // Only react to events coming from the active console session
            let event_session_id: u32 = lparam as u32;
            let active_id = unsafe { WTSGetActiveConsoleSessionId() };
            if active_id == u32::MAX {
                // No active console session; ignore but log
                tracing::trace!(
                    event_session_id,
                    "WTS: no active console session; ignoring event"
                );
                return 0;
            }
            if event_session_id != active_id {
                tracing::trace!(
                    event_session_id,
                    active_id,
                    "WTS: event for other session; ignoring"
                );
                return 0;
            }
            let locked = match wparam {
                WTS_SESSION_LOCK => true,
                WTS_SESSION_UNLOCK => false,
                _ => return 0,
            };
            unsafe { (*ptr).store(locked, Ordering::Relaxed) };
            tracing::debug!(
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
        NOTIFY_FOR_ALL_SESSIONS, WTSRegisterSessionNotification,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, CreateWindowExW, DispatchMessageW, GWLP_USERDATA,
        GetMessageW, MSG, RegisterClassW, SetWindowLongPtrW, TranslateMessage, WNDCLASSW,
        WS_OVERLAPPEDWINDOW,
    };

    unsafe {
        let hinstance: HINSTANCE = GetModuleHandleW(null_mut());
        if hinstance.is_null() {
            warn!("Windows: GetModuleHandleW returned NULL; session watcher not started");
            return;
        }
        let class_name: [u16; 24] = to_wide("GamiScreenSessWnd");
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
        // Stash pointer to AtomicBool in window user data for use in wnd_proc
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Arc::into_raw(state) as isize);
        // Register for session change notifications for all sessions
        let ok = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_ALL_SESSIONS);
        if ok == 0 {
            warn!("Windows: WTSRegisterSessionNotification failed; will not receive lock events");
        }

        // Message loop
        let mut msg: MSG = core::mem::zeroed();
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
    }
}

fn to_wide(s: &str) -> [u16; 24] {
    // Simple helper for our fixed class name. Pad/truncate to 24.
    let mut buf = [0u16; 24];
    let mut i = 0;
    for u in s.encode_utf16() {
        if i >= 23 {
            break;
        }
        buf[i] = u;
        i += 1;
    }
    buf[i] = 0;
    buf
}
