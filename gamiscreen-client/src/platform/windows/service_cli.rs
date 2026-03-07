use std::ffi::c_void;
use std::ptr::null_mut;

use tracing::{error, info};
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::Services::{
    CloseServiceHandle, ControlService, CreateServiceW, DeleteService, OpenSCManagerW,
    OpenServiceW, SC_MANAGER_ALL_ACCESS, SERVICE_ALL_ACCESS, SERVICE_AUTO_START,
    SERVICE_CONTROL_STOP, SERVICE_ERROR_NORMAL, SERVICE_STATUS, SERVICE_WIN32_OWN_PROCESS,
    StartServiceW,
};

use super::service;
use crate::AppError;
use crate::cli::ServiceCommand;

const SERVICE_NAME: &str = "GamiScreenAgent";
const SERVICE_DISPLAY_NAME: &str = "GamiScreen Agent";
const SERVICE_DESCRIPTION: &str = "GamiScreen parental control agent";

/// Handle Windows service management commands.
pub async fn handle_service_command(action: ServiceCommand) -> Result<(), AppError> {
    match action {
        ServiceCommand::Run => {
            info!("Windows: service run command invoked");
            tokio::task::block_in_place(|| service::run_service_host())
        }
        ServiceCommand::Install => install_service(),
        ServiceCommand::Uninstall => uninstall_service(),
        ServiceCommand::Start => start_service(),
        ServiceCommand::Stop => stop_service(),
    }
}

fn install_service() -> Result<(), AppError> {
    let exe_path = std::env::current_exe().map_err(AppError::Io)?;
    let binary_path = format!("\"{}\" service run", exe_path.display());

    let service_name_w = to_wide_null(SERVICE_NAME);
    let display_name_w = to_wide_null(SERVICE_DISPLAY_NAME);
    let binary_path_w = to_wide_null(&binary_path);

    unsafe {
        let scm = OpenSCManagerW(null_mut(), null_mut(), SC_MANAGER_ALL_ACCESS);
        if scm.is_null() {
            let err = last_error();
            error!(os_error = err.raw_os_error(), %err, "OpenSCManagerW failed");
            return Err(AppError::Io(err));
        }

        let svc = CreateServiceW(
            scm,
            service_name_w.as_ptr(),
            display_name_w.as_ptr(),
            SERVICE_ALL_ACCESS,
            SERVICE_WIN32_OWN_PROCESS,
            SERVICE_AUTO_START,
            SERVICE_ERROR_NORMAL,
            binary_path_w.as_ptr(),
            null_mut(), // no load order group
            null_mut(), // no tag
            null_mut(), // no dependencies
            null_mut(), // LocalSystem account
            null_mut(), // no password
        );

        if svc.is_null() {
            let err = last_error();
            CloseServiceHandle(scm);
            error!(os_error = err.raw_os_error(), %err, "CreateServiceW failed");
            return Err(AppError::Io(err));
        }

        set_service_description(svc);

        info!(
            service = SERVICE_NAME,
            binary_path, "service installed successfully"
        );
        CloseServiceHandle(svc);
        CloseServiceHandle(scm);
    }
    Ok(())
}

fn uninstall_service() -> Result<(), AppError> {
    let service_name_w = to_wide_null(SERVICE_NAME);

    unsafe {
        let scm = OpenSCManagerW(null_mut(), null_mut(), SC_MANAGER_ALL_ACCESS);
        if scm.is_null() {
            let err = last_error();
            error!(os_error = err.raw_os_error(), %err, "OpenSCManagerW failed");
            return Err(AppError::Io(err));
        }

        let svc = OpenServiceW(scm, service_name_w.as_ptr(), SERVICE_ALL_ACCESS);
        if svc.is_null() {
            let err = last_error();
            CloseServiceHandle(scm);
            error!(os_error = err.raw_os_error(), %err, "OpenServiceW failed");
            return Err(AppError::Io(err));
        }

        // Try to stop the service first (ignore errors — it might not be running)
        let mut status: SERVICE_STATUS = std::mem::zeroed();
        let _ = ControlService(svc, SERVICE_CONTROL_STOP, &mut status);

        if DeleteService(svc) == 0 {
            let err = last_error();
            CloseServiceHandle(svc);
            CloseServiceHandle(scm);
            error!(os_error = err.raw_os_error(), %err, "DeleteService failed");
            return Err(AppError::Io(err));
        }

        info!(service = SERVICE_NAME, "service uninstalled successfully");
        CloseServiceHandle(svc);
        CloseServiceHandle(scm);
    }
    Ok(())
}

fn start_service() -> Result<(), AppError> {
    let service_name_w = to_wide_null(SERVICE_NAME);

    unsafe {
        let scm = OpenSCManagerW(null_mut(), null_mut(), SC_MANAGER_ALL_ACCESS);
        if scm.is_null() {
            let err = last_error();
            error!(os_error = err.raw_os_error(), %err, "OpenSCManagerW failed");
            return Err(AppError::Io(err));
        }

        let svc = OpenServiceW(scm, service_name_w.as_ptr(), SERVICE_ALL_ACCESS);
        if svc.is_null() {
            let err = last_error();
            CloseServiceHandle(scm);
            error!(os_error = err.raw_os_error(), %err, "OpenServiceW failed");
            return Err(AppError::Io(err));
        }

        if StartServiceW(svc, 0, null_mut()) == 0 {
            let err = last_error();
            CloseServiceHandle(svc);
            CloseServiceHandle(scm);
            error!(os_error = err.raw_os_error(), %err, "StartServiceW failed");
            return Err(AppError::Io(err));
        }

        info!(service = SERVICE_NAME, "service started successfully");
        CloseServiceHandle(svc);
        CloseServiceHandle(scm);
    }
    Ok(())
}

fn stop_service() -> Result<(), AppError> {
    let service_name_w = to_wide_null(SERVICE_NAME);

    unsafe {
        let scm = OpenSCManagerW(null_mut(), null_mut(), SC_MANAGER_ALL_ACCESS);
        if scm.is_null() {
            let err = last_error();
            error!(os_error = err.raw_os_error(), %err, "OpenSCManagerW failed");
            return Err(AppError::Io(err));
        }

        let svc = OpenServiceW(scm, service_name_w.as_ptr(), SERVICE_ALL_ACCESS);
        if svc.is_null() {
            let err = last_error();
            CloseServiceHandle(scm);
            error!(os_error = err.raw_os_error(), %err, "OpenServiceW failed");
            return Err(AppError::Io(err));
        }

        let mut status: SERVICE_STATUS = std::mem::zeroed();
        if ControlService(svc, SERVICE_CONTROL_STOP, &mut status) == 0 {
            let err = last_error();
            CloseServiceHandle(svc);
            CloseServiceHandle(scm);
            error!(os_error = err.raw_os_error(), %err, "ControlService(STOP) failed");
            return Err(AppError::Io(err));
        }

        info!(
            service = SERVICE_NAME,
            "service stop requested successfully"
        );
        CloseServiceHandle(svc);
        CloseServiceHandle(scm);
    }
    Ok(())
}

/// Set the service description via `ChangeServiceConfig2W`.
fn set_service_description(svc_handle: *mut c_void) {
    use windows_sys::Win32::System::Services::{
        ChangeServiceConfig2W, SERVICE_CONFIG_DESCRIPTION, SERVICE_DESCRIPTIONW,
    };

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

fn to_wide_null(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn last_error() -> std::io::Error {
    std::io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
}
