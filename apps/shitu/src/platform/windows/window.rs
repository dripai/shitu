use std::mem::size_of;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::{PhysicalPosition, PhysicalSize, Window};
use windows::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, RECT, WPARAM},
    Graphics::{
        Dwm::{
            DWMNCRENDERINGPOLICY, DWMNCRP_DISABLED, DWMNCRP_ENABLED, DWMWA_NCRENDERING_POLICY,
            DwmSetWindowAttribute,
        },
        Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow},
    },
    UI::{
        Input::KeyboardAndMouse::ReleaseCapture,
        WindowsAndMessaging::{
            GWL_EXSTYLE, GWL_STYLE, GWLP_HWNDPARENT, GetWindowLongPtrW, HTCAPTION, HWND_NOTOPMOST,
            HWND_TOPMOST, LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
            SendMessageW, SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongPtrW,
            SetWindowPos, WM_NCLBUTTONDOWN, WS_EX_LAYERED, WS_EX_TOPMOST, WS_MAXIMIZEBOX,
            WS_MINIMIZEBOX,
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

pub fn remove_minimize_maximize(window: &Window) {
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let style = style & !WS_MINIMIZEBOX.0 & !WS_MAXIMIZEBOX.0;
        SetWindowLongPtrW(hwnd, GWL_STYLE, style as isize);
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

pub fn set_owner(window: &Window, owner: &Window) {
    let (Some(hwnd), Some(owner_hwnd)) = (hwnd(window), hwnd(owner)) else {
        return;
    };
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner_hwnd.0 as isize);
    }
}

pub fn position_below(anchor: &Window, floating: &Window, gap: i32) {
    let anchor_position = anchor.position();
    let anchor_size = anchor.size();
    let floating_size = floating.size();
    let mut x = anchor_position.x + (anchor_size.width as i32 - floating_size.width as i32) / 2;
    let below = anchor_position.y + anchor_size.height as i32 + gap;
    let mut y = below;

    if let Some(anchor_hwnd) = hwnd(anchor) {
        let monitor = unsafe { MonitorFromWindow(anchor_hwnd, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            let work = info.rcWork;
            x = x.clamp(
                work.left,
                (work.right - floating_size.width as i32).max(work.left),
            );
            if below + floating_size.height as i32 > work.bottom {
                y = (anchor_position.y - floating_size.height as i32 - gap).max(work.top);
            }
        }
    }

    floating.set_position(PhysicalPosition::new(x, y));
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
        let available_width = (right - left - 24).max(1) as u32;
        let available_height = (bottom - top - 24).max(1) as u32;
        let (width, height) =
            fitted_size(image_width, image_height, available_width, available_height);
        window.set_size(PhysicalSize::new(width, height));
        let x = left + ((right - left - width as i32) / 2);
        let y = top + ((bottom - top - height as i32) / 2);
        window.set_position(PhysicalPosition::new(x, y));
    }
}

fn fitted_size(
    image_width: u32,
    image_height: u32,
    available_width: u32,
    available_height: u32,
) -> (u32, u32) {
    let scale = (available_width as f64 / image_width as f64)
        .min(available_height as f64 / image_height as f64)
        .min(1.0);
    (
        (image_width as f64 * scale).round().max(1.0) as u32,
        (image_height as f64 * scale).round().max(1.0) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::fitted_size;

    #[test]
    fn fit_to_work_area_preserves_extreme_aspect_ratios() {
        assert_eq!(fitted_size(10_000, 100, 1_900, 1_000), (1_900, 19));
        assert_eq!(fitted_size(100, 10_000, 1_000, 1_900), (19, 1_900));
        assert_eq!(fitted_size(800, 600, 1_900, 1_000), (800, 600));
    }
}
