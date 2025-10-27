use std::collections::HashMap;
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};
use windows_sys::Win32::Foundation::{GetLastError, NO_ERROR};
use windows_sys::Win32::System::RemoteDesktop::WTSSESSION_NOTIFICATION;
use windows_sys::Win32::System::Services::{
    RegisterServiceCtrlHandlerExW, SERVICE_ACCEPT_SESSIONCHANGE, SERVICE_ACCEPT_SHUTDOWN,
    SERVICE_ACCEPT_STOP, SERVICE_CONTROL_INTERROGATE, SERVICE_CONTROL_SESSIONCHANGE,
    SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_RUNNING, SERVICE_START_PENDING,
    SERVICE_STATUS, SERVICE_STATUS_HANDLE, SERVICE_STOP_PENDING, SERVICE_STOPPED,
    SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS, SetServiceStatus, StartServiceCtrlDispatcherW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    WTS_CONSOLE_CONNECT, WTS_CONSOLE_DISCONNECT, WTS_REMOTE_CONNECT, WTS_REMOTE_DISCONNECT,
    WTS_SESSION_LOCK, WTS_SESSION_LOGOFF, WTS_SESSION_LOGON, WTS_SESSION_UNLOCK,
};

use crate::AppError;

const SERVICE_NAME: &str = "GamiScreenAgent";
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

    unsafe {
        Arc::from_raw(ctx_ptr as *const ServiceContext);
    }
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
    let ctx = &*(context as *const ServiceContext);

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
            let notification = *(event_data as *const WTSSESSION_NOTIFICATION);
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
        WTS_SESSION_UNLOCK => Some(SessionEventKind::Activate(SessionActivateReason::Unlock)),
        WTS_CONSOLE_CONNECT => Some(SessionEventKind::Activate(
            SessionActivateReason::ConsoleConnect,
        )),
        WTS_REMOTE_CONNECT => Some(SessionEventKind::Activate(
            SessionActivateReason::RemoteConnect,
        )),
        WTS_SESSION_LOGOFF => Some(SessionEventKind::Deactivate(
            SessionDeactivateReason::Logoff,
        )),
        WTS_SESSION_LOCK => Some(SessionEventKind::Deactivate(SessionDeactivateReason::Lock)),
        WTS_CONSOLE_DISCONNECT => Some(SessionEventKind::Deactivate(
            SessionDeactivateReason::ConsoleDisconnect,
        )),
        WTS_REMOTE_DISCONNECT => Some(SessionEventKind::Deactivate(
            SessionDeactivateReason::RemoteDisconnect,
        )),
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
        self.handle.store(handle as *mut c_void, Ordering::Release);
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
                if !handle.is_null() {
                    if SetServiceStatus(handle as SERVICE_STATUS_HANDLE, &mut *status) == 0 {
                        let err = last_error();
                        warn!(error=%err, "SetServiceStatus failed");
                    }
                }
            }
        }
    }

    fn resend_status(&self) {
        if let Ok(mut status) = self.status.lock() {
            unsafe {
                let handle = self.handle.load(Ordering::Acquire);
                if !handle.is_null() {
                    if SetServiceStatus(handle as SERVICE_STATUS_HANDLE, &mut *status) == 0 {
                        let err = last_error();
                        warn!(error=%err, "SetServiceStatus failed while interrogating");
                    }
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

    async fn run(mut self, mut rx: mpsc::Receiver<ServiceEvent>) {
        info!("session supervisor event loop started");
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
    info!(session_id, generation, "session worker started");
    let mut ticker = tokio::time::interval(Duration::from_secs(60));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!(session_id, generation, "session worker stopped (requested)");
                return WorkerExitKind::Requested;
            }
            _ = ticker.tick() => {
                debug!(session_id, generation, "session worker heartbeat");
            }
        }
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
    Unlock,
    ConsoleConnect,
    RemoteConnect,
}

#[derive(Clone, Copy)]
enum SessionDeactivateReason {
    Logoff,
    Lock,
    ConsoleDisconnect,
    RemoteDisconnect,
}

impl SessionActivateReason {
    fn as_str(&self) -> &'static str {
        match self {
            SessionActivateReason::Logon => "logon",
            SessionActivateReason::Unlock => "unlock",
            SessionActivateReason::ConsoleConnect => "console-connect",
            SessionActivateReason::RemoteConnect => "remote-connect",
        }
    }
}

impl SessionDeactivateReason {
    fn as_str(&self) -> &'static str {
        match self {
            SessionDeactivateReason::Logoff => "logoff",
            SessionDeactivateReason::Lock => "lock",
            SessionDeactivateReason::ConsoleDisconnect => "console-disconnect",
            SessionDeactivateReason::RemoteDisconnect => "remote-disconnect",
        }
    }
}

fn to_wide_null(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn last_error() -> std::io::Error {
    std::io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
}
