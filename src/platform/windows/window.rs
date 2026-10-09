//! Physical desktop placement, ownership and topmost policy not exposed by GPUI 0.3.8.
//! GPUI owns rendering, input, DPI and window lifetime.
//! https://docs.rs/gpui-pre/0.3.8/gpui/struct.Window.html
use anyhow::{Context, Result, anyhow, ensure};
use gpui_kit::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::mem::size_of;
use windows::Win32::{
    Foundation::{GetLastError, HWND, POINT, RECT, SetLastError, WIN32_ERROR},
    Graphics::{
        Dwm::{
            DWMNCRENDERINGPOLICY, DWMNCRP_DISABLED, DWMNCRP_ENABLED, DWMWA_NCRENDERING_POLICY,
            DwmSetWindowAttribute,
        },
        Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect},
    },
    UI::WindowsAndMessaging::{
        GWL_STYLE, GWLP_HWNDPARENT, GetCursorPos, GetPropW, GetWindowLongPtrW, GetWindowRect,
        HWND_NOTOPMOST, HWND_TOPMOST, IsWindowVisible, RemovePropW, SW_HIDE, SW_RESTORE,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW,
        SetWindowPos, ShowWindow, WS_CAPTION, WS_POPUP, WS_THICKFRAME,
    },
};

/// GPUI 0.3.8 creates PopUp with style 0 (WS_OVERLAPPED), and Windows
/// adds a non-client frame. This shrinks our client image by 16x8 pixels.
/// Use a genuine borderless popup; don't replace GPUI's window procedure.
/// https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-setwindowlongptrw
pub fn prepare_image_window(window: &Window, cover_taskbar: bool) -> Result<()> {
    let hwnd = hwnd(window)?;
    unsafe {
        SetLastError(WIN32_ERROR(0));
        let old = GetWindowLongPtrW(hwnd, GWL_STYLE);
        if old == 0 {
            GetLastError().ok().context("Read image window style")?;
        }
        let style = (old as u32 | WS_POPUP.0) & !(WS_CAPTION.0 | WS_THICKFRAME.0);
        SetLastError(WIN32_ERROR(0));
        if SetWindowLongPtrW(hwnd, GWL_STYLE, style as isize) == 0 {
            GetLastError()
                .ok()
                .context("Set borderless image window style")?;
        }
        // The backend sets NonRudeHWND on every window, clearing it only in
        // single-monitor fullscreen mode. Our capture covers a virtual desktop,
        // so undo that same property here, on our own capture window only.
        // gpui-pre-windows 0.3.8: window.rs::set_non_rude_hwnd.
        if cover_taskbar && !GetPropW(hwnd, windows::core::w!("NonRudeHWND")).is_invalid() {
            RemovePropW(hwnd, windows::core::w!("NonRudeHWND"))
                .context("Allow capture to cover the taskbar")?;
        }
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
        )
        .context("Apply borderless image window frame")?;
    }
    Ok(())
}

pub fn hwnd(window: &Window) -> Result<HWND> {
    match HasWindowHandle::window_handle(window)
        .map_err(|error| anyhow!("Window handle: {error}"))?
        .as_raw()
    {
        RawWindowHandle::Win32(handle) => Ok(HWND(handle.hwnd.get() as *mut _)),
        _ => Err(anyhow!("Expected a Win32 window")),
    }
}
/// Keep the floating toolbar above its editor even when the canvas is activated.
/// GPUI 0.3.8 WindowOptions has no owner option. A Win32 owned popup preserves
/// native focus/input behavior; repeatedly raising an unowned topmost window
/// would still let the editor obscure it on the next click.
/// https://learn.microsoft.com/windows/win32/winmsg/window-features#owned-windows
pub fn set_owner(window: &Window, owner: &Window) -> Result<()> {
    let window = hwnd(window)?;
    let owner = hwnd(owner)?;
    ensure!(window != owner, "A window cannot own itself");
    unsafe {
        SetLastError(WIN32_ERROR(0));
        if SetWindowLongPtrW(window, GWLP_HWNDPARENT, owner.0 as isize) == 0 {
            GetLastError().ok().context("Set toolbar owner")?;
        }
    }
    Ok(())
}
pub fn hide(window: &Window) -> Result<()> {
    unsafe {
        let _ = ShowWindow(hwnd(window)?, SW_HIDE);
    }
    Ok(())
}
pub fn show(window: &Window) -> Result<()> {
    unsafe {
        let _ = ShowWindow(hwnd(window)?, SW_RESTORE);
    }
    window.activate_window();
    Ok(())
}
pub fn show_without_activation(window: &Window) -> Result<()> {
    // SW_SHOWNA preserves the CURRENT physical size and active window.
    // SW_SHOWNOACTIVATE/SW_RESTORE may restore GPUI's original placement,
    // undoing the full-desktop dimensions set immediately before showing.
    unsafe {
        let _ = ShowWindow(
            hwnd(window)?,
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNA,
        );
    }
    Ok(())
}
pub fn visible(window: &Window) -> Result<bool> {
    Ok(unsafe { IsWindowVisible(hwnd(window)?) }.as_bool())
}
pub fn bounds(window: &Window) -> Result<RECT> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd(window)?, &mut rect) }.context("Read window rectangle")?;
    Ok(rect)
}
pub fn place(window: &Window, rect: RECT) -> Result<()> {
    ensure!(
        rect.right > rect.left && rect.bottom > rect.top,
        "Invalid window rectangle"
    );
    unsafe {
        SetWindowPos(
            hwnd(window)?,
            None,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
    }
    .context("Position window")
}
pub fn set_always_on_top(window: &Window, enabled: bool) -> Result<()> {
    unsafe {
        SetWindowPos(
            hwnd(window)?,
            Some(if enabled {
                HWND_TOPMOST
            } else {
                HWND_NOTOPMOST
            }),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    }
    .context("Set window topmost state")
}
pub fn cursor_position() -> Result<(i32, i32)> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.context("Read cursor position")?;
    Ok((point.x, point.y))
}
pub fn work_area_for_rect(rect: RECT) -> Result<RECT> {
    ensure!(
        rect.right > rect.left && rect.bottom > rect.top,
        "Invalid monitor selection rectangle"
    );
    let monitor = unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST) };
    ensure!(!monitor.0.is_null(), "No monitor for selection");
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetMonitorInfoW(monitor, &mut info) }
        .ok()
        .context("Read monitor work area")?;
    Ok(info.rcWork)
}
pub fn set_shadow(window: &Window, enabled: bool) -> Result<()> {
    let policy = if enabled {
        DWMNCRP_ENABLED
    } else {
        DWMNCRP_DISABLED
    };
    unsafe {
        DwmSetWindowAttribute(
            hwnd(window)?,
            DWMWA_NCRENDERING_POLICY,
            (&policy as *const DWMNCRENDERINGPOLICY).cast(),
            size_of::<i32>() as u32,
        )
    }
    .context("Set window shadow policy")
}
