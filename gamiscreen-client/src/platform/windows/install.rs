use tokio::process::Command;
use tracing::{info, warn};

use crate::AppError;

pub const TASK_NAME: &str = "GamiScreen Client";

pub async fn install_for_current_user(start_now: bool) -> Result<(), AppError> {
    let exe = std::env::current_exe().map_err(AppError::Io)?;
    let exe_str = exe.display().to_string();
    // schtasks expects quotes around the full path
    let tr = format!("\"{}\"", exe_str);

    // Resolve account for Run As: prefer token-based lookup over env vars
    let run_as = lookup_current_account_name()
        .or_else(|| fallback_env_account_name())
        .unwrap_or_default();
    let args_vec = build_schtasks_create_args(TASK_NAME, &tr, Some(&run_as));

    // Create or update the task (use /F) as the current user, interactive token
    let status = Command::new("schtasks")
        .args(args_vec.iter().map(|s| s.as_str()))
        .status()
        .await
        .map_err(AppError::Io)?;
    if !status.success() {
        // Build a PowerShell command that elevates schtasks via UAC and waits for completion
        let ps_cmd = build_powershell_elevated_schtasks(&args_vec);
        let ps_status = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps_cmd])
            .status()
            .await
            .map_err(AppError::Io)?;
        if !ps_status.success() {
            return Err(AppError::Io(std::io::Error::other(format!(
                "schtasks elevated /Create failed with status {}",
                ps_status
            ))));
        }
    }

    info!(task=TASK_NAME, path=%exe_str, "Windows Scheduled Task installed for current user");

    if start_now {
        // Best-effort: start it immediately in current session
        let _ = Command::new("schtasks")
            .args(["/Run", "/TN", TASK_NAME])
            .status()
            .await;
    }
    Ok(())
}

pub async fn uninstall_for_current_user() -> Result<(), AppError> {
    let status = Command::new("schtasks")
        .args(["/Delete", "/F", "/TN", TASK_NAME])
        .status()
        .await
        .map_err(AppError::Io)?;
    if !status.success() {
        // Treat missing task as success
        warn!(task=TASK_NAME, status=%status, "schtasks /Delete failed or task missing; continuing");
    }
    Ok(())
}

/// Build a PowerShell command string that elevates and runs schtasks with the provided arguments.
fn build_powershell_elevated_schtasks(args: &[String]) -> String {
    // Each argument becomes a single-quoted PS string with single quotes doubled
    let mut ps_args = String::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            ps_args.push_str(",");
        }
        let token = a.replace('\'', "''");
        ps_args.push('\'');
        ps_args.push_str(&token);
        ps_args.push('\'');
    }
    // Example produced:
    // $p = Start-Process -FilePath 'schtasks' -ArgumentList @('/Create','/F',...) -Verb RunAs -Wait -PassThru; exit $p.ExitCode
    format!(
        "$p = Start-Process -FilePath 'schtasks' -ArgumentList @({}) -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
        ps_args
    )
}

/// Build the schtasks /Create args for our task.
fn build_schtasks_create_args(
    task_name: &str,
    tr_quoted: &str,
    run_as: Option<&str>,
) -> Vec<String> {
    let mut v = vec![
        "/Create".to_string(),
        "/F".to_string(),
        "/SC".to_string(),
        "ONLOGON".to_string(),
        "/RL".to_string(),
        "LIMITED".to_string(),
        "/IT".to_string(),
        "/TN".to_string(),
        task_name.to_string(),
        "/TR".to_string(),
        tr_quoted.to_string(),
    ];
    if let Some(ru) = run_as {
        v.push("/RU".to_string());
        v.push(ru.to_string());
    }
    v
}

/// Resolve current user account name as DOMAIN\\User using the access token SID.
fn lookup_current_account_name() -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, LookupAccountSidW, TOKEN_QUERY, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut needed: u32 = 0;
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

        // Probe sizes
        let mut name_len: u32 = 0;
        let mut domain_len: u32 = 0;
        let mut pe_use: i32 = 0;
        let ok_probe = LookupAccountSidW(
            std::ptr::null(),
            tu.User.Sid,
            std::ptr::null_mut(),
            &mut name_len,
            std::ptr::null_mut(),
            &mut domain_len,
            &mut pe_use,
        );
        let insufficient = ok_probe == 0 && name_len > 0 && domain_len > 0;
        if !insufficient {
            return None;
        }
        let mut name_buf: Vec<u16> = vec![0u16; name_len as usize];
        let mut domain_buf: Vec<u16> = vec![0u16; domain_len as usize];
        if LookupAccountSidW(
            std::ptr::null(),
            tu.User.Sid,
            name_buf.as_mut_ptr(),
            &mut name_len,
            domain_buf.as_mut_ptr(),
            &mut domain_len,
            &mut pe_use,
        ) == 0
        {
            return None;
        }
        let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
        let domain = String::from_utf16_lossy(&domain_buf[..domain_len as usize]);
        if domain.is_empty() {
            Some(name)
        } else {
            Some(format!("{}\\{}", domain, name))
        }
    }
}

fn fallback_env_account_name() -> Option<String> {
    let username = std::env::var("USERNAME").ok()?;
    let domain = std::env::var("USERDOMAIN")
        .ok()
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_default();
    if domain.is_empty() {
        Some(username)
    } else {
        Some(format!("{}\\{}", domain, username))
    }
}
