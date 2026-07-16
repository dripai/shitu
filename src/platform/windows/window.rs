use anyhow::{Result, anyhow};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::{PhysicalPosition, PhysicalSize, Window};
use windows::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, POINT, RECT, WPARAM},
    Graphics::{
        Dwm::{
            DWMNCRENDERINGPOLICY, DWMNCRP_DISABLED, DWMNCRP_ENABLED, DWMWA_NCRENDERING_POLICY,
            DwmSetWindowAttribute,
        },
        Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
            MonitorFromWindow,
        },
    },
    UI::{
        Input::KeyboardAndMouse::ReleaseCapture,
        WindowsAndMessaging::{
            GWL_EXSTYLE, GWLP_HWNDPARENT, GetCursorPos, GetForegroundWindow, GetWindowLongPtrW,
            HTCAPTION, HWND_NOTOPMOST, HWND_TOPMOST, LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOMOVE,
            SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetForegroundWindow,
            SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowLongPtrW, SetWindowPos,
            WDA_EXCLUDEFROMCAPTURE, WDA_NONE, WM_NCLBUTTONDOWN, WS_EX_APPWINDOW, WS_EX_LAYERED,
            WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        },
    },
};

pub fn hwnd(window: &Window) -> Option<HWND> {
    let handle = window.window_handle();
    let Ok(handle) = handle.window_handle() else {
        return None;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return None;
    };
    Some(HWND(handle.hwnd.get() as *mut _))
}

pub fn activate(window: &Window) {
    if let Some(hwnd) = hwnd(window) {
        unsafe {
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

pub fn set_excluded_from_capture(window: &Window, excluded: bool) -> Result<()> {
    let hwnd = hwnd(window).ok_or_else(|| anyhow!("窗口句柄尚未就绪"))?;
    let affinity = if excluded {
        WDA_EXCLUDEFROMCAPTURE
    } else {
        WDA_NONE
    };
    unsafe { SetWindowDisplayAffinity(hwnd, affinity) }.map_err(Into::into)
}

pub fn configure_context_menu(window: &Window, owner: &Window) {
    let (Some(menu_hwnd), Some(owner_hwnd)) = (hwnd(window), hwnd(owner)) else {
        return;
    };
    unsafe {
        let mut style = GetWindowLongPtrW(menu_hwnd, GWL_EXSTYLE) as u32;
        style |= WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0;
        style &= !WS_EX_APPWINDOW.0;
        SetWindowLongPtrW(menu_hwnd, GWL_EXSTYLE, style as isize);
        SetWindowLongPtrW(menu_hwnd, GWLP_HWNDPARENT, owner_hwnd.0 as isize);
        let _ = SetWindowPos(
            menu_hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE,
        );
    }
}

pub fn place_context_menu_at_cursor(window: &Window) {
    let mut cursor = POINT::default();
    if unsafe { GetCursorPos(&mut cursor) }.is_err() {
        return;
    }

    let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return;
    }

    let size = window.size();
    let width = size.width as i32;
    let height = size.height as i32;
    let RECT {
        left,
        top,
        right,
        bottom,
    } = info.rcWork;
    let x = if cursor.x + width + 2 <= right {
        cursor.x + 2
    } else {
        cursor.x - width - 2
    }
    .clamp(left, (right - width).max(left));
    let y = if cursor.y + height + 2 <= bottom {
        cursor.y + 2
    } else {
        cursor.y - height - 2
    }
    .clamp(top, (bottom - height).max(top));
    window.set_position(PhysicalPosition::new(x, y));
}

pub fn is_foreground(window: &Window) -> bool {
    hwnd(window).is_some_and(|handle| unsafe { GetForegroundWindow() } == handle)
}

pub fn drag(window: &Window) {
    if let Some(hwnd) = hwnd(window) {
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
}

pub fn set_opacity(window: &Window, opacity_percent: u8) {
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    unsafe {
        let mut style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        style |= WS_EX_LAYERED.0;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style as isize);
        let alpha = ((opacity_percent.clamp(25, 100) as u16 * 255) / 100) as u8;
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        );
    }
}

pub fn set_always_on_top(window: &Window, enabled: bool) {
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    unsafe {
        let target = if enabled {
            HWND_TOPMOST
        } else {
            HWND_NOTOPMOST
        };
        let _ = SetWindowPos(hwnd, Some(target), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        let mut style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        if enabled {
            style |= WS_EX_TOPMOST.0;
        } else {
            style &= !WS_EX_TOPMOST.0;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style as isize);
    }
}

pub fn set_shadow(window: &Window, enabled: bool) {
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    let policy = if enabled {
        DWMNCRP_ENABLED
    } else {
        DWMNCRP_DISABLED
    };
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY,
            (&policy as *const DWMNCRENDERINGPOLICY).cast(),
            size_of::<i32>() as u32,
        );
    }
}

pub fn fit_to_work_area(window: &Window, image_width: u32, image_height: u32) {
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        let RECT {
            left,
            top,
            right,
            bottom,
        } = info.rcWork;
        let available_width = (right - left - 24).max(80) as f64;
        let available_height = (bottom - top - 24).max(60) as f64;
        let scale = (available_width / image_width as f64)
            .min(available_height / image_height as f64)
            .min(1.0);
        let width = (image_width as f64 * scale).round().max(80.0) as u32;
        let height = (image_height as f64 * scale).round().max(60.0) as u32;
        window.set_size(PhysicalSize::new(width, height));
        let x = left + ((right - left - width as i32) / 2);
        let y = top + ((bottom - top - height as i32) / 2);
        window.set_position(PhysicalPosition::new(x, y));
    }
}

use std::mem::size_of;
