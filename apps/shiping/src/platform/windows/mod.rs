use std::{os::windows::ffi::OsStrExt, path::Path};

use anyhow::{Context, Result};

pub(crate) mod audio;
pub(crate) mod capture;
pub(crate) mod encoder;
pub(crate) mod shell;
pub(crate) mod target;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, POINT, WPARAM},
        Storage::FileSystem::{
            MOVE_FILE_FLAGS, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
        System::SystemInformation::GetLocalTime,
        UI::{
            Input::KeyboardAndMouse::ReleaseCapture,
            WindowsAndMessaging::{
                AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, HTCAPTION, MF_CHECKED,
                MF_STRING, PostMessageW, SendMessageW, SetForegroundWindow, TPM_LEFTALIGN,
                TPM_RETURNCMD, TPM_TOPALIGN, TrackPopupMenu, WM_NCLBUTTONDOWN, WM_NULL,
            },
        },
    },
    core::{HSTRING, PCWSTR},
};

pub(crate) struct ComRuntime;

impl ComRuntime {
    pub(crate) fn initialize() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .context("初始化 COM 失败")?;
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
    .with_context(|| format!("替换配置文件失败：{}", target.display()))
}

pub(crate) fn local_timestamp() -> String {
    let value = unsafe { GetLocalTime() };
    format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}",
        value.wYear, value.wMonth, value.wDay, value.wHour, value.wMinute, value.wSecond
    )
}

pub(crate) fn show_native_choice_menu(
    window: &slint::Window,
    labels: &[&str],
    checked_index: usize,
) -> Result<Option<usize>> {
    let raw = native_window_handle(window).context("无法获取主窗口句柄")?;
    let hwnd = HWND(raw as *mut _);
    let menu = unsafe { CreatePopupMenu() }.context("无法创建下拉菜单")?;

    let result = (|| -> Result<Option<usize>> {
        for (index, label) in labels.iter().enumerate() {
            let mut flags = MF_STRING;
            if index == checked_index {
                flags |= MF_CHECKED;
            }
            let title = HSTRING::from(*label);
            unsafe { AppendMenuW(menu, flags, index + 1, &title) }
                .with_context(|| format!("无法添加下拉选项：{label}"))?;
        }

        let mut cursor = POINT::default();
        unsafe { GetCursorPos(&mut cursor) }.context("无法获取菜单弹出位置")?;
        unsafe {
            let _ = SetForegroundWindow(hwnd);
        }
        let command = unsafe {
            TrackPopupMenu(
                menu,
                TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RETURNCMD,
                cursor.x,
                cursor.y,
                None,
                hwnd,
                None,
            )
        }
        .0 as usize;
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        }

        Ok(command.checked_sub(1).filter(|index| *index < labels.len()))
    })();

    let destroy_result = unsafe { DestroyMenu(menu) }.context("无法释放下拉菜单");
    if result.is_ok() {
        destroy_result?;
    }
    result
}

pub(crate) fn begin_window_drag(window: &slint::Window) {
    let Some(raw) = native_window_handle(window) else {
        return;
    };
    let hwnd = HWND(raw as *mut _);

    unsafe {
        let _ = ReleaseCapture();
        let _ = SendMessageW(
            hwnd,
            WM_NCLBUTTONDOWN,
            Some(WPARAM(HTCAPTION as usize)),
            Some(LPARAM(0)),
        );
    }
}

pub(crate) fn native_window_handle(window: &slint::Window) -> Option<isize> {
    let handle = window.window_handle();
    let handle = handle.window_handle().ok()?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return None;
    };
    Some(handle.hwnd.get())
}
