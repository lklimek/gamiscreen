use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};
use windows_sys::Win32::Foundation::NO_ERROR;
use windows_sys::Win32::System::RemoteDesktop::{
    WTS_SESSION_INFOW, WTSActive, WTSEnumerateSessionsW, WTSFreeMemory, WTSSESSION_NOTIFICATION,
};
use windows_sys::Win32::System::Services::{
    RegisterServiceCtrlHandlerExW, SERVICE_ACCEPT_SESSIONCHANGE, SERVICE_ACCEPT_SHUTDOWN,
    SERVICE_ACCEPT_STOP, SERVICE_CONTROL_INTERROGATE, SERVICE_CONTROL_SESSIONCHANGE,
    SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_RUNNING, SERVICE_START_PENDING,
    SERVICE_STATUS, SERVICE_STATUS_HANDLE, SERVICE_STOP_PENDING, SERVICE_STOPPED,
    SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS, SetServiceStatus, StartServiceCtrlDispatcherW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    WTS_CONSOLE_CONNECT, WTS_CONSOLE_DISCONNECT, WTS_REMOTE_CONNECT, WTS_REMOTE_DISCONNECT,
    WTS_SESSION_LOGOFF, WTS_SESSION_LOGON,
};

use super::util::{SERVICE_NAME, last_error, to_wide_null};
use crate::AppError;

const SERVICE_DISPLAY_WAIT_HINT_MS: u32 = 5_000;

pub fn run_service_host() -> Result<(), AppError> {
    let service_name_w = to_wide_null(SERVICE_NAME);
    let mut table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: service_name_w.as_ptr() as *mut u16,
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: std::ptr::null_mut(),
            lpServiceProc: None,
        },
    ];

    unsafe {
        info!(
            service = SERVICE_NAME,
            "starting Windows service control dispatcher"
        );
        let result = StartServiceCtrlDispatcherW(table.as_mut_ptr());
        if result == 0 {
            let err = last_error();
            error!(service = SERVICE_NAME, os_error = err.raw_os_error(), %err, "StartServiceCtrlDispatcherW failed");
            return Err(AppError::Io(err));
        }
    }
    info!(service = SERVICE_NAME, "service control dispatcher exited");
    Ok(())
}

unsafe extern "system" fn service_main(_argc: u32, _argv: *mut *mut u16) {
    if let Err(err) = service_main_impl() {
        error!(error=%err, "service main failed");
    }
}

fn service_main_impl() -> Result<(), AppError> {
    info!(service = SERVICE_NAME, "service main entry");
    let (tx, rx) = mpsc::channel(32);
    let ctx = Arc::new(ServiceContext::new(tx.clone()));
    let ctx_ptr = Arc::into_raw(ctx.clone()) as *mut c_void;
    let service_name_w = to_wide_null(SERVICE_NAME);

    let handle = unsafe {
        RegisterServiceCtrlHandlerExW(
            service_name_w.as_ptr(),
            Some(service_control_handler),
            ctx_ptr,
        )
    };
    if handle.is_null() {
        let err = last_error();
        error!(service = SERVICE_NAME, os_error = err.raw_os_error(), %err, "RegisterServiceCtrlHandlerExW failed");
        unsafe {
            Arc::from_raw(ctx_ptr as *const ServiceContext);
        }
        return Err(AppError::Io(err));
    }
    ctx.set_handle(handle);
    debug!(service = SERVICE_NAME, "registered service control handler");
    ctx.update_status(|status| {
        status.dwCurrentState = SERVICE_START_PENDING;
        status.dwWaitHint = SERVICE_DISPLAY_WAIT_HINT_MS;
        status.dwControlsAccepted = 0;
    });

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(AppError::Io)?;
    debug!(
        service = SERVICE_NAME,
        "Tokio runtime initialised for service"
    );

    ctx.update_status(|status| {
        status.dwCurrentState = SERVICE_RUNNING;
        status.dwControlsAccepted =
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN | SERVICE_ACCEPT_SESSIONCHANGE;
        status.dwWaitHint = 0;
        status.dwCheckPoint = 0;
    });
    info!(
        service = SERVICE_NAME,
        "service state transitioned to RUNNING"
    );

    runtime.block_on(async move {
        info!(service = SERVICE_NAME, "session supervisor starting");
        let supervisor = SessionSupervisor::new(tx);
        supervisor.run(rx).await;
        info!(service = SERVICE_NAME, "session supervisor exited");
    });

    ctx.update_status(|status| {
        status.dwCurrentState = SERVICE_STOPPED;
        status.dwControlsAccepted = 0;
        status.dwWaitHint = 0;
        status.dwCheckPoint = 0;
    });
    info!(
        service = SERVICE_NAME,
        "service state transitioned to STOPPED"
    );

    // Intentionally leak the Arc<ServiceContext>: the SCM may dispatch a final
    // control event on another thread after block_on returns. Reclaiming the pointer
    // here would be a use-after-free. The leaked Arc is acceptable for a
    // process-lifetime service context — the OS reclaims memory at process exit.
    Ok(())
}

unsafe extern "system" fn service_control_handler(
    control: u32,
    event_type: u32,
    event_data: *mut c_void,
    context: *mut c_void,
) -> u32 {
    if context.is_null() {
        return NO_ERROR;
    }
    // SAFETY: `context` was set to an `Arc::into_raw(ctx)` pointer in `service_main_impl`
    // and is guaranteed non-null (checked above) and valid for the service lifetime.
    let ctx = unsafe { &*(context as *const ServiceContext) };

    trace!(control, event_type, "service control handler invoked");

    match control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            info!("service stop requested");
            ctx.update_status(|status| {
                status.dwCurrentState = SERVICE_STOP_PENDING;
                status.dwControlsAccepted = 0;
                status.dwWaitHint = SERVICE_DISPLAY_WAIT_HINT_MS;
            });
            if let Err(err) = ctx.try_send(ServiceEvent::Stop) {
                warn!(error=%err, "failed to queue stop event");
            }
        }
        SERVICE_CONTROL_SESSIONCHANGE => {
            if event_data.is_null() {
                return NO_ERROR;
            }
            // SAFETY: SCM guarantees `event_data` points to a valid WTSSESSION_NOTIFICATION
            // for SERVICE_CONTROL_SESSIONCHANGE events. Null check is above.
            let notification = unsafe { *(event_data as *const WTSSESSION_NOTIFICATION) };
            let session_id = notification.dwSessionId;
            trace!(event_type, session_id, "received session change event");
            if let Some(kind) = map_session_event(event_type) {
                let evt = SessionEvent { session_id, kind };
                if let Err(err) = ctx.try_send(ServiceEvent::Session(evt)) {
                    warn!(error=%err, session_id, "failed to queue session event");
                }
            } else {
                debug!(event_type, session_id, "ignored session event");
            }
        }
        SERVICE_CONTROL_INTERROGATE => {
            ctx.resend_status();
        }
        _ => {}
    }

    NO_ERROR
}

fn map_session_event(event_type: u32) -> Option<SessionEventKind> {
    match event_type {
        WTS_SESSION_LOGON => Some(SessionEventKind::Activate(SessionActivateReason::Logon)),
        WTS_CONSOLE_CONNECT => Some(SessionEventKind::Activate(
            SessionActivateReason::ConsoleConnect,
        )),
        WTS_REMOTE_CONNECT => Some(SessionEventKind::Activate(
            SessionActivateReason::RemoteConnect,
        )),
        WTS_SESSION_LOGOFF => Some(SessionEventKind::Deactivate(
            SessionDeactivateReason::Logoff,
        )),
        WTS_CONSOLE_DISCONNECT => Some(SessionEventKind::Deactivate(
            SessionDeactivateReason::ConsoleDisconnect,
        )),
        WTS_REMOTE_DISCONNECT => Some(SessionEventKind::Deactivate(
            SessionDeactivateReason::RemoteDisconnect,
        )),
        // Lock/Unlock: the session agent detects lock state internally via
        // is_session_locked() and skips heartbeats accordingly.
        _ => None,
    }
}

struct ServiceContext {
    handle: AtomicPtr<c_void>,
    status: Mutex<SERVICE_STATUS>,
    tx: mpsc::Sender<ServiceEvent>,
}

impl ServiceContext {
    fn new(tx: mpsc::Sender<ServiceEvent>) -> Self {
        let status = SERVICE_STATUS {
            dwServiceType: SERVICE_WIN32_OWN_PROCESS,
            dwCurrentState: SERVICE_START_PENDING,
            dwControlsAccepted: 0,
            dwWin32ExitCode: 0,
            dwServiceSpecificExitCode: 0,
            dwCheckPoint: 0,
            dwWaitHint: SERVICE_DISPLAY_WAIT_HINT_MS,
        };
        Self {
            handle: AtomicPtr::new(std::ptr::null_mut()),
            status: Mutex::new(status),
            tx,
        }
    }

    fn set_handle(&self, handle: SERVICE_STATUS_HANDLE) {
        self.handle.store(handle, Ordering::Release);
    }

    fn update_status<F>(&self, update: F)
    where
        F: FnOnce(&mut SERVICE_STATUS),
    {
        if let Ok(mut status) = self.status.lock() {
            update(&mut status);
            if status.dwCurrentState == SERVICE_RUNNING || status.dwCurrentState == SERVICE_STOPPED
            {
                status.dwCheckPoint = 0;
            } else {
                status.dwCheckPoint = status.dwCheckPoint.saturating_add(1);
            }
            unsafe {
                let handle = self.handle.load(Ordering::Acquire);
                if !handle.is_null()
                    && SetServiceStatus(handle as SERVICE_STATUS_HANDLE, &*status) == 0
                {
                    let err = last_error();
                    warn!(error=%err, "SetServiceStatus failed");
                }
            }
        }
    }

    fn resend_status(&self) {
        if let Ok(status) = self.status.lock() {
            unsafe {
                let handle = self.handle.load(Ordering::Acquire);
                if !handle.is_null()
                    && SetServiceStatus(handle as SERVICE_STATUS_HANDLE, &*status) == 0
                {
                    let err = last_error();
                    warn!(error=%err, "SetServiceStatus failed while interrogating");
                }
            }
        }
    }

    fn try_send(&self, event: ServiceEvent) -> Result<(), mpsc::error::TrySendError<ServiceEvent>> {
        self.tx.try_send(event)
    }
}

struct SessionSupervisor {
    sender: mpsc::Sender<ServiceEvent>,
    workers: HashMap<u32, SessionWorker>,
    joinset: JoinSet<WorkerExit>,
    stopping: bool,
}

impl SessionSupervisor {
    fn new(sender: mpsc::Sender<ServiceEvent>) -> Self {
        Self {
            sender,
            workers: HashMap::new(),
            joinset: JoinSet::new(),
            stopping: false,
        }
    }

    fn enumerate_existing_sessions(&mut self) {
        unsafe {
            let mut session_info: *mut WTS_SESSION_INFOW = std::ptr::null_mut();
            let mut count: u32 = 0;
            let result = WTSEnumerateSessionsW(
                std::ptr::null_mut(), // WTS_CURRENT_SERVER_HANDLE
                0,
                1,
                &mut session_info,
                &mut count,
            );
            if result == 0 {
                warn!("WTSEnumerateSessionsW failed; skipping existing session enumeration");
                return;
            }
            // RAII guard for WTSFreeMemory
            struct WtsGuard(*mut WTS_SESSION_INFOW);
            impl Drop for WtsGuard {
                fn drop(&mut self) {
                    unsafe {
                        WTSFreeMemory(self.0 as *mut c_void);
                    }
                }
            }
            let _guard = WtsGuard(session_info);
            let sessions = std::slice::from_raw_parts(session_info, count as usize);
            for session in sessions {
                if session.SessionId == 0 {
                    continue;
                } // skip services session
                if session.State == WTSActive {
                    info!(
                        session_id = session.SessionId,
                        "found existing active session at startup"
                    );
                    self.activate_session(session.SessionId);
                }
            }
        }
    }

    async fn run(mut self, mut rx: mpsc::Receiver<ServiceEvent>) {
        info!("session supervisor event loop started");
        self.enumerate_existing_sessions();
        loop {
            if self.stopping && self.joinset.is_empty() && self.workers.is_empty() {
                break;
            }

            tokio::select! {
                Some(event) = rx.recv() => {
                    self.handle_event(event).await;
                }
                Some(result) = self.joinset.join_next() => {
                    self.handle_exit(result);
                }
                else => break,
            }
        }

        self.cancel_all();
        while let Some(result) = self.joinset.join_next().await {
            self.handle_exit(result);
        }
        info!("session supervisor event loop finished");
    }

    async fn handle_event(&mut self, event: ServiceEvent) {
        match event {
            ServiceEvent::Stop => {
                if !self.stopping {
                    self.stopping = true;
                    info!("stop event received; cancelling workers");
                    self.cancel_all();
                }
            }
            ServiceEvent::Session(evt) => match evt.kind {
                SessionEventKind::Activate(reason) => {
                    debug!(
                        session_id = evt.session_id,
                        reason = reason.as_str(),
                        "session activated"
                    );
                    self.activate_session(evt.session_id);
                }
                SessionEventKind::Deactivate(reason) => {
                    debug!(
                        session_id = evt.session_id,
                        reason = reason.as_str(),
                        "session deactivated"
                    );
                    self.deactivate_session(evt.session_id);
                }
            },
            ServiceEvent::RestartSession { session_id } => {
                if self.stopping {
                    return;
                }
                debug!(session_id, "restarting session worker after backoff");
                self.activate_session(session_id);
            }
        }
    }

    fn activate_session(&mut self, session_id: u32) {
        if self.stopping {
            return;
        }
        let entry = self
            .workers
            .entry(session_id)
            .or_insert_with(|| SessionWorker::new(session_id));
        if entry.active {
            return;
        }
        info!(session_id, "starting session worker");
        entry.start(&mut self.joinset);
    }

    fn deactivate_session(&mut self, session_id: u32) {
        if let Some(worker) = self.workers.get_mut(&session_id) {
            info!(
                session_id,
                "cancelling session worker due to deactivate event"
            );
            worker.remove_on_exit = true;
            worker.cancel();
        }
    }

    fn cancel_all(&mut self) {
        debug!("cancelling all session workers");
        for worker in self.workers.values_mut() {
            worker.remove_on_exit = true;
            worker.cancel();
        }
    }

    fn handle_exit(&mut self, result: Result<WorkerExit, tokio::task::JoinError>) {
        match result {
            Ok(exit) => {
                let session_id = exit.session_id;
                let generation = exit.generation;
                let kind = exit.kind;
                if let Some(worker) = self.workers.get_mut(&session_id) {
                    if worker.generation != generation {
                        trace!(
                            session_id,
                            exit_generation = generation,
                            worker_generation = worker.generation,
                            "ignoring stale worker exit"
                        );
                        return;
                    }
                    worker.active = false;
                    match kind {
                        WorkerExitKind::Requested => {
                            debug!(session_id, generation, "session worker stopped on request");
                            worker.backoff.reset();
                            if worker.remove_on_exit {
                                self.workers.remove(&session_id);
                            } else {
                                worker.remove_on_exit = false;
                            }
                        }
                        WorkerExitKind::Completed => {
                            debug!(session_id, generation, "session worker completed normally");
                            worker.backoff.reset();
                            if worker.remove_on_exit {
                                self.workers.remove(&session_id);
                            }
                        }
                        WorkerExitKind::Failed(reason) => {
                            warn!(session_id, %reason, "session worker failed");
                            if worker.remove_on_exit || self.stopping {
                                self.workers.remove(&session_id);
                            } else {
                                let delay = worker.backoff.next_delay();
                                let attempt = worker.backoff.attempt;
                                info!(
                                    session_id,
                                    generation,
                                    delay_secs = delay.as_secs(),
                                    attempt,
                                    "scheduling session worker restart"
                                );
                                let tx = self.sender.clone();
                                tokio::spawn(async move {
                                    tokio::time::sleep(delay).await;
                                    let _ =
                                        tx.send(ServiceEvent::RestartSession { session_id }).await;
                                });
                            }
                        }
                    }
                }
            }
            Err(err) => {
                warn!(error=%err, "session worker panicked");
            }
        }
    }
}

struct SessionWorker {
    session_id: u32,
    cancel_token: CancellationToken,
    generation: u64,
    backoff: Backoff,
    pub active: bool,
    pub remove_on_exit: bool,
}

impl SessionWorker {
    fn new(session_id: u32) -> Self {
        Self {
            session_id,
            cancel_token: CancellationToken::new(),
            generation: 0,
            backoff: Backoff::default(),
            active: false,
            remove_on_exit: false,
        }
    }

    fn start(&mut self, joinset: &mut JoinSet<WorkerExit>) {
        self.generation = self.generation.wrapping_add(1);
        self.cancel_token = CancellationToken::new();
        let cancel = self.cancel_token.clone();
        let session_id = self.session_id;
        let generation = self.generation;
        joinset.spawn(async move {
            let kind = session_worker_task(session_id, generation, cancel).await;
            WorkerExit {
                session_id,
                generation,
                kind,
            }
        });
        self.active = true;
        self.remove_on_exit = false;
        info!(session_id, generation, "session worker task spawned");
    }

    fn cancel(&self) {
        debug!(session_id = self.session_id, "cancelling session worker");
        self.cancel_token.cancel();
    }
}

#[derive(Default)]
struct Backoff {
    attempt: u32,
}

impl Backoff {
    fn reset(&mut self) {
        self.attempt = 0;
    }

    fn next_delay(&mut self) -> Duration {
        let delay_secs = match self.attempt {
            0 => 1,
            1 => 5,
            2 => 15,
            3 => 30,
            _ => 60,
        };
        self.attempt = self.attempt.saturating_add(1);
        Duration::from_secs(delay_secs as u64)
    }
}

struct WorkerExit {
    session_id: u32,
    generation: u64,
    kind: WorkerExitKind,
}

enum WorkerExitKind {
    Requested,
    Completed,
    Failed(String),
}

async fn session_worker_task(
    session_id: u32,
    generation: u64,
    cancel: CancellationToken,
) -> WorkerExitKind {
    info!(
        session_id,
        generation, "session worker starting child process"
    );

    let child = match spawn_session_agent(session_id) {
        Ok(child) => child,
        Err(e) => {
            error!(session_id, generation, error=%e, "failed to spawn session agent");
            return WorkerExitKind::Failed(e.to_string());
        }
    };

    info!(
        session_id,
        generation,
        pid = child.pid,
        "session agent process spawned"
    );

    // Cast handles to usize so they are Send-safe for use across await points.
    let proc_h = child.process_handle.0 as usize;
    let thread_h = child.thread_handle.0 as usize;

    // Spawn the blocking wait as a JoinHandle so we can await it on both paths.
    let wait_handle = tokio::task::spawn_blocking(move || unsafe {
        let h = proc_h as windows_sys::Win32::Foundation::HANDLE;
        const INFINITE: u32 = 0xFFFFFFFF;
        const WAIT_FAILED_VAL: u32 = 0xFFFFFFFF;

        let wait_result = windows_sys::Win32::System::Threading::WaitForSingleObject(h, INFINITE);
        if wait_result == WAIT_FAILED_VAL {
            return Err(last_error());
        }

        let mut exit_code: u32 = 0;
        if windows_sys::Win32::System::Threading::GetExitCodeProcess(h, &mut exit_code) == 0 {
            return Err(last_error());
        }
        Ok(exit_code)
    });

    // Track whether the blocking wait thread has been joined, so we know
    // it's safe to close the handles (the thread also uses proc_h).
    let mut handles_safe_to_close = true;

    let result = tokio::select! {
        _ = cancel.cancelled() => {
            info!(session_id, generation, "cancel requested; terminating session agent");
            unsafe {
                let h = proc_h as windows_sys::Win32::Foundation::HANDLE;
                if windows_sys::Win32::System::Threading::TerminateProcess(h, 1) == 0 {
                    let err = last_error();
                    warn!(session_id, error=%err, "TerminateProcess failed");
                }
                let wait_ret = windows_sys::Win32::System::Threading::WaitForSingleObject(h, 5000);
                const WAIT_TIMEOUT: u32 = 0x00000102;
                const WAIT_FAILED_VAL: u32 = 0xFFFFFFFF;
                match wait_ret {
                    WAIT_TIMEOUT => warn!(session_id, "timed out waiting for terminated process to exit"),
                    WAIT_FAILED_VAL => {
                        let err = last_error();
                        warn!(session_id, error=%err, "WaitForSingleObject failed after TerminateProcess");
                    }
                    _ => {}
                }
            }
            // Await the blocking thread with a timeout to avoid hanging shutdown
            // indefinitely if TerminateProcess failed and the process won't exit.
            // SAFETY: If the timeout fires, the blocking thread may still be using
            // proc_h, so we must NOT close handles — they will leak instead.
            match tokio::time::timeout(Duration::from_secs(10), wait_handle).await {
                Ok(_) => {}
                Err(_) => {
                    warn!(session_id, "timed out waiting for blocking wait thread after termination; handles will leak");
                    handles_safe_to_close = false;
                }
            }
            WorkerExitKind::Requested
        }
        join_result = wait_handle => {
            match join_result {
                Ok(Ok(0)) => {
                    info!(session_id, generation, "session agent exited cleanly");
                    WorkerExitKind::Completed
                }
                Ok(Ok(code)) => {
                    warn!(session_id, generation, exit_code = code, "session agent exited with error");
                    WorkerExitKind::Failed(format!("exit code {code}"))
                }
                Ok(Err(e)) => {
                    error!(session_id, generation, error=%e, "failed to wait for session agent");
                    WorkerExitKind::Failed(e.to_string())
                }
                Err(e) => {
                    error!(session_id, generation, error=%e, "wait task panicked");
                    WorkerExitKind::Failed(e.to_string())
                }
            }
        }
    };

    if handles_safe_to_close {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(
                proc_h as windows_sys::Win32::Foundation::HANDLE,
            );
            windows_sys::Win32::Foundation::CloseHandle(
                thread_h as windows_sys::Win32::Foundation::HANDLE,
            );
        }
    }

    result
}

/// Wrapper to make a Win32 HANDLE sendable across threads.
/// SAFETY: Win32 handles are safe to send between threads; the OS manages synchronization.
// TODO: Add Drop impl to auto-close the handle and prevent leaks on refactoring.
// Currently handles are manually closed in session_worker_task.
struct SendHandle(windows_sys::Win32::Foundation::HANDLE);
unsafe impl Send for SendHandle {}

struct ChildProcess {
    process_handle: SendHandle,
    thread_handle: SendHandle,
    pid: u32,
}

/// RAII guard for tokens and environment block allocated during process spawning.
struct SpawnResources {
    /// Primary token (duplicated from the impersonation token via DuplicateTokenEx)
    primary_token: windows_sys::Win32::Foundation::HANDLE,
    env_block: *mut std::ffi::c_void,
}

impl Drop for SpawnResources {
    fn drop(&mut self) {
        unsafe {
            if !self.env_block.is_null() {
                windows_sys::Win32::System::Environment::DestroyEnvironmentBlock(self.env_block);
            }
            if !self.primary_token.is_null() {
                windows_sys::Win32::Foundation::CloseHandle(self.primary_token);
            }
        }
    }
}

fn spawn_session_agent(session_id: u32) -> Result<ChildProcess, std::io::Error> {
    use windows_sys::Win32::Security::{
        DuplicateTokenEx, SecurityImpersonation, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE,
        TOKEN_QUERY, TokenPrimary,
    };
    use windows_sys::Win32::System::Environment::CreateEnvironmentBlock;
    use windows_sys::Win32::System::RemoteDesktop::WTSQueryUserToken;
    use windows_sys::Win32::System::Threading::{
        CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessAsUserW, PROCESS_INFORMATION,
        STARTUPINFOW,
    };

    unsafe {
        // WTSQueryUserToken returns an impersonation token; we must convert it
        // to a primary token via DuplicateTokenEx for CreateProcessAsUserW.
        let mut impersonation_token: windows_sys::Win32::Foundation::HANDLE = std::ptr::null_mut();
        if WTSQueryUserToken(session_id, &mut impersonation_token) == 0 {
            return Err(last_error());
        }

        let mut primary_token: windows_sys::Win32::Foundation::HANDLE = std::ptr::null_mut();
        let dup_result = DuplicateTokenEx(
            impersonation_token,
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY,
            std::ptr::null(),
            SecurityImpersonation,
            TokenPrimary,
            &mut primary_token,
        );
        windows_sys::Win32::Foundation::CloseHandle(impersonation_token);
        if dup_result == 0 {
            return Err(last_error());
        }

        let mut env_block: *mut std::ffi::c_void = std::ptr::null_mut();
        if CreateEnvironmentBlock(&mut env_block, primary_token, 0) == 0 {
            let err = last_error();
            windows_sys::Win32::Foundation::CloseHandle(primary_token);
            return Err(err);
        }

        let resources = SpawnResources {
            primary_token,
            env_block,
        };

        let exe_path = std::env::current_exe()?;
        let cmd = format!(
            "\"{}\" session-agent --session-id {}",
            exe_path.display(),
            session_id
        );
        let mut cmd_w = to_wide_null(&cmd);

        let si: STARTUPINFOW = {
            let mut si: STARTUPINFOW = std::mem::zeroed();
            si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            si
        };
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();

        // SECURITY: The agent runs as the child user. A technically inclined child
        // could terminate it via Task Manager. The supervisor restarts it with
        // exponential backoff (max 60s). Future hardening:
        // TODO: Set a restrictive DACL on the process handle to deny PROCESS_TERMINATE
        // TODO: Alert the parent when repeated agent failures are detected
        let create_result = CreateProcessAsUserW(
            resources.primary_token,
            std::ptr::null(),   // application name (use command line)
            cmd_w.as_mut_ptr(), // command line
            std::ptr::null(),   // process security attributes
            std::ptr::null(),   // thread security attributes
            0,                  // don't inherit handles
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            resources.env_block, // environment
            std::ptr::null(),    // current directory (inherit)
            &si,
            &mut pi,
        );

        drop(resources);

        if create_result == 0 {
            return Err(last_error());
        }

        Ok(ChildProcess {
            process_handle: SendHandle(pi.hProcess),
            thread_handle: SendHandle(pi.hThread),
            pid: pi.dwProcessId,
        })
    }
}

enum ServiceEvent {
    Stop,
    Session(SessionEvent),
    RestartSession { session_id: u32 },
}

#[derive(Clone)]
struct SessionEvent {
    session_id: u32,
    kind: SessionEventKind,
}

#[derive(Clone, Copy)]
enum SessionEventKind {
    Activate(SessionActivateReason),
    Deactivate(SessionDeactivateReason),
}

#[derive(Clone, Copy)]
enum SessionActivateReason {
    Logon,
    ConsoleConnect,
    RemoteConnect,
}

#[derive(Clone, Copy)]
enum SessionDeactivateReason {
    Logoff,
    ConsoleDisconnect,
    RemoteDisconnect,
}

impl SessionActivateReason {
    fn as_str(&self) -> &'static str {
        match self {
            SessionActivateReason::Logon => "logon",
            SessionActivateReason::ConsoleConnect => "console-connect",
            SessionActivateReason::RemoteConnect => "remote-connect",
        }
    }
}

impl SessionDeactivateReason {
    fn as_str(&self) -> &'static str {
        match self {
            SessionDeactivateReason::Logoff => "logoff",
            SessionDeactivateReason::ConsoleDisconnect => "console-disconnect",
            SessionDeactivateReason::RemoteDisconnect => "remote-disconnect",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_delay_sequence() {
        let mut b = Backoff::default();
        assert_eq!(b.next_delay(), Duration::from_secs(1));
        assert_eq!(b.next_delay(), Duration::from_secs(5));
        assert_eq!(b.next_delay(), Duration::from_secs(15));
        assert_eq!(b.next_delay(), Duration::from_secs(30));
        assert_eq!(b.next_delay(), Duration::from_secs(60));
        // caps at 60
        assert_eq!(b.next_delay(), Duration::from_secs(60));
    }

    #[test]
    fn backoff_reset() {
        let mut b = Backoff::default();
        b.next_delay();
        b.next_delay();
        b.reset();
        assert_eq!(b.next_delay(), Duration::from_secs(1));
    }

    #[test]
    fn map_session_event_known_events() {
        assert!(matches!(
            map_session_event(WTS_SESSION_LOGON),
            Some(SessionEventKind::Activate(SessionActivateReason::Logon))
        ));
        assert!(matches!(
            map_session_event(WTS_CONSOLE_CONNECT),
            Some(SessionEventKind::Activate(
                SessionActivateReason::ConsoleConnect
            ))
        ));
        assert!(matches!(
            map_session_event(WTS_SESSION_LOGOFF),
            Some(SessionEventKind::Deactivate(
                SessionDeactivateReason::Logoff
            ))
        ));
    }

    #[test]
    fn map_session_event_unknown_returns_none() {
        assert!(map_session_event(0xFFFF).is_none());
    }
}
