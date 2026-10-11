use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Result, anyhow};
use windows::{
    Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    core::PCWSTR,
};

pub fn open_path(path: &Path) -> Result<()> {
    shell_execute(path.as_os_str(), None)
}

/// Run on a dedicated worker. ShellExecute can pump COM messages, so do not
/// call it while a GPUI view/App is borrowed on the UI thread.
/// https://learn.microsoft.com/windows/win32/api/shellapi/nf-shellapi-shellexecutew
pub fn open_on_worker(path: &Path, reveal: bool) -> Result<()> {
    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).ok()?;
    }
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }
    let _apartment = Apartment;
    if reveal {
        reveal_in_folder(path)
    } else {
        open_path(path)
    }
}

pub fn reveal_in_folder(path: &Path) -> Result<()> {
    let canonical = path.canonicalize().unwrap_or_else(|_| PathBuf::from(path));
    let status = Command::new("explorer.exe")
        .arg(format!("/select,{}", canonical.display()))
        .status()?;
    if !status.success() {
        return Err(anyhow!(crate::i18n::text("无法在文件夹中显示该文件")));
    }
    Ok(())
}

fn shell_execute(target: &OsStr, parameters: Option<&OsStr>) -> Result<()> {
    let target = wide(target);
    let verb = wide(OsStr::new("open"));
    let parameters = parameters.map(wide);
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(target.as_ptr()),
            parameters
                .as_ref()
                .map_or(PCWSTR::null(), |value| PCWSTR(value.as_ptr())),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        return Err(anyhow!(
            "{}: {}",
            crate::i18n::text("打开目标失败，系统返回代码"),
            result.0 as isize
        ));
    }
    Ok(())
}

fn wide(value: &OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(std::iter::once(0)).collect()
}
