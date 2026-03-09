use std::ffi::c_void;
use std::mem::{MaybeUninit, size_of};

use tracing::{error, info, warn};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TokenElevation};
use windows_sys::Win32::System::Services::{
    ChangeServiceConfig2W, CloseServiceHandle, ControlService, CreateServiceW, DeleteService,
    OpenSCManagerW, OpenServiceW, SC_MANAGER_CONNECT, SC_MANAGER_CREATE_SERVICE,
    SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG, SERVICE_CONFIG_DESCRIPTION, SERVICE_CONTROL_STOP,
    SERVICE_DESCRIPTIONW, SERVICE_ERROR_NORMAL, SERVICE_QUERY_STATUS, SERVICE_START,
    SERVICE_STATUS, SERVICE_STOP, SERVICE_WIN32_OWN_PROCESS, StartServiceW,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, OpenProcessToken, WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};

use super::service;
use super::util::{SERVICE_DISPLAY_NAME, SERVICE_NAME, last_error, to_wide_null};
use crate::AppError;
use crate::cli::ServiceCommand;

const SERVICE_DESCRIPTION: &str = "GamiScreen parental control agent";
/// Standard Win32 DELETE access right (0x00010000).
const DELETE: u32 = 0x0001_0000;

/// RAII wrapper for Win32 SC_HANDLE (F7 fix).
struct ScHandle(*mut c_void);

impl Drop for ScHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                CloseServiceHandle(self.0);
            }
        }
    }
}

/// Returns `true` if the current process has elevated privileges.
fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = MaybeUninit::<windows_sys::Win32::Security::TOKEN_ELEVATION>::uninit();
        let mut ret_len: u32 = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            elevation.as_mut_ptr() as *mut c_void,
            size_of::<windows_sys::Win32::Security::TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );
        CloseHandle(token);
        if ok == 0 {
            return false;
        }
        elevation.assume_init().TokenIsElevated != 0
    }
}

/// Check if elevated; if not, relaunch with UAC and forward exit code.
fn ensure_elevated(args: &[&str]) -> Result<(), AppError> {
    if is_elevated() {
        return Ok(());
    }
    eprintln!("Administrator privileges required. Requesting elevation...");
    relaunch_elevated(args)
}

/// Re-launch with `ShellExecuteExW` + "runas" verb.
/// Uses `SEE_MASK_NOCLOSEPROCESS` to get `hProcess`, waits, forwards exit code.
/// Handles `ERROR_CANCELLED` (1223) gracefully.
fn relaunch_elevated(args: &[&str]) -> Result<(), AppError> {
    let exe_path = std::env::current_exe().map_err(AppError::Io)?;
    let exe_w = to_wide_null(&exe_path.to_string_lossy());
    let params = args.join(" ");
    let params_w = to_wide_null(&params);
    let verb_w = to_wide_null("runas");

    unsafe {
        let mut sei: SHELLEXECUTEINFOW = std::mem::zeroed();
        sei.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        sei.fMask = SEE_MASK_NOCLOSEPROCESS;
        sei.lpVerb = verb_w.as_ptr();
        sei.lpFile = exe_w.as_ptr();
        sei.lpParameters = params_w.as_ptr();
        sei.nShow = 1; // SW_SHOWNORMAL

        if ShellExecuteExW(&mut sei) == 0 {
            let code = GetLastError();
            if code == 1223 {
                // ERROR_CANCELLED: user declined the UAC prompt
                return Err(AppError::Config("elevation cancelled by user".into()));
            }
            let err = std::io::Error::from_raw_os_error(code as i32);
            error!(os_error = err.raw_os_error(), %err, "ShellExecuteExW failed");
            return Err(AppError::Io(err));
        }

        let proc_handle = sei.hProcess;
        if proc_handle.is_null() {
            return Err(AppError::Config("elevated process handle is null".into()));
        }

        const WAIT_FAILED_VAL: u32 = 0xFFFFFFFF;
        let wait_result = WaitForSingleObject(proc_handle, 0xFFFFFFFF); // INFINITE
        if wait_result == WAIT_FAILED_VAL {
            let err = last_error();
            error!(os_error = err.raw_os_error(), %err, "WaitForSingleObject failed for elevated process");
            CloseHandle(proc_handle);
            return Err(AppError::Io(err));
        }
        let mut exit_code: u32 = 0;
        if GetExitCodeProcess(proc_handle, &mut exit_code) == 0 {
            let err = last_error();
            warn!(os_error = err.raw_os_error(), %err, "GetExitCodeProcess failed");
        }
        CloseHandle(proc_handle);

        if exit_code != 0 {
            std::process::exit(exit_code as i32);
        }
        std::process::exit(0);
    }
}

/// Handle Windows service management commands.
pub async fn handle_service_command(action: ServiceCommand) -> Result<(), AppError> {
    match action {
        ServiceCommand::Run => {
            info!("Windows: service run command invoked");
            tokio::task::block_in_place(service::run_service_host)
        }
        ServiceCommand::Install => {
            ensure_elevated(&["service", "install"])?;
            install_service()
        }
        ServiceCommand::Uninstall => {
            ensure_elevated(&["service", "uninstall"])?;
            uninstall_service()
        }
        ServiceCommand::Start => {
            ensure_elevated(&["service", "start"])?;
            start_service()
        }
        ServiceCommand::Stop => {
            ensure_elevated(&["service", "stop"])?;
            stop_service()
        }
    }
}

fn install_service() -> Result<(), AppError> {
    let exe_path = std::env::current_exe().map_err(AppError::Io)?;
    let binary_path = format!("\"{}\" service run", exe_path.display());

    let service_name_w = to_wide_null(SERVICE_NAME);
    let display_name_w = to_wide_null(SERVICE_DISPLAY_NAME);
    let binary_path_w = to_wide_null(&binary_path);

    unsafe {
        // F6: use SC_MANAGER_CREATE_SERVICE instead of SC_MANAGER_ALL_ACCESS
        let scm_raw = OpenSCManagerW(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            SC_MANAGER_CREATE_SERVICE,
        );
        if scm_raw.is_null() {
            let err = last_error();
            error!(os_error = err.raw_os_error(), %err, "OpenSCManagerW failed");
            return Err(AppError::Io(err));
        }
        let scm = ScHandle(scm_raw);

        // Only request rights needed: set description + start the service after creation
        let svc_raw = CreateServiceW(
            scm.0,
            service_name_w.as_ptr(),
            display_name_w.as_ptr(),
            SERVICE_CHANGE_CONFIG | SERVICE_START,
            SERVICE_WIN32_OWN_PROCESS,
            SERVICE_AUTO_START,
            SERVICE_ERROR_NORMAL,
            binary_path_w.as_ptr(),
            std::ptr::null_mut(), // no load order group
            std::ptr::null_mut(), // no tag
            std::ptr::null_mut(), // no dependencies
            std::ptr::null_mut(), // LocalSystem account
            std::ptr::null_mut(), // no password
        );

        if svc_raw.is_null() {
            let err = last_error();
            error!(os_error = err.raw_os_error(), %err, "CreateServiceW failed");
            return Err(AppError::Io(err));
        }
        let svc = ScHandle(svc_raw);

        set_service_description(svc.0);

        // Start the service immediately after install
        if StartServiceW(svc.0, 0, std::ptr::null_mut()) == 0 {
            let err = last_error();
            warn!(os_error = err.raw_os_error(), %err, "service installed but failed to start; will start on next boot");
        } else {
            info!(service = SERVICE_NAME, "service started after install");
        }

        info!(
            service = SERVICE_NAME,
            binary_path, "service installed successfully"
        );
    }
    Ok(())
}

fn uninstall_service() -> Result<(), AppError> {
    let service_name_w = to_wide_null(SERVICE_NAME);

    unsafe {
        // F6: use SC_MANAGER_CONNECT instead of SC_MANAGER_ALL_ACCESS
        let scm_raw = OpenSCManagerW(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            SC_MANAGER_CONNECT,
        );
        if scm_raw.is_null() {
            let err = last_error();
            error!(os_error = err.raw_os_error(), %err, "OpenSCManagerW failed");
            return Err(AppError::Io(err));
        }
        let scm = ScHandle(scm_raw);

        // F6: use only the rights we need
        let svc_raw = OpenServiceW(
            scm.0,
            service_name_w.as_ptr(),
            SERVICE_STOP | DELETE | SERVICE_QUERY_STATUS,
        );
        if svc_raw.is_null() {
            let err = last_error();
            error!(os_error = err.raw_os_error(), %err, "OpenServiceW failed");
            return Err(AppError::Io(err));
        }
        let svc = ScHandle(svc_raw);

        // Try to stop the service first (ignore errors -- it might not be running)
        let mut status: SERVICE_STATUS = std::mem::zeroed();
        let _ = ControlService(svc.0, SERVICE_CONTROL_STOP, &mut status);

        if DeleteService(svc.0) == 0 {
            let err = last_error();
            error!(os_error = err.raw_os_error(), %err, "DeleteService failed");
            return Err(AppError::Io(err));
        }

        info!(service = SERVICE_NAME, "service uninstalled successfully");
    }
    Ok(())
}

fn start_service() -> Result<(), AppError> {
    let service_name_w = to_wide_null(SERVICE_NAME);

    unsafe {
        // F6: use SC_MANAGER_CONNECT instead of SC_MANAGER_ALL_ACCESS
        let scm_raw = OpenSCManagerW(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            SC_MANAGER_CONNECT,
        );
        if scm_raw.is_null() {
            let err = last_error();
            error!(os_error = err.raw_os_error(), %err, "OpenSCManagerW failed");
            return Err(AppError::Io(err));
        }
        let scm = ScHandle(scm_raw);

        // F6: use SERVICE_START instead of SERVICE_ALL_ACCESS
        let svc_raw = OpenServiceW(scm.0, service_name_w.as_ptr(), SERVICE_START);
        if svc_raw.is_null() {
            let err = last_error();
            error!(os_error = err.raw_os_error(), %err, "OpenServiceW failed");
            return Err(AppError::Io(err));
        }
        let svc = ScHandle(svc_raw);

        if StartServiceW(svc.0, 0, std::ptr::null_mut()) == 0 {
            let err = last_error();
            error!(os_error = err.raw_os_error(), %err, "StartServiceW failed");
            return Err(AppError::Io(err));
        }

        info!(service = SERVICE_NAME, "service started successfully");
    }
    Ok(())
}

fn stop_service() -> Result<(), AppError> {
    let service_name_w = to_wide_null(SERVICE_NAME);

    unsafe {
        // F6: use SC_MANAGER_CONNECT instead of SC_MANAGER_ALL_ACCESS
        let scm_raw = OpenSCManagerW(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            SC_MANAGER_CONNECT,
        );
        if scm_raw.is_null() {
            let err = last_error();
            error!(os_error = err.raw_os_error(), %err, "OpenSCManagerW failed");
            return Err(AppError::Io(err));
        }
        let scm = ScHandle(scm_raw);

        // F6: use SERVICE_STOP instead of SERVICE_ALL_ACCESS
        let svc_raw = OpenServiceW(scm.0, service_name_w.as_ptr(), SERVICE_STOP);
        if svc_raw.is_null() {
            let err = last_error();
            error!(os_error = err.raw_os_error(), %err, "OpenServiceW failed");
            return Err(AppError::Io(err));
        }
        let svc = ScHandle(svc_raw);

        let mut status: SERVICE_STATUS = std::mem::zeroed();
        if ControlService(svc.0, SERVICE_CONTROL_STOP, &mut status) == 0 {
            let err = last_error();
            error!(os_error = err.raw_os_error(), %err, "ControlService(STOP) failed");
            return Err(AppError::Io(err));
        }

        info!(
            service = SERVICE_NAME,
            "service stop requested successfully"
        );
    }
    Ok(())
}

/// Set the service description via `ChangeServiceConfig2W`.
fn set_service_description(svc_handle: *mut c_void) {
    let desc_w = to_wide_null(SERVICE_DESCRIPTION);
    let mut desc = SERVICE_DESCRIPTIONW {
        lpDescription: desc_w.as_ptr() as *mut u16,
    };

    unsafe {
        if ChangeServiceConfig2W(
            svc_handle,
            SERVICE_CONFIG_DESCRIPTION,
            &mut desc as *mut _ as *mut c_void,
        ) == 0
        {
            let err = last_error();
            tracing::warn!(os_error = err.raw_os_error(), %err, "failed to set service description");
        }
    }
}
