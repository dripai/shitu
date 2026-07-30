use std::mem::size_of;

use anyhow::{Result, anyhow};
use shi_foundation::i18n;
use windows::Win32::{
    Foundation::{HWND, LPARAM, RECT},
    Graphics::{
        Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
        Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO},
    },
    UI::WindowsAndMessaging::{
        EnumWindows, GWL_EXSTYLE, GetSystemMetrics, GetWindowLongW, GetWindowRect,
        GetWindowTextLengthW, GetWindowTextW, IsIconic, IsWindowVisible, MONITORINFOF_PRIMARY,
        SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN, WS_EX_TRANSPARENT,
    },
};

use crate::{
    domain::{
        Bounds, MonitorCandidate, MonitorCandidates, RecordingTarget, WindowCandidate,
        WindowCandidates, WindowId,
    },
    ports::TargetSelection,
};

pub(crate) static WINDOWS_TARGET_SELECTION: WindowsTargetSelection = WindowsTargetSelection;

pub(crate) struct WindowsTargetSelection;

impl TargetSelection for WindowsTargetSelection {
    fn monitors(&self, _owner: Option<&slint::Window>) -> Result<MonitorCandidates> {
        snapshot_monitors()
    }

    fn windows(&self, desktop: Bounds) -> Result<WindowCandidates> {
        snapshot_windows(desktop)
    }

    fn primary_screen_bounds(&self) -> Result<Bounds> {
        primary_screen_bounds()
    }

    fn virtual_desktop_bounds(&self) -> Result<Bounds> {
        virtual_desktop_bounds()
    }

    fn current_bounds(&self, target: RecordingTarget) -> Result<Bounds> {
        match target {
            RecordingTarget::Screen(bounds) | RecordingTarget::Region(bounds) => bounds.validate(),
            RecordingTarget::Window { id, .. } => window_bounds(id)
                .ok_or_else(|| {
                    anyhow!(i18n::text(
                        "所选窗口已关闭、隐藏或最小化",
                        "The selected window was closed, hidden, or minimized"
                    ))
                })?
                .validate(),
        }
    }
}

fn snapshot_monitors() -> Result<MonitorCandidates> {
    unsafe extern "system" fn enumerate(
        monitor: HMONITOR,
        _device_context: HDC,
        _bounds: *mut RECT,
        parameter: LPARAM,
    ) -> windows::core::BOOL {
        let values = unsafe { &mut *(parameter.0 as *mut Vec<MonitorCandidate>) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            values.push(MonitorCandidate {
                bounds: Bounds {
                    left: info.rcMonitor.left,
                    top: info.rcMonitor.top,
                    width: info.rcMonitor.right.saturating_sub(info.rcMonitor.left),
                    height: info.rcMonitor.bottom.saturating_sub(info.rcMonitor.top),
                },
                primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
            });
        }
        windows::core::BOOL(1)
    }

    let mut values: Vec<MonitorCandidate> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(enumerate),
            LPARAM((&mut values as *mut Vec<MonitorCandidate>) as isize),
        )
        .ok()?;
    }
    values.retain(|monitor| monitor.bounds.validate().is_ok());
    values.sort_by_key(|monitor| (!monitor.primary, monitor.bounds.top, monitor.bounds.left));
    if values.is_empty() {
        return Err(anyhow!(i18n::text(
            "未检测到可录制的显示器",
            "No recordable display was detected"
        )));
    }
    Ok(MonitorCandidates::new(values))
}

fn snapshot_windows(desktop: Bounds) -> Result<WindowCandidates> {
    struct Enumeration {
        desktop: Bounds,
        values: Vec<WindowCandidate>,
    }

    unsafe extern "system" fn enumerate(hwnd: HWND, parameter: LPARAM) -> windows::core::BOOL {
        let enumeration = unsafe { &mut *(parameter.0 as *mut Enumeration) };
        let id = window_id(hwnd);
        if let Some(bounds) = clipped_window_bounds(id, enumeration.desktop) {
            enumeration.values.push(WindowCandidate {
                id,
                bounds,
                title: window_title(id),
            });
        }
        windows::core::BOOL(1)
    }

    let mut enumeration = Enumeration {
        desktop,
        values: Vec::new(),
    };
    unsafe {
        EnumWindows(
            Some(enumerate),
            LPARAM((&mut enumeration as *mut Enumeration) as isize),
        )?;
    }
    Ok(WindowCandidates::new(enumeration.values))
}

fn window_title(id: WindowId) -> String {
    let handle = window_handle(id);
    let length = unsafe { GetWindowTextLengthW(handle) };
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0_u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(handle, &mut buffer) };
    if copied <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..copied as usize])
        .trim()
        .to_owned()
}

fn virtual_desktop_bounds() -> Result<Bounds> {
    Bounds {
        left: unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) },
        top: unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) },
        width: unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) },
        height: unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) },
    }
    .validate()
}

fn primary_screen_bounds() -> Result<Bounds> {
    Bounds {
        left: 0,
        top: 0,
        width: unsafe { GetSystemMetrics(SM_CXSCREEN) },
        height: unsafe { GetSystemMetrics(SM_CYSCREEN) },
    }
    .validate()
}

fn window_bounds(id: WindowId) -> Option<Bounds> {
    let hwnd = window_handle(id);
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() || unsafe { IsIconic(hwnd) }.as_bool() {
        return None;
    }
    let mut rect = RECT::default();
    if unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut rect as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        )
    }
    .is_err()
        && unsafe { GetWindowRect(hwnd, &mut rect) }.is_err()
    {
        return None;
    }
    Some(Bounds {
        left: rect.left,
        top: rect.top,
        width: rect.right.saturating_sub(rect.left),
        height: rect.bottom.saturating_sub(rect.top),
    })
}

fn clipped_window_bounds(id: WindowId, desktop: Bounds) -> Option<Bounds> {
    let handle = window_handle(id);
    let extended_style = unsafe { GetWindowLongW(handle, GWL_EXSTYLE) } as u32;
    if extended_style & WS_EX_TRANSPARENT.0 != 0 {
        return None;
    }
    let mut cloaked = 0_u32;
    if unsafe {
        DwmGetWindowAttribute(
            handle,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut u32).cast(),
            size_of::<u32>() as u32,
        )
    }
    .is_ok()
        && cloaked != 0
    {
        return None;
    }
    let bounds = window_bounds(id)?;
    let left = bounds.left.max(desktop.left);
    let top = bounds.top.max(desktop.top);
    let right = (bounds.left + bounds.width).min(desktop.left + desktop.width);
    let bottom = (bounds.top + bounds.height).min(desktop.top + desktop.height);
    let clipped = Bounds {
        left,
        top,
        width: right.saturating_sub(left),
        height: bottom.saturating_sub(top),
    };
    (clipped.width >= 24 && clipped.height >= 24).then_some(clipped)
}

pub(crate) fn window_id(hwnd: HWND) -> WindowId {
    WindowId::from_platform_value(hwnd.0 as usize as u64)
}

fn window_handle(id: WindowId) -> HWND {
    HWND(id.platform_value() as usize as *mut _)
}
