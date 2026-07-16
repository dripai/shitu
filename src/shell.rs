use std::path::Path;

use anyhow::{Result, anyhow};
use windows::{
    Win32::UI::Shell::ShellExecuteW, Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL, core::PCWSTR,
};

pub fn launch(path: &Path) -> Result<()> {
    let path_wide = wide(path.as_os_str());
    let verb = wide("open");
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(path_wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        return Err(anyhow!(
            "ShellExecuteW failed with code {}",
            result.0 as isize
        ));
    }
    Ok(())
}

fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.as_ref().encode_wide().chain(Some(0)).collect()
}
