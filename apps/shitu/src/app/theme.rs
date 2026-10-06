use std::{cell::RefCell, rc::Rc, time::Duration};

use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};
use slint::{ComponentHandle, PlatformError, Timer};

use crate::{logging, platform::windows::window};

use super::{AppController, AppTheme, CaptureTray, MainWindow, NativeFrameStyle, OcrResultWindow};

pub(super) fn bind(
    main: &MainWindow,
    result: &OcrResultWindow,
    tray: &CaptureTray,
    state: Rc<RefCell<AppController>>,
) -> Result<(), PlatformError> {
    bind_native_frame(main, MainWindow::get_native_frame_style)?;
    bind_native_frame(result, OcrResultWindow::get_native_frame_style)?;
    let main_frame = main.as_weak();
    main.on_native_frame_changed(move || {
        if let Some(main) = main_frame.upgrade() {
            apply_native_frame(&main, MainWindow::get_native_frame_style);
        }
    });
    let result_frame = result.as_weak();
    result.on_native_frame_changed(move || {
        if let Some(result) = result_frame.upgrade() {
            apply_native_frame(&result, OcrResultWindow::get_native_frame_style);
        }
    });

    tray.global::<AppTheme>().set_mode(main.get_theme_mode());
    synchronize(main.get_theme_mode(), &state.borrow());
    let tray = tray.as_weak();
    main.on_theme_changed(move |mode| {
        if let Some(tray) = tray.upgrade() {
            tray.global::<AppTheme>().set_mode(mode);
        }
        synchronize(mode, &state.borrow());
    });
    Ok(())
}

fn bind_native_frame<T: ComponentHandle + 'static>(
    component: &T,
    read_style: fn(&T) -> NativeFrameStyle,
) -> Result<(), PlatformError> {
    // These framed windows have no other winit event filter. Let Slint process
    // system theme notifications before reapplying the resolved app colors.
    let weak = component.as_weak();
    component.window().on_winit_window_event(move |_, event| {
        if matches!(event, WindowEvent::ThemeChanged(_)) {
            let weak = weak.clone();
            Timer::single_shot(Duration::ZERO, move || {
                if let Some(component) = weak.upgrade() {
                    apply_native_frame(&component, read_style);
                }
            });
        }
        EventResult::Propagate
    });

    // The native window can be created lazily. The official accessor future
    // waits for it; apply the latest preview colors when that window exists.
    let weak = component.as_weak();
    slint::spawn_local(async move {
        let Some(component) = weak.upgrade() else {
            return;
        };
        match component.window().winit_window().await {
            Ok(_) => apply_native_frame(&component, read_style),
            Err(error) => {
                logging::error(format!("Native title bar initialization failed: {error}"))
            }
        }
    })
    .map_err(|error| PlatformError::Other(error.to_string()))?;
    Ok(())
}

fn apply_native_frame<T: ComponentHandle>(component: &T, read_style: fn(&T) -> NativeFrameStyle) {
    let style = read_style(component);
    // Before the first native creation, bind_native_frame's future owns the
    // pending update. A hidden OCR window picks up current colors on creation.
    let _ = component.window().with_winit_window(|native| {
        window::set_titlebar_theme(native, style.dark, style.background, style.foreground);
    });
}

fn synchronize(mode: i32, state: &AppController) {
    // Slint globals are per top-level component, not process-wide singletons.
    // Copy the preview mode; each Palette still handles live system changes.
    if let Some(result) = state.ocr_result.upgrade() {
        result.global::<AppTheme>().set_mode(mode);
    }
    if let Some(session) = &state.session {
        session.set_theme_mode(mode);
    }
    state.pins.borrow().set_theme_mode(mode);
}
