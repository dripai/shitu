use std::{cell::Cell, mem::size_of, rc::Rc, time::Duration};

use anyhow::{Context, Result, ensure};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};
use slint::{PhysicalPosition, PhysicalSize, Timer, Window};
use windows::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, POINT, RECT, WPARAM},
    Graphics::{
        Dwm::{
            DWMNCRENDERINGPOLICY, DWMNCRP_DISABLED, DWMNCRP_ENABLED, DWMWA_NCRENDERING_POLICY,
            DwmSetWindowAttribute,
        },
        Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect,
            MonitorFromWindow,
        },
    },
    UI::{
        Input::KeyboardAndMouse::ReleaseCapture,
        WindowsAndMessaging::{
            GWL_EXSTYLE, GWL_STYLE, GWLP_HWNDPARENT, GetCursorPos, GetWindowLongPtrW, HTCAPTION,
            HWND_NOTOPMOST, HWND_TOPMOST, LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE,
            SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetLayeredWindowAttributes,
            SetWindowLongPtrW, SetWindowPos, WM_NCLBUTTONDOWN, WS_EX_LAYERED, WS_EX_TOPMOST,
            WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
        },
    },
};

thread_local! {
    static SKIP_TASKBAR_ON_CREATION: Cell<bool> = const { Cell::new(false) };
}

pub fn initialize_backend() -> Result<(), slint::PlatformError> {
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .with_winit_window_attributes_hook(|attributes| {
            if SKIP_TASKBAR_ON_CREATION.get() {
                attributes.with_skip_taskbar(true)
            } else {
                attributes
            }
        })
        .select()
}

pub fn create_without_taskbar<T>(
    create: impl FnOnce() -> Result<T, slint::PlatformError>,
) -> Result<T, slint::PlatformError> {
    // Slint 1.17 has no Window taskbar property. Its Window constructor invokes
    // the winit attributes hook synchronously, before the native window is shown.
    // Limit the official winit option to this constructor, including on failure.
    struct RestoreTaskbarPolicy(bool);

    impl Drop for RestoreTaskbarPolicy {
        fn drop(&mut self) {
            SKIP_TASKBAR_ON_CREATION.set(self.0);
        }
    }

    let _restore = RestoreTaskbarPolicy(SKIP_TASKBAR_ON_CREATION.replace(true));
    create()
}

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

pub fn cursor_position() -> Result<PhysicalPosition> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.context("Failed to read cursor position")?;
    Ok(PhysicalPosition::new(point.x, point.y))
}

pub fn on_scale_factor_changed(window: &Window, callback: impl Fn() + 'static) {
    // initialize_backend selects winit before creating any UI. The official
    // accessor registers on its adapter even before the first native show;
    // has_winit_window() only becomes true once that native window exists.
    let callback = Rc::new(callback);
    window.on_winit_window_event(move |_, event| {
        if matches!(event, WindowEvent::ScaleFactorChanged { .. }) {
            // Native filters run before Slint applies the new scale factor.
            // Reposition afterwards and leave DPI/resize handling to Slint.
            let callback = Rc::clone(&callback);
            Timer::single_shot(Duration::ZERO, move || callback());
        }
        EventResult::Propagate
    });
}

pub fn work_area_for_rect(rect: RECT) -> Result<RECT> {
    ensure!(
        rect.right > rect.left && rect.bottom > rect.top,
        "Invalid monitor selection rectangle"
    );
    // Slint 1.17 and winit 0.30 expose monitor size/position, not rcWork for
    // an arbitrary selection. Query only the native bounds; placement is shared
    // by screenshot and pin toolbars in app::toolbar_layout.
    let monitor = unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST) };
    ensure!(!monitor.0.is_null(), "Failed to find monitor for selection");
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetMonitorInfoW(monitor, &mut info) }
        .ok()
        .context("Failed to read monitor work area")?;
    Ok(info.rcWork)
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
    use super::{SKIP_TASKBAR_ON_CREATION, create_without_taskbar, fitted_size};

    #[test]
    fn taskbar_policy_is_restored_when_window_creation_fails() {
        assert!(!SKIP_TASKBAR_ON_CREATION.get());
        let result = create_without_taskbar::<()>(|| {
            assert!(SKIP_TASKBAR_ON_CREATION.get());
            Err("window creation failed".into())
        });
        assert!(result.is_err());
        assert!(!SKIP_TASKBAR_ON_CREATION.get());
    }

    #[test]
    fn nested_window_creation_restores_the_outer_taskbar_policy() {
        create_without_taskbar(|| {
            create_without_taskbar(|| Ok(()))?;
            assert!(SKIP_TASKBAR_ON_CREATION.get());
            Ok(())
        })
        .unwrap();
        assert!(!SKIP_TASKBAR_ON_CREATION.get());
    }

    #[test]
    fn fit_to_work_area_preserves_extreme_aspect_ratios() {
        assert_eq!(fitted_size(10_000, 100, 1_900, 1_000), (1_900, 19));
        assert_eq!(fitted_size(100, 10_000, 1_000, 1_900), (19, 1_900));
        assert_eq!(fitted_size(800, 600, 1_900, 1_000), (800, 600));
    }
}
