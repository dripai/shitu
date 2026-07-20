use std::{os::windows::ffi::OsStrExt, path::Path};

use anyhow::{Context, Result};
use shi_foundation::i18n;

pub(crate) mod audio;
pub(crate) mod capture;
pub(crate) mod encoder;
pub(crate) mod shell;
pub(crate) mod target;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::{
    Win32::{
        Storage::FileSystem::{
            MOVE_FILE_FLAGS, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
        System::SystemInformation::GetLocalTime,
    },
    core::PCWSTR,
};

pub(crate) struct ComRuntime;

impl ComRuntime {
    pub(crate) fn initialize() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .context(i18n::text("初始化 COM 失败", "Failed to initialize COM"))?;
        Ok(Self)
    }
}

impl Drop for ComRuntime {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub(crate) fn replace_file(source: &Path, target: &Path) -> Result<()> {
    let source_wide: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let target_wide: Vec<u16> = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let flags = MOVE_FILE_FLAGS(MOVEFILE_REPLACE_EXISTING.0 | MOVEFILE_WRITE_THROUGH.0);
    unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(target_wide.as_ptr()),
            flags,
        )
    }
    .with_context(|| {
        format!(
            "{}: {}",
            i18n::text("替换配置文件失败", "Failed to replace the settings file"),
            target.display()
        )
    })
}

pub(crate) fn local_timestamp() -> String {
    let value = unsafe { GetLocalTime() };
    format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}",
        value.wYear, value.wMonth, value.wDay, value.wHour, value.wMinute, value.wSecond
    )
}

pub(crate) fn native_window_handle(window: &slint::Window) -> Option<isize> {
    let handle = window.window_handle();
    let handle = handle.window_handle().ok()?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return None;
    };
    Some(handle.hwnd.get())
}
