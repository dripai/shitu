#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use slint::ComponentHandle;

#[cfg(target_os = "windows")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[cfg(target_os = "windows")]
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::{
        Input::KeyboardAndMouse::ReleaseCapture,
        WindowsAndMessaging::{HTCAPTION, SendMessageW, WM_NCLBUTTONDOWN},
    },
};

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let main = MainWindow::new()?;

    {
        let main = main.as_weak();
        main.unwrap().on_start_recording(move || {
            if let Some(main) = main.upgrade() {
                main.set_status_text("录制功能尚未接入".into());
                main.set_status_level(0);
            }
        });
    }
    {
        let main = main.as_weak();
        main.unwrap().on_pause_recording(move || {
            if let Some(main) = main.upgrade() {
                main.set_status_text("暂停功能尚未接入".into());
                main.set_status_level(0);
            }
        });
    }
    {
        let main = main.as_weak();
        main.unwrap().on_stop_recording(move || {
            if let Some(main) = main.upgrade() {
                main.set_status_text("停止功能尚未接入".into());
                main.set_status_level(0);
            }
        });
    }
    #[cfg(target_os = "windows")]
    {
        let main = main.as_weak();
        main.unwrap().on_begin_window_drag(move || {
            if let Some(main) = main.upgrade() {
                begin_window_drag(main.window());
            }
        });
    }
    {
        let main = main.as_weak();
        main.unwrap().on_choose_source(move || {
            if let Some(main) = main.upgrade() {
                main.set_status_text("目标选择暂不可用".into());
                main.set_status_level(0);
            }
        });
    }
    {
        let main = main.as_weak();
        main.unwrap().on_choose_output_directory(move || {
            if let Some(main) = main.upgrade() {
                main.set_status_text("目录选择暂不可用".into());
                main.set_status_level(0);
            }
        });
    }
    {
        let main = main.as_weak();
        main.unwrap().on_open_output_directory(move || {
            if let Some(main) = main.upgrade() {
                main.set_status_text("输出目录尚未创建".into());
                main.set_status_level(0);
            }
        });
    }
    {
        let main = main.as_weak();
        main.unwrap().on_open_output_file(move || {
            if let Some(main) = main.upgrade() {
                main.set_status_text("录制文件尚未创建".into());
                main.set_status_level(0);
            }
        });
    }
    main.on_quit_application(|| {
        let _ = slint::quit_event_loop();
    });

    main.run()
}

#[cfg(target_os = "windows")]
fn begin_window_drag(window: &slint::Window) {
    let handle = window.window_handle();
    let Ok(handle) = handle.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut _);

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
