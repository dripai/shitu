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

use anyhow::{Result, anyhow};
use global_hotkey::{GlobalHotKeyEvent, HotKeyState as GlobalHotKeyEventState};
use slint::{CloseRequestResponse, ComponentHandle, SharedString, Timer};

use crate::{
    capture,
    config::{AppearanceMode, Config, OcrEngineKind},
    hotkey::HotkeyState,
    image::DrawStyle,
    logging,
    platform::{
        ocr::{AiOcrState, ai_availability, system_availability},
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
    let (system_ocr_available, system_ocr_status) = match system_availability() {
        Ok(()) => {
            logging::info("Windows system OCR is available");
            (true, "可用（Windows 系统 OCR）".to_owned())
        }
        Err(error) => {
            let message = error.message();
            logging::error(format!("Windows system OCR unavailable: {message}"));
            (false, format!("不可用：{message}"))
        }
    };
    let ai_ocr_state = match ai_availability() {
        Ok(state) => {
            logging::info(format!("Windows AI OCR state: {state:?}"));
            state
        }
        Err(error) => {
            let message = error.message();
            logging::error(format!("Windows AI OCR probe failed: {message}"));
            AiOcrState::Failed(message)
        }
    };

    let main = MainWindow::new()?;
    let ocr_result = OcrResultWindow::new()?;
    let tray = CaptureTray::new()?;
    let pins = Rc::new(RefCell::new(PinRegistry::default()));

    let (config, mut initial_status, mut initial_level) = match Config::load() {
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
    let (ocr_available, ocr_status) = selected_ocr_status(
        &config,
        system_ocr_available,
        &system_ocr_status,
        &ai_ocr_state,
    );
    if !ocr_available && initial_level != StatusLevel::Error {
        initial_status = ocr_status.clone();
        initial_level = StatusLevel::Error;
    }
    let state = Rc::new(RefCell::new(AppController::new(
        config,
        initial_status,
        initial_level,
        Rc::clone(&pins),
        ocr_result.as_weak(),
        OcrCapabilities {
            system_available: system_ocr_available,
            system_status: system_ocr_status,
            ai_state: ai_ocr_state,
        },
    )));

    refresh_main(&main, &state.borrow());
    settings::populate(&main, &state.borrow());
    bind_main_window(&main, Rc::clone(&state));
    bind_ocr_result_window(&ocr_result, main.as_weak(), Rc::clone(&state));
    settings::bind(&main, Rc::clone(&state));
    bind_tray(&tray, main.as_weak(), Rc::clone(&state));
    bind_hotkey_events(main.as_weak(), state.borrow().hotkey.active_id_handle());

    main.window()
        .on_close_requested(|| CloseRequestResponse::HideWindow);
    ocr_result
        .window()
        .on_close_requested(|| CloseRequestResponse::HideWindow);

    main.show()?;
    configure_main_window_frame(main.as_weak());
    tray.show()?;
    slint::run_event_loop_until_quit()
}

fn bind_main_window(main: &MainWindow, state: Rc<RefCell<AppController>>) {
    {
        let main = main.as_weak();
        let state = Rc::clone(&state);
        main.unwrap()
            .on_capture(move || capture_controller::start_capture(main.clone(), Rc::clone(&state)));
    }
}

fn bind_ocr_result_window(
    result: &OcrResultWindow,
    main: slint::Weak<MainWindow>,
    state: Rc<RefCell<AppController>>,
) {
    {
        let result = result.as_weak();
        result.unwrap().on_copy_text(move || {
            let Some(result) = result.upgrade() else {
                return;
            };
            let text = result.get_result_text();
            match capture::copy_text_to_clipboard(text.as_str()) {
                Ok(()) => {
                    result.set_status_text("已复制全部文字".into());
                    set_status_level(
                        &main,
                        &mut state.borrow_mut(),
                        "已复制 OCR 文字".to_owned(),
                        StatusLevel::Success,
                    );
                }
                Err(error) => {
                    logging::error(error.to_string());
                    result.set_status_text(format!("复制失败：{error}").into());
                    set_status_level(
                        &main,
                        &mut state.borrow_mut(),
                        format!("OCR 文字复制失败：{error}"),
                        StatusLevel::Error,
                    );
                }
            }
        });
    }
    {
        let result = result.as_weak();
        result.unwrap().on_close_result(move || {
            if let Some(result) = result.upgrade() {
                let _ = result.hide();
            }
        });
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

    configure_main_window_frame(main.as_weak());
}

fn configure_main_window_frame(main: slint::Weak<MainWindow>) {
    Timer::single_shot(Duration::from_millis(16), move || {
        if let Some(main) = main.upgrade() {
            window::remove_minimize_maximize(main.window());
            main.window().request_redraw();
        }
    });
}

fn refresh_main(main: &MainWindow, state: &AppController) {
    main.set_theme_mode(appearance_index(state.config.appearance));
    main.set_hotkey_text(state.config.hotkey.clone().unwrap_or_default().into());
    main.set_status_text(state.status.as_str().into());
    main.set_status_level(state.status_level as i32);
    main.set_ocr_available(state.ocr_available);
    main.set_ocr_status(state.ocr_status.as_str().into());
    main.set_system_ocr_available(state.system_ocr_available);
    main.set_system_ocr_status(state.system_ocr_status.as_str().into());
    main.set_ai_ocr_available(state.ai_ocr_state.is_ready());
    main.set_ai_ocr_status(state.ai_ocr_state.message().into());
    main.set_ai_ocr_can_install(state.ai_ocr_state.can_install());
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

fn present_ocr_result(result: &slint::Weak<OcrResultWindow>, text: &str) -> Result<()> {
    present_ocr_window(result, text, "可选择文字，或复制全部内容。")
}

fn present_ocr_error(result: &slint::Weak<OcrResultWindow>, message: &str) -> Result<()> {
    present_ocr_window(result, message, "OCR 识别失败，错误详情已写入日志。")
}

fn present_ocr_notice(result: &slint::Weak<OcrResultWindow>, message: &str) -> Result<()> {
    present_ocr_window(result, message, "OCR 识别已完成。")
}

fn present_ocr_window(
    result: &slint::Weak<OcrResultWindow>,
    text: &str,
    status: &str,
) -> Result<()> {
    let result = result
        .upgrade()
        .ok_or_else(|| anyhow!("OCR 结果窗口已不可用"))?;
    result.set_result_text(text.into());
    result.set_status_text(status.into());
    result.show()?;
    result.window().request_redraw();
    window::activate(result.window());
    Ok(())
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
    ocr_result: slint::Weak<OcrResultWindow>,
    ocr_available: bool,
    ocr_status: String,
    system_ocr_available: bool,
    system_ocr_status: String,
    ai_ocr_state: AiOcrState,
    draw_style: DrawStyle,
}

struct OcrCapabilities {
    system_available: bool,
    system_status: String,
    ai_state: AiOcrState,
}

impl AppController {
    fn new(
        config: Config,
        mut status: String,
        mut status_level: StatusLevel,
        pins: Rc<RefCell<PinRegistry>>,
        ocr_result: slint::Weak<OcrResultWindow>,
        capabilities: OcrCapabilities,
    ) -> Self {
        let OcrCapabilities {
            system_available: system_ocr_available,
            system_status: system_ocr_status,
            ai_state: ai_ocr_state,
        } = capabilities;
        let hotkey = HotkeyState::new(config.hotkey.as_deref());
        if let Some(error) = hotkey.error() {
            status = format!("启动快捷键异常：{}", error.message());
            status_level = StatusLevel::Error;
        }
        let (ocr_available, ocr_status) = selected_ocr_status(
            &config,
            system_ocr_available,
            &system_ocr_status,
            &ai_ocr_state,
        );
        Self {
            config,
            hotkey,
            status,
            status_level,
            capturing: false,
            restore_main_after_capture: false,
            session: None,
            pins,
            ocr_result,
            ocr_available,
            ocr_status,
            system_ocr_available,
            system_ocr_status,
            ai_ocr_state,
            draw_style: DrawStyle {
                rgba: [236, 92, 102, 255],
                radius: 2,
            },
        }
    }

    fn refresh_selected_ocr(&mut self) {
        (self.ocr_available, self.ocr_status) = selected_ocr_status(
            &self.config,
            self.system_ocr_available,
            &self.system_ocr_status,
            &self.ai_ocr_state,
        );
    }
}

fn selected_ocr_status(
    config: &Config,
    system_available: bool,
    system_status: &str,
    ai_state: &AiOcrState,
) -> (bool, String) {
    match config.ocr.engine {
        OcrEngineKind::System => (system_available, system_status.to_owned()),
        OcrEngineKind::WindowsAi => (ai_state.is_ready(), ai_state.message()),
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
