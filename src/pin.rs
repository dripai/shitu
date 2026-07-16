use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::Window;
use windows::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, WPARAM},
    UI::{
        Input::KeyboardAndMouse::ReleaseCapture,
        WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongPtrW, HTCAPTION, LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOMOVE,
            SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetForegroundWindow,
            SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, WM_NCLBUTTONDOWN,
            WS_EX_LAYERED, WS_EX_TRANSPARENT,
        },
    },
};

fn hwnd(window: &Window) -> Option<HWND> {
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

pub fn apply(window: &Window, opacity: u8, click_through: bool) {
    let Some(hwnd) = hwnd(window) else {
        return;
    };

    unsafe {
        let mut style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        style |= WS_EX_LAYERED.0;
        if click_through {
            style |= WS_EX_TRANSPARENT.0;
        } else {
            style &= !WS_EX_TRANSPARENT.0;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style as isize);
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), opacity, LWA_ALPHA);
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
