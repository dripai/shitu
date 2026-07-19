use std::{ffi::OsStr, path::Path};

use anyhow::{Result, anyhow};

#[cfg(windows)]
pub fn open_path(path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        core::PCWSTR,
    };

    let target: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let verb: Vec<u16> = OsStr::new("open")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        return Err(anyhow!("打开目标失败，系统返回代码 {}", result.0 as isize));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn open_path(_path: &Path) -> Result<()> {
    Err(anyhow!("当前平台尚未实现打开路径"))
}
