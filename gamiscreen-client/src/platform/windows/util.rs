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
