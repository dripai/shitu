mod annotation;
mod capture_controller;
mod pin;
mod settings;

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    thread,
    time::Duration,
};

use anyhow::Result;
use global_hotkey::{GlobalHotKeyEvent, HotKeyState as GlobalHotKeyEventState};
use slint::{CloseRequestResponse, ComponentHandle, SharedString, Timer};

use crate::{
    config::{AppearanceMode, Config},
    hotkey::HotkeyState,
    image::DrawStyle,
    logging,
    platform::{
        ocr::{OcrEngine, system_engine},
        windows::window,
    },
};
use capture_controller::CaptureSession;
use pin::PinRegistry;

slint::include_modules!();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatusLevel {
    Info = 0,
    Success = 1,
    Error = 2,
}

pub fn run() -> Result<(), slint::PlatformError> {
    logging::initialize();
    if !system_engine().is_available() {
        logging::info("Windows system OCR is not currently available");
    }

    let main = MainWindow::new()?;
    let tray = CaptureTray::new()?;
    let pins = Rc::new(RefCell::new(PinRegistry::default()));

    let (config, initial_status, initial_level) = match Config::load() {
        Ok(config) => (
            config,
            "就绪。右键托盘图标可打开菜单。".to_owned(),
            StatusLevel::Info,
        ),
        Err(error) => {
            logging::error(error.to_string());
            (
                Config::default(),
                format!("配置加载失败，已使用默认值：{error}"),
                StatusLevel::Error,
            )
        }
    };
    let state = Rc::new(RefCell::new(AppController::new(
        config,
        initial_status,
        initial_level,
        Rc::clone(&pins),
    )));

    refresh_main(&main, &state.borrow());
    settings::populate(&main, &state.borrow());
    bind_main_window(&main, Rc::clone(&state));
    settings::bind(&main, Rc::clone(&state));
    bind_tray(&tray, main.as_weak(), Rc::clone(&state));
    bind_hotkey_events(main.as_weak(), state.borrow().hotkey.active_id_handle());

    main.window()
        .on_close_requested(|| CloseRequestResponse::HideWindow);

    main.show()?;
    tray.show()?;
    slint::run_event_loop()
}

fn bind_main_window(main: &MainWindow, state: Rc<RefCell<AppController>>) {
    {
        let main = main.as_weak();
        let state = Rc::clone(&state);
        main.unwrap()
            .on_capture(move || capture_controller::start_capture(main.clone(), Rc::clone(&state)));
    }
}

fn bind_tray(tray: &CaptureTray, main: slint::Weak<MainWindow>, state: Rc<RefCell<AppController>>) {
    {
        let main = main.clone();
        let state = Rc::clone(&state);
        tray.on_capture(move || capture_controller::start_capture(main.clone(), Rc::clone(&state)));
    }
    {
        let main = main.clone();
        tray.on_show_main(move || show_main_window(&main));
    }
    {
        let main = main.clone();
        tray.on_hide_main(move || {
            if let Some(main) = main.upgrade() {
                let _ = main.hide();
            }
        });
    }
    tray.on_quit(|| {
        let _ = slint::quit_event_loop();
    });
}

fn bind_hotkey_events(main: slint::Weak<MainWindow>, active_id: Arc<AtomicU32>) {
    thread::spawn(move || {
        while let Ok(event) = GlobalHotKeyEvent::receiver().recv() {
            if !should_trigger_hotkey_event(&event, active_id.load(Ordering::Relaxed)) {
                continue;
            }
            if main
                .upgrade_in_event_loop(|main| main.invoke_capture())
                .is_err()
            {
                break;
            }
        }
    });
}

fn should_trigger_hotkey_event(event: &GlobalHotKeyEvent, active_id: u32) -> bool {
    active_id != 0 && event.state == GlobalHotKeyEventState::Pressed && event.id == active_id
}

fn show_main_window(main: &slint::Weak<MainWindow>) {
    let Some(main) = main.upgrade() else {
        return;
    };
    main.window().set_minimized(false);
    let _ = main.show();
    main.window().request_redraw();
    window::activate(main.window());

    let main = main.as_weak();
    Timer::single_shot(Duration::from_millis(16), move || {
        if let Some(main) = main.upgrade() {
            main.window().request_redraw();
        }
    });
}

fn refresh_main(main: &MainWindow, state: &AppController) {
    main.set_theme_mode(appearance_index(state.config.appearance));
    main.set_hotkey_text(state.config.hotkey.clone().unwrap_or_default().into());
    main.set_status_text(state.status.as_str().into());
    main.set_status_level(state.status_level as i32);
    main.set_ocr_available(cfg!(windows));
}

fn set_status(main: &slint::Weak<MainWindow>, state: &mut AppController, status: String) {
    set_status_level(main, state, status, StatusLevel::Info);
}

fn set_error_status(main: &slint::Weak<MainWindow>, state: &mut AppController, status: String) {
    set_status_level(main, state, status, StatusLevel::Error);
}

fn set_status_level(
    main: &slint::Weak<MainWindow>,
    state: &mut AppController,
    status: String,
    level: StatusLevel,
) {
    state.status = status;
    state.status_level = level;
    if let Some(main) = main.upgrade() {
        main.set_status_text(SharedString::from(state.status.as_str()));
        main.set_status_level(level as i32);
    }
}

fn refresh_main_if_available(main: &slint::Weak<MainWindow>, state: &AppController) {
    if let Some(main) = main.upgrade() {
        refresh_main(&main, state);
    }
}

struct AppController {
    config: Config,
    hotkey: HotkeyState,
    status: String,
    status_level: StatusLevel,
    capturing: bool,
    restore_main_after_capture: bool,
    session: Option<CaptureSession>,
    pins: Rc<RefCell<PinRegistry>>,
    draw_style: DrawStyle,
}

impl AppController {
    fn new(
        config: Config,
        mut status: String,
        mut status_level: StatusLevel,
        pins: Rc<RefCell<PinRegistry>>,
    ) -> Self {
        let hotkey = HotkeyState::new(config.hotkey.as_deref());
        if let Some(error) = hotkey.error() {
            status = format!("快捷键无效：{}", error.message());
            status_level = StatusLevel::Error;
        }
        Self {
            config,
            hotkey,
            status,
            status_level,
            capturing: false,
            restore_main_after_capture: false,
            session: None,
            pins,
            draw_style: DrawStyle {
                rgba: [236, 92, 102, 255],
                radius: 2,
            },
        }
    }
}

fn appearance_index(mode: AppearanceMode) -> i32 {
    match mode {
        AppearanceMode::System => 0,
        AppearanceMode::Light => 1,
        AppearanceMode::Dark => 2,
    }
}

#[cfg(test)]
mod tests {
    use global_hotkey::{GlobalHotKeyEvent, HotKeyState};

    use super::{capture_controller::normalized_selection, should_trigger_hotkey_event};

    #[test]
    fn selection_coordinates_support_reverse_drag_and_clamping() {
        assert_eq!(
            normalized_selection(300.0, 250.0, 100.0, 50.0, 1920, 1080),
            Some((100, 50, 200, 200))
        );
        assert_eq!(
            normalized_selection(-50.0, -25.0, 2000.0, 1200.0, 1920, 1080),
            Some((0, 0, 1920, 1080))
        );
        assert_eq!(
            normalized_selection(100.0, 100.0, 100.0, 200.0, 1920, 1080),
            None
        );
    }

    #[test]
    fn hotkey_events_require_pressed_state_and_current_id() {
        let pressed = GlobalHotKeyEvent {
            id: 42,
            state: HotKeyState::Pressed,
        };
        let released = GlobalHotKeyEvent {
            id: 42,
            state: HotKeyState::Released,
        };
        assert!(should_trigger_hotkey_event(&pressed, 42));
        assert!(!should_trigger_hotkey_event(&released, 42));
        assert!(!should_trigger_hotkey_event(&pressed, 7));
        assert!(!should_trigger_hotkey_event(&pressed, 0));
    }
}
