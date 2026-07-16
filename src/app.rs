mod annotation;

use std::{
    cell::RefCell,
    path::PathBuf,
    rc::{Rc, Weak as RcWeak},
    thread,
    time::Duration,
};

use anyhow::{Result, anyhow};
use global_hotkey::GlobalHotKeyEvent;
use slint::{
    CloseRequestResponse, ComponentHandle, ModelRc, PhysicalPosition, PhysicalSize, SharedString,
    Timer, VecModel,
};

use crate::{
    capture::{self, CapturedImage, DrawStyle},
    config::{AppearanceMode, CaptureConfig, CompletionAction, Config, ImageFormat, PinConfig},
    hotkey::{HotkeyState, validate_binding},
    logging, output,
    platform::{
        ocr::{OcrEngine, OcrFailure, system_engine},
        windows::{shell, startup, window},
    },
};
use annotation::AnnotationHistory;

slint::include_modules!();

pub fn run() -> Result<(), slint::PlatformError> {
    logging::initialize();
    if !system_engine().is_available() {
        logging::info("Windows system OCR is not currently available");
    }

    let main = MainWindow::new()?;
    let tray = CaptureTray::new()?;
    let pins = Rc::new(RefCell::new(PinRegistry::default()));

    let (config, initial_status) = match Config::load() {
        Ok(config) => (config, "就绪。右键托盘图标可打开菜单。".to_owned()),
        Err(error) => {
            logging::error(error.to_string());
            (
                Config::default(),
                format!("配置加载失败，已使用默认值：{error}"),
            )
        }
    };
    let state = Rc::new(RefCell::new(AppController::new(
        config,
        initial_status,
        Rc::clone(&pins),
    )));

    refresh_main(&main, &state.borrow());
    populate_settings(&main, &state.borrow());
    bind_main_window(&main, Rc::clone(&state));
    bind_settings(&main, Rc::clone(&state));
    bind_tray(&tray, main.as_weak(), Rc::clone(&state));
    bind_hotkey_events(main.as_weak());

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
            .on_capture(move || start_capture(main.clone(), Rc::clone(&state)));
    }
    {
        let main = main.as_weak();
        main.unwrap().on_hide_to_tray(move || {
            if let Some(main) = main.upgrade() {
                let _ = main.hide();
            }
        });
    }
    main.on_quit(|| {
        let _ = slint::quit_event_loop();
    });
}

fn bind_settings(settings: &MainWindow, state: Rc<RefCell<AppController>>) {
    let main = settings.as_weak();
    {
        let settings = settings.as_weak();
        let main = main.clone();
        let state = Rc::clone(&state);
        settings.unwrap().on_apply_settings(move || {
            apply_settings(&settings, &main, &state);
        });
    }
    {
        let settings = settings.as_weak();
        let state = Rc::clone(&state);
        settings.unwrap().on_cancel_settings(move || {
            if let Some(settings) = settings.upgrade() {
                populate_settings(&settings, &state.borrow());
                settings.set_settings_status("已撤销未保存修改".into());
            }
        });
    }
    {
        let settings = settings.as_weak();
        settings.unwrap().on_restore_defaults(move |tab| {
            if let Some(settings) = settings.upgrade() {
                restore_settings_page(&settings, tab);
                settings.set_settings_status("已恢复当前页默认值，尚未保存".into());
            }
        });
    }
    {
        let settings = settings.as_weak();
        settings.unwrap().on_choose_save_directory(move || {
            let Some(settings) = settings.upgrade() else {
                return;
            };
            let current = PathBuf::from(settings.get_save_directory().as_str());
            if let Some(path) = rfd::FileDialog::new().set_directory(current).pick_folder() {
                settings.set_save_directory(path.to_string_lossy().into_owned().into());
            }
        });
    }
    {
        let settings = settings.as_weak();
        settings.unwrap().on_open_save_directory(move || {
            let Some(settings) = settings.upgrade() else {
                return;
            };
            let path = PathBuf::from(settings.get_save_directory().as_str());
            let result = std::fs::create_dir_all(&path).map_err(anyhow::Error::from);
            let result = result.and_then(|_| shell::open_path(&path));
            set_settings_result(&settings, result, "已打开保存目录");
        });
    }
    {
        let settings = settings.as_weak();
        settings.unwrap().on_open_log_directory(move || {
            let Some(settings) = settings.upgrade() else {
                return;
            };
            let path = Config::log_directory();
            let result = std::fs::create_dir_all(&path).map_err(anyhow::Error::from);
            let result = result.and_then(|_| shell::open_path(&path));
            set_settings_result(&settings, result, "已打开日志文件夹");
        });
    }
    {
        let settings = settings.as_weak();
        let state = Rc::clone(&state);
        settings.unwrap().on_open_config_file(move || {
            let Some(settings) = settings.upgrade() else {
                return;
            };
            let path = Config::path();
            let result = if path.exists() {
                Ok(())
            } else {
                state.borrow().config.save()
            };
            let result = result.and_then(|_| shell::open_path(&path));
            set_settings_result(&settings, result, "已打开配置文件，修改后请重启应用");
        });
    }
    {
        let settings = settings.as_weak();
        settings.unwrap().on_open_licenses(move || {
            let Some(settings) = settings.upgrade() else {
                return;
            };
            let path = Config::third_party_licenses_path();
            let result = write_third_party_licenses(&path).and_then(|_| shell::open_path(&path));
            set_settings_result(&settings, result, "已打开第三方许可");
        });
    }
    {
        let settings = settings.as_weak();
        settings.unwrap().on_clear_hotkey(move || {
            if let Some(settings) = settings.upgrade() {
                settings.set_hotkey_text("".into());
                settings.set_hotkey_status(0);
                settings.set_hotkey_status_tip("".into());
            }
        });
    }
}

fn bind_tray(tray: &CaptureTray, main: slint::Weak<MainWindow>, state: Rc<RefCell<AppController>>) {
    {
        let main = main.clone();
        let state = Rc::clone(&state);
        tray.on_capture(move || start_capture(main.clone(), Rc::clone(&state)));
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

fn bind_hotkey_events(main: slint::Weak<MainWindow>) {
    thread::spawn(move || {
        while GlobalHotKeyEvent::receiver().recv().is_ok() {
            if main
                .upgrade_in_event_loop(|main| main.invoke_capture())
                .is_err()
            {
                break;
            }
        }
    });
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
    main.set_output_summary(output_summary(&state.config.capture).into());
    main.set_ocr_available(cfg!(windows));
}

fn populate_settings(settings: &MainWindow, state: &AppController) {
    settings.set_theme_mode(appearance_index(state.config.appearance));
    settings.set_launch_at_startup(state.config.launch_at_startup);
    settings.set_completion_action(completion_action_index(
        state.config.capture.completion_action,
    ));
    settings.set_image_format(image_format_index(state.config.capture.format));
    settings.set_jpeg_quality(state.config.capture.jpeg_quality as i32);
    settings.set_save_directory(
        state
            .config
            .capture
            .save_directory
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    settings.set_filename_template(state.config.capture.filename_template.clone().into());
    settings.set_auto_save(state.config.capture.auto_save);
    settings.set_save_notification(state.config.capture.save_notification);
    settings.set_pin_opacity(state.config.pin.default_opacity as i32);
    settings.set_pin_shadow(state.config.pin.shadow);
    settings.set_pin_always_on_top(state.config.pin.always_on_top);
    settings.set_pin_wheel_zoom(state.config.pin.wheel_zoom);
    settings.set_pin_zoom_step(state.config.pin.zoom_step as i32);
    settings.set_pin_double_click_close(state.config.pin.double_click_close);
    settings.set_hotkey_text(state.config.hotkey.clone().unwrap_or_default().into());
    set_hotkey_indicator(settings, state);
    settings.set_settings_status("".into());
    settings.set_version_text(format!("版本 {}", env!("CARGO_PKG_VERSION")).into());
    settings.set_build_text(build_information().into());
    settings.set_config_path(Config::path().to_string_lossy().into_owned().into());
}

fn set_hotkey_indicator(settings: &MainWindow, state: &AppController) {
    if state.config.hotkey.is_none() {
        settings.set_hotkey_status(0);
        settings.set_hotkey_status_tip("".into());
    } else if let Some(error) = state.hotkey.error() {
        settings.set_hotkey_status(2);
        settings.set_hotkey_status_tip(error.message().into());
    } else {
        settings.set_hotkey_status(1);
        settings.set_hotkey_status_tip("有效".into());
    }
}

fn apply_settings(
    settings: &slint::Weak<MainWindow>,
    main: &slint::Weak<MainWindow>,
    state: &Rc<RefCell<AppController>>,
) {
    let Some(settings) = settings.upgrade() else {
        return;
    };
    let mut candidate = config_from_settings(&settings);
    if let Err(error) = candidate.validate() {
        settings.set_settings_status(format!("设置无效：{error}").into());
        return;
    }
    if let Some(binding) = candidate.hotkey.as_deref()
        && let Err(error) = validate_binding(binding)
    {
        settings.set_hotkey_status(2);
        settings.set_hotkey_status_tip(error.message().into());
        settings.set_settings_status(format!("快捷键无效：{}", error.message()).into());
        return;
    }

    let old = state.borrow().config.clone();
    if let Err(error) = startup::set_enabled(candidate.launch_at_startup) {
        settings.set_settings_status(format!("开机启动设置失败：{error}").into());
        return;
    }

    {
        let mut state = state.borrow_mut();
        if let Err(error) = state.hotkey.set_binding(candidate.hotkey.as_deref()) {
            let _ = startup::set_enabled(old.launch_at_startup);
            settings.set_hotkey_status(2);
            settings.set_hotkey_status_tip(error.message().into());
            settings.set_settings_status(format!("快捷键设置失败：{}", error.message()).into());
            return;
        }
    }

    if let Err(error) = candidate.save() {
        let _ = startup::set_enabled(old.launch_at_startup);
        let _ = state.borrow_mut().hotkey.set_binding(old.hotkey.as_deref());
        settings.set_settings_status(format!("配置保存失败：{error}").into());
        logging::error(error.to_string());
        return;
    }

    {
        let mut state = state.borrow_mut();
        state.config = candidate;
        state.status = "设置已保存".to_owned();
        refresh_main_if_available(main, &state);
        populate_settings(&settings, &state);
    }
    settings.set_settings_status("设置已保存".into());
    logging::info("settings saved");
}

fn config_from_settings(settings: &MainWindow) -> Config {
    Config {
        appearance: appearance_from_index(settings.get_theme_mode()),
        launch_at_startup: settings.get_launch_at_startup(),
        hotkey: {
            let value = settings.get_hotkey_text().trim().to_owned();
            (!value.is_empty()).then_some(value)
        },
        capture: CaptureConfig {
            completion_action: completion_action_from_index(settings.get_completion_action()),
            format: image_format_from_index(settings.get_image_format()),
            jpeg_quality: settings.get_jpeg_quality().clamp(1, 100) as u8,
            save_directory: PathBuf::from(settings.get_save_directory().as_str()),
            filename_template: settings.get_filename_template().to_string(),
            auto_save: settings.get_auto_save(),
            save_notification: settings.get_save_notification(),
        },
        pin: PinConfig {
            default_opacity: settings.get_pin_opacity().clamp(25, 100) as u8,
            shadow: settings.get_pin_shadow(),
            always_on_top: settings.get_pin_always_on_top(),
            wheel_zoom: settings.get_pin_wheel_zoom(),
            zoom_step: settings.get_pin_zoom_step().clamp(5, 100) as u8,
            double_click_close: settings.get_pin_double_click_close(),
        },
    }
}

fn restore_settings_page(settings: &MainWindow, tab: i32) {
    let defaults = Config::default();
    match tab {
        0 => {
            settings.set_theme_mode(appearance_index(defaults.appearance));
            settings.set_launch_at_startup(defaults.launch_at_startup);
        }
        1 => {
            settings
                .set_completion_action(completion_action_index(defaults.capture.completion_action));
            settings.set_image_format(image_format_index(defaults.capture.format));
            settings.set_jpeg_quality(defaults.capture.jpeg_quality as i32);
            settings.set_save_directory(
                defaults
                    .capture
                    .save_directory
                    .to_string_lossy()
                    .into_owned()
                    .into(),
            );
            settings.set_filename_template(defaults.capture.filename_template.into());
            settings.set_auto_save(defaults.capture.auto_save);
            settings.set_save_notification(defaults.capture.save_notification);
        }
        2 => {
            settings.set_pin_opacity(defaults.pin.default_opacity as i32);
            settings.set_pin_shadow(defaults.pin.shadow);
            settings.set_pin_always_on_top(defaults.pin.always_on_top);
            settings.set_pin_wheel_zoom(defaults.pin.wheel_zoom);
            settings.set_pin_zoom_step(defaults.pin.zoom_step as i32);
            settings.set_pin_double_click_close(defaults.pin.double_click_close);
        }
        3 => {
            settings.set_hotkey_text("".into());
            settings.set_hotkey_status(0);
            settings.set_hotkey_status_tip("".into());
        }
        _ => {}
    }
}

fn set_settings_result(settings: &MainWindow, result: Result<()>, success: &str) {
    match result {
        Ok(()) => settings.set_settings_status(success.into()),
        Err(error) => {
            logging::error(error.to_string());
            settings.set_settings_status(format!("操作失败：{error}").into());
        }
    }
}

fn write_third_party_licenses(path: &std::path::Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, include_str!("../docs/third-party-licenses.md"))?;
    Ok(())
}

fn build_information() -> String {
    format!(
        "Windows {} · {} · Slint 1.17.0 · {}",
        std::env::consts::ARCH,
        env!("RUSTC_VERSION"),
        if cfg!(debug_assertions) {
            "Debug"
        } else {
            "Release"
        }
    )
}

fn start_capture(main: slint::Weak<MainWindow>, state: Rc<RefCell<AppController>>) {
    let Some(main_window) = main.upgrade() else {
        return;
    };

    {
        let mut state = state.borrow_mut();
        if state.capturing || state.session.is_some() {
            set_status(
                &main,
                &mut state,
                "已有截图任务正在进行，请先完成或取消".to_owned(),
            );
            return;
        }
        state.capturing = true;
        state.restore_main_after_capture = main_window.window().is_visible();
        set_status(&main, &mut state, "正在准备截图...".to_owned());
    }

    if main_window.window().is_visible() {
        let _ = main_window.hide();
    }

    Timer::single_shot(Duration::from_millis(90), move || {
        let result = capture::capture_virtual_desktop();
        state.borrow_mut().capturing = false;

        match result {
            Ok(image) => {
                logging::info("desktop captured");
                if let Err(error) = open_overlay(main.clone(), Rc::clone(&state), image) {
                    set_status(
                        &main,
                        &mut state.borrow_mut(),
                        format!("截图窗口打开失败：{error}"),
                    );
                    restore_main_after_capture(&main, &state);
                }
            }
            Err(error) => {
                logging::error(error.to_string());
                set_status(&main, &mut state.borrow_mut(), format!("截图失败：{error}"));
                restore_main_after_capture(&main, &state);
            }
        }
    });
}

fn open_overlay(
    main: slint::Weak<MainWindow>,
    state: Rc<RefCell<AppController>>,
    desktop: CapturedImage,
) -> Result<()> {
    let overlay = OverlayWindow::new()?;
    let bounds = desktop.bounds;
    let output_mode = completion_action_index(state.borrow().config.capture.completion_action);

    overlay.set_desktop_image(desktop.slint_image());
    overlay.set_annotations(empty_annotation_model());
    overlay.set_output_mode(output_mode);
    overlay.set_ocr_available(cfg!(windows));
    overlay
        .window()
        .set_position(PhysicalPosition::new(bounds.left, bounds.top));
    overlay
        .window()
        .set_size(PhysicalSize::new(bounds.width as u32, bounds.height as u32));

    bind_overlay(&overlay, main.clone(), Rc::clone(&state));
    state.borrow_mut().session = Some(CaptureSession {
        desktop,
        selected: None,
        annotations: AnnotationHistory::default(),
        _overlay: overlay.clone_strong(),
    });

    if let Err(error) = overlay.show() {
        state.borrow_mut().session = None;
        return Err(error.into());
    }
    Ok(())
}

fn bind_overlay(
    overlay: &OverlayWindow,
    main: slint::Weak<MainWindow>,
    state: Rc<RefCell<AppController>>,
) {
    {
        let overlay = overlay.as_weak();
        let state = Rc::clone(&state);
        overlay
            .unwrap()
            .on_selected(move |left, top, right, bottom| {
                let selection = state.borrow_mut().select_area(left, top, right, bottom);
                match selection {
                    Ok((image, info)) => {
                        if let Some(overlay) = overlay.upgrade() {
                            overlay.set_selected_image(image);
                            overlay.set_selection_info(info.into());
                            overlay.set_annotations(empty_annotation_model());
                        }
                    }
                    Err(error) => {
                        if let Some(overlay) = overlay.upgrade() {
                            overlay.set_selection_info(format!("选区无效：{error}").into());
                        }
                    }
                }
            });
    }
    {
        let overlay = overlay.as_weak();
        let state = Rc::clone(&state);
        overlay.unwrap().on_begin_annotation(move |x, y, tool| {
            state.borrow_mut().begin_annotation(x, y, tool);
            refresh_annotations(&overlay, &state);
        });
    }
    {
        let overlay = overlay.as_weak();
        let state = Rc::clone(&state);
        overlay.unwrap().on_update_annotation(move |x, y| {
            state.borrow_mut().update_annotation(x, y);
            refresh_annotations(&overlay, &state);
        });
    }
    {
        let state = Rc::clone(&state);
        overlay.on_finish_annotation(move || state.borrow_mut().finish_annotation());
    }
    {
        let state = Rc::clone(&state);
        overlay.on_select_color(move |index| state.borrow_mut().set_color(index));
    }
    {
        let state = Rc::clone(&state);
        overlay.on_select_width(move |radius| state.borrow_mut().set_width(radius));
    }
    {
        let overlay = overlay.as_weak();
        let state = Rc::clone(&state);
        overlay.unwrap().on_undo(move || {
            state.borrow_mut().undo();
            refresh_annotations(&overlay, &state);
        });
    }
    {
        let overlay = overlay.as_weak();
        let state = Rc::clone(&state);
        overlay.unwrap().on_redo(move || {
            state.borrow_mut().redo();
            refresh_annotations(&overlay, &state);
        });
    }
    {
        let overlay = overlay.as_weak();
        let main = main.clone();
        let state = Rc::clone(&state);
        overlay.unwrap().on_complete_output(move || {
            let (image, config) = {
                let state = state.borrow();
                (state.rendered_selection(), state.config.capture.clone())
            };
            match image.and_then(|image| execute_output(&image, &config)) {
                Ok(status) => finish_capture(&overlay, &main, &state, status),
                Err(error) => {
                    logging::error(error.to_string());
                    set_status(&main, &mut state.borrow_mut(), format!("输出失败：{error}"));
                }
            }
        });
    }
    {
        let overlay = overlay.as_weak();
        let state = Rc::clone(&state);
        overlay.unwrap().on_recognize_text(move || {
            let image = state.borrow().original_selection();
            match image {
                Ok(image) => {
                    if let Some(overlay) = overlay.upgrade() {
                        overlay.set_selection_info("正在识别文字...".into());
                    }
                    spawn_overlay_ocr(overlay.clone(), image);
                }
                Err(error) => {
                    if let Some(overlay) = overlay.upgrade() {
                        overlay.set_selection_info(format!("OCR 失败：{error}").into());
                    }
                }
            }
        });
    }
    {
        let overlay = overlay.as_weak();
        let main = main.clone();
        let state = Rc::clone(&state);
        overlay.unwrap().on_ocr_result(move |code, text| {
            handle_overlay_ocr_result(code, text.as_str(), &overlay, &main, &state);
        });
    }
    {
        let overlay = overlay.as_weak();
        let main = main.clone();
        let state = Rc::clone(&state);
        let app_state = Rc::downgrade(&state);
        overlay.unwrap().on_pin_selection(move || {
            let (image, pin_config, capture_config, pins) = {
                let state = state.borrow();
                (
                    state.rendered_selection(),
                    state.config.pin.clone(),
                    state.config.capture.clone(),
                    state.pins.clone(),
                )
            };
            let image = match image {
                Ok(image) => image,
                Err(error) => {
                    set_status(&main, &mut state.borrow_mut(), format!("钉住失败：{error}"));
                    return;
                }
            };
            let (source_path, auto_save_error) = if capture_config.auto_save {
                match output::save_quick(&image, &capture_config) {
                    Ok(path) => (Some(path), None),
                    Err(error) => {
                        logging::error(error.to_string());
                        (None, Some(error.to_string()))
                    }
                }
            } else {
                (None, None)
            };
            let show_save_result = capture_config.save_notification;
            match PinRegistry::add(
                &pins,
                image,
                source_path,
                pin_config,
                capture_config,
                main.clone(),
                app_state.clone(),
            ) {
                Ok(()) => {
                    let status = match auto_save_error {
                        Some(error) if show_save_result => {
                            format!("已钉住，但自动保存失败：{error}")
                        }
                        _ => "已将截图钉在屏幕上".to_owned(),
                    };
                    finish_capture(&overlay, &main, &state, status);
                }
                Err(error) => {
                    set_status(&main, &mut state.borrow_mut(), format!("钉住失败：{error}"));
                }
            }
        });
    }
    {
        let overlay = overlay.as_weak();
        overlay.unwrap().on_cancel_selection(move || {
            finish_capture(&overlay, &main, &state, "已取消截图".to_owned());
        });
    }
}

fn spawn_overlay_ocr(overlay: slint::Weak<OverlayWindow>, image: CapturedImage) {
    let width = image.width();
    let height = image.height();
    let bounds = image.bounds;
    let rgba = image.rgba_bytes();
    thread::spawn(move || {
        let result = CapturedImage::from_rgba(bounds.left, bounds.top, width, height, &rgba)
            .map_err(|error| OcrFailure::Failed(error.to_string()))
            .and_then(|image| system_engine().recognize(&image));
        let (code, text) = ocr_result_payload(result);
        let _ = overlay.upgrade_in_event_loop(move |overlay| {
            overlay.invoke_ocr_result(code, text.into());
        });
    });
}

fn handle_overlay_ocr_result(
    code: i32,
    text: &str,
    overlay: &slint::Weak<OverlayWindow>,
    main: &slint::Weak<MainWindow>,
    state: &Rc<RefCell<AppController>>,
) {
    match code {
        0 => match capture::copy_text_to_clipboard(text) {
            Ok(()) => finish_capture(overlay, main, state, "已识别并复制文字".to_owned()),
            Err(error) => {
                set_status(
                    main,
                    &mut state.borrow_mut(),
                    format!("OCR 文字复制失败：{error}"),
                );
            }
        },
        1 => set_overlay_message(overlay, main, state, "未识别到文字"),
        2 => set_overlay_message(overlay, main, state, "缺少中文 OCR 语言包"),
        3 => set_overlay_message(overlay, main, state, "当前平台不支持系统 OCR"),
        _ => set_overlay_message(overlay, main, state, "OCR 识别失败"),
    }
}

fn set_overlay_message(
    overlay: &slint::Weak<OverlayWindow>,
    main: &slint::Weak<MainWindow>,
    state: &Rc<RefCell<AppController>>,
    message: &str,
) {
    if let Some(overlay) = overlay.upgrade() {
        overlay.set_selection_info(message.into());
    }
    set_status(main, &mut state.borrow_mut(), message.to_owned());
}

fn ocr_result_payload(result: Result<String, OcrFailure>) -> (i32, String) {
    match result {
        Ok(text) if text.trim().is_empty() => (1, String::new()),
        Ok(text) => (0, text),
        Err(OcrFailure::MissingLanguagePack) => (2, String::new()),
        Err(OcrFailure::Unsupported) => (3, String::new()),
        Err(OcrFailure::Failed(message)) => {
            logging::error(message);
            (4, String::new())
        }
    }
}

fn execute_output(image: &CapturedImage, config: &CaptureConfig) -> Result<String> {
    let should_copy = matches!(
        config.completion_action,
        CompletionAction::Copy | CompletionAction::CopyAndSave
    );
    let should_save = config.auto_save
        || matches!(
            config.completion_action,
            CompletionAction::Save | CompletionAction::CopyAndSave
        );

    if should_copy {
        capture::copy_to_clipboard(image)?;
    }
    let saved = if should_save {
        Some(output::save_quick(image, config)?)
    } else {
        None
    };

    Ok(match (should_copy, saved, config.save_notification) {
        (true, Some(path), true) => format!("已复制并保存到 {}", path.display()),
        (false, Some(path), true) => format!("已保存到 {}", path.display()),
        (_, Some(_), false) => "截图已完成".to_owned(),
        (true, None, _) => "已复制到剪贴板".to_owned(),
        (false, None, _) => "截图已完成".to_owned(),
    })
}

fn refresh_annotations(overlay: &slint::Weak<OverlayWindow>, state: &Rc<RefCell<AppController>>) {
    let views = state.borrow().annotation_views();
    if let Some(overlay) = overlay.upgrade() {
        overlay.set_annotations(ModelRc::new(VecModel::from(views)));
    }
}

fn empty_annotation_model() -> ModelRc<AnnotationView> {
    ModelRc::new(VecModel::from(Vec::<AnnotationView>::new()))
}

fn finish_capture(
    overlay: &slint::Weak<OverlayWindow>,
    main: &slint::Weak<MainWindow>,
    state: &Rc<RefCell<AppController>>,
    status: String,
) {
    if let Some(overlay) = overlay.upgrade() {
        let _ = overlay.hide();
    }
    {
        let mut state = state.borrow_mut();
        state.session = None;
        set_status(main, &mut state, status);
    }
    restore_main_after_capture(main, state);
}

fn restore_main_after_capture(main: &slint::Weak<MainWindow>, state: &Rc<RefCell<AppController>>) {
    if state.borrow().restore_main_after_capture {
        show_main_window(main);
    }
}

fn set_status(main: &slint::Weak<MainWindow>, state: &mut AppController, status: String) {
    state.status = status;
    if let Some(main) = main.upgrade() {
        main.set_status_text(SharedString::from(state.status.as_str()));
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
    capturing: bool,
    restore_main_after_capture: bool,
    session: Option<CaptureSession>,
    pins: Rc<RefCell<PinRegistry>>,
    draw_style: DrawStyle,
}

impl AppController {
    fn new(config: Config, mut status: String, pins: Rc<RefCell<PinRegistry>>) -> Self {
        let hotkey = HotkeyState::new(config.hotkey.as_deref());
        if let Some(error) = hotkey.error() {
            status = format!("快捷键无效：{}", error.message());
        }
        Self {
            config,
            hotkey,
            status,
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

    fn select_area(
        &mut self,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
    ) -> Result<(slint::Image, String)> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| anyhow!("没有活动截图"))?;
        let (left, top, width, height) = normalized_selection(
            left,
            top,
            right,
            bottom,
            session.desktop.bounds.width as u32,
            session.desktop.bounds.height as u32,
        )
        .ok_or_else(|| anyhow!("选区过小"))?;
        let selected = session
            .desktop
            .crop(left, top, width, height)
            .ok_or_else(|| anyhow!("选区过小"))?;
        let image = selected.slint_image();
        session.selected = Some(selected);
        session.annotations.clear();
        Ok((image, format!("{width} × {height}")))
    }

    fn begin_annotation(&mut self, x: f32, y: f32, tool: i32) {
        let style = self.draw_style;
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let Some(selected) = session.selected.as_ref() else {
            return;
        };
        let point = clamp_point(x, y, selected);
        session.annotations.begin(tool, point, style);
    }

    fn update_annotation(&mut self, x: f32, y: f32) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let Some(selected) = session.selected.as_ref() else {
            return;
        };
        let point = clamp_point(x, y, selected);
        session.annotations.update(point);
    }

    fn finish_annotation(&mut self) {
        if let Some(session) = self.session.as_mut() {
            session.annotations.finish();
        }
    }

    fn set_color(&mut self, index: i32) {
        self.draw_style.rgba = match index {
            0 => [236, 92, 102, 255],
            1 => [74, 144, 226, 255],
            2 => [49, 163, 107, 255],
            3 => [245, 197, 66, 255],
            _ => self.draw_style.rgba,
        };
    }

    fn set_width(&mut self, radius: i32) {
        self.draw_style.radius = radius.clamp(1, 12);
    }

    fn undo(&mut self) {
        if let Some(session) = self.session.as_mut() {
            session.annotations.undo();
        }
    }

    fn redo(&mut self) {
        if let Some(session) = self.session.as_mut() {
            session.annotations.redo();
        }
    }

    fn annotation_views(&self) -> Vec<AnnotationView> {
        self.session
            .as_ref()
            .map(|session| session.annotations.views())
            .unwrap_or_default()
    }

    fn rendered_selection(&self) -> Result<CapturedImage> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| anyhow!("没有活动截图"))?;
        let selected = session
            .selected
            .as_ref()
            .ok_or_else(|| anyhow!("尚未选择截图区域"))?;
        Ok(session.annotations.render(selected))
    }

    fn original_selection(&self) -> Result<CapturedImage> {
        self.session
            .as_ref()
            .and_then(|session| session.selected.clone())
            .ok_or_else(|| anyhow!("尚未选择截图区域"))
    }
}

fn clamp_point(x: f32, y: f32, image: &CapturedImage) -> (u32, u32) {
    (
        x.clamp(0.0, image.bounds.width.saturating_sub(1) as f32) as u32,
        y.clamp(0.0, image.bounds.height.saturating_sub(1) as f32) as u32,
    )
}

fn normalized_selection(
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
    max_width: u32,
    max_height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let left = start_x.min(end_x).clamp(0.0, max_width as f32) as u32;
    let top = start_y.min(end_y).clamp(0.0, max_height as f32) as u32;
    let right = start_x.max(end_x).clamp(0.0, max_width as f32) as u32;
    let bottom = start_y.max(end_y).clamp(0.0, max_height as f32) as u32;
    let width = right.saturating_sub(left);
    let height = bottom.saturating_sub(top);
    (width > 0 && height > 0).then_some((left, top, width, height))
}

struct CaptureSession {
    desktop: CapturedImage,
    selected: Option<CapturedImage>,
    annotations: AnnotationHistory,
    _overlay: OverlayWindow,
}

#[derive(Default)]
struct PinRegistry {
    next_id: u64,
    windows: Vec<PinnedWindow>,
}

impl PinRegistry {
    #[allow(clippy::too_many_arguments)]
    fn add(
        registry: &Rc<RefCell<Self>>,
        image: CapturedImage,
        source_path: Option<PathBuf>,
        config: PinConfig,
        capture_config: CaptureConfig,
        main: slint::Weak<MainWindow>,
        app: RcWeak<RefCell<AppController>>,
    ) -> Result<()> {
        let pin = PinWindow::new()?;
        pin.set_screenshot(image.slint_image());
        pin.set_alpha_percent(config.default_opacity as i32);
        pin.set_shadow_enabled(config.shadow);
        pin.set_top_enabled(config.always_on_top);
        pin.set_wheel_zoom(config.wheel_zoom);
        pin.set_zoom_step(config.zoom_step as i32);
        pin.set_double_click_close(config.double_click_close);
        pin.set_scale_percent(100);
        pin.set_original_size_text(format!("{} × {}", image.width(), image.height()).into());
        pin.set_has_source_file(source_path.is_some());
        pin.window()
            .set_position(PhysicalPosition::new(image.bounds.left, image.bounds.top));
        pin.window()
            .set_size(PhysicalSize::new(image.width(), image.height()));

        let id = {
            let mut registry = registry.borrow_mut();
            let id = registry.next_id;
            registry.next_id += 1;
            id
        };
        let window_state = Rc::new(RefCell::new(PinnedWindowState {
            image,
            source_path,
            opacity: config.default_opacity,
            shadow: config.shadow,
            always_on_top: config.always_on_top,
            scale_percent: 100,
            zoom_step: config.zoom_step,
            capture_config,
        }));

        {
            let pin = pin.as_weak();
            let registry: RcWeak<RefCell<Self>> = Rc::downgrade(registry);
            pin.unwrap().on_close_pin(move || {
                if let Some(pin) = pin.upgrade() {
                    let _ = pin.hide();
                }
                if let Some(registry) = registry.upgrade() {
                    registry.borrow_mut().windows.retain(|item| item.id != id);
                }
            });
        }
        {
            let pin = pin.as_weak();
            pin.unwrap().on_drag_pin(move || {
                if let Some(pin) = pin.upgrade() {
                    window::drag(pin.window());
                }
            });
        }
        {
            let pin = pin.as_weak();
            let window_state = Rc::clone(&window_state);
            pin.unwrap().on_scale_pin(move |direction| {
                if let Some(pin) = pin.upgrade() {
                    let current = window_state.borrow().scale_percent;
                    let step = window_state.borrow().zoom_step as i32;
                    set_pin_scale(&pin, &window_state, current + direction.signum() * step);
                }
            });
        }
        {
            let pin = pin.as_weak();
            let window_state = Rc::clone(&window_state);
            pin.unwrap().on_set_scale(move |percent| {
                if let Some(pin) = pin.upgrade() {
                    set_pin_scale(&pin, &window_state, percent);
                }
            });
        }
        {
            let pin = pin.as_weak();
            let window_state = Rc::clone(&window_state);
            pin.unwrap().on_fit_screen(move || {
                if let Some(pin) = pin.upgrade() {
                    let state = window_state.borrow();
                    window::fit_to_work_area(
                        pin.window(),
                        state.image.width(),
                        state.image.height(),
                    );
                    let size = pin.window().size();
                    let scale =
                        ((size.width as f64 / state.image.width() as f64) * 100.0).round() as i32;
                    drop(state);
                    window_state.borrow_mut().scale_percent = scale;
                    pin.set_scale_percent(scale);
                }
            });
        }
        {
            let pin = pin.as_weak();
            let window_state = Rc::clone(&window_state);
            let main = main.clone();
            let app = app.clone();
            pin.unwrap().on_copy_image(move || {
                let result = capture::copy_to_clipboard(&window_state.borrow().image);
                report_pin_result(&main, &app, result, "已复制钉住图像");
            });
        }
        {
            let pin = pin.as_weak();
            let window_state = Rc::clone(&window_state);
            let main = main.clone();
            let app = app.clone();
            pin.unwrap().on_save_image(move || {
                let Some(pin) = pin.upgrade() else {
                    return;
                };
                let state = window_state.borrow();
                let result = output::save_as_dialog(
                    &state.image,
                    &state.capture_config.save_directory,
                    state.capture_config.format,
                    state.capture_config.jpeg_quality,
                );
                drop(state);
                match result {
                    Ok(Some(path)) => {
                        window_state.borrow_mut().source_path = Some(path.clone());
                        pin.set_has_source_file(true);
                        update_app_status(&main, &app, format!("图像已保存到 {}", path.display()));
                    }
                    Ok(None) => {}
                    Err(error) => {
                        update_app_status(&main, &app, format!("图像保存失败：{error}"));
                    }
                }
            });
        }
        {
            let pin = pin.as_weak();
            let window_state = Rc::clone(&window_state);
            pin.unwrap().on_recognize_text(move || {
                let image = window_state.borrow().image.clone();
                spawn_pin_ocr(pin.clone(), image);
            });
        }
        {
            let main = main.clone();
            let app = app.clone();
            pin.on_ocr_result(move |code, text| match code {
                0 => report_pin_result(
                    &main,
                    &app,
                    capture::copy_text_to_clipboard(text.as_str()),
                    "已识别并复制文字",
                ),
                1 => update_app_status(&main, &app, "未识别到文字".to_owned()),
                2 => update_app_status(&main, &app, "缺少中文 OCR 语言包".to_owned()),
                3 => update_app_status(&main, &app, "当前平台不支持系统 OCR".to_owned()),
                _ => update_app_status(&main, &app, "OCR 识别失败".to_owned()),
            });
        }
        {
            let pin = pin.as_weak();
            let window_state = Rc::clone(&window_state);
            pin.unwrap().on_set_opacity(move |percent| {
                if let Some(pin) = pin.upgrade() {
                    let percent = percent.clamp(25, 100) as u8;
                    window_state.borrow_mut().opacity = percent;
                    pin.set_alpha_percent(percent as i32);
                    window::set_opacity(pin.window(), percent);
                }
            });
        }
        {
            let pin = pin.as_weak();
            let window_state = Rc::clone(&window_state);
            pin.unwrap().on_set_shadow(move |enabled| {
                if let Some(pin) = pin.upgrade() {
                    window_state.borrow_mut().shadow = enabled;
                    window::set_shadow(pin.window(), enabled);
                }
            });
        }
        {
            let pin = pin.as_weak();
            let window_state = Rc::clone(&window_state);
            pin.unwrap().on_set_top(move |enabled| {
                if let Some(pin) = pin.upgrade() {
                    window_state.borrow_mut().always_on_top = enabled;
                    window::set_always_on_top(pin.window(), enabled);
                }
            });
        }
        {
            let pin = pin.as_weak();
            let window_state = Rc::clone(&window_state);
            let main = main.clone();
            let app = app.clone();
            pin.unwrap().on_replace_clipboard(move || {
                let Some(pin) = pin.upgrade() else {
                    return;
                };
                let position = pin.window().position();
                match capture::image_from_clipboard(position.x, position.y) {
                    Ok(image) => {
                        replace_pin_image(&pin, &window_state, image, None);
                        update_app_status(&main, &app, "已从剪贴板替换图像".to_owned());
                    }
                    Err(error) => {
                        update_app_status(&main, &app, format!("替换图像失败：{error}"));
                    }
                }
            });
        }
        {
            let pin = pin.as_weak();
            let window_state = Rc::clone(&window_state);
            let main = main.clone();
            let app = app.clone();
            pin.unwrap().on_replace_file(move || {
                let Some(pin) = pin.upgrade() else {
                    return;
                };
                let Some(path) = rfd::FileDialog::new()
                    .add_filter("图像", &["png", "jpg", "jpeg"])
                    .pick_file()
                else {
                    return;
                };
                let position = pin.window().position();
                match CapturedImage::from_file(&path, position.x, position.y) {
                    Ok(image) => {
                        replace_pin_image(&pin, &window_state, image, Some(path));
                        update_app_status(&main, &app, "已从文件替换图像".to_owned());
                    }
                    Err(error) => {
                        update_app_status(&main, &app, format!("替换图像失败：{error}"));
                    }
                }
            });
        }
        {
            let window_state = Rc::clone(&window_state);
            let main = main.clone();
            let app = app.clone();
            pin.on_reveal_file(move || {
                let path = window_state.borrow().source_path.clone();
                match path {
                    Some(path) => {
                        report_pin_result(
                            &main,
                            &app,
                            shell::reveal_in_folder(&path),
                            "已在文件夹中显示",
                        );
                    }
                    None => update_app_status(&main, &app, "当前图像尚未保存".to_owned()),
                }
            });
        }
        bind_pin_transform(
            &pin,
            Rc::clone(&window_state),
            main.clone(),
            app.clone(),
            PinTransform::RotateLeft,
        );
        bind_pin_transform(
            &pin,
            Rc::clone(&window_state),
            main.clone(),
            app.clone(),
            PinTransform::RotateRight,
        );
        bind_pin_transform(
            &pin,
            Rc::clone(&window_state),
            main.clone(),
            app.clone(),
            PinTransform::FlipHorizontal,
        );
        bind_pin_transform(
            &pin,
            Rc::clone(&window_state),
            main,
            app,
            PinTransform::FlipVertical,
        );

        pin.show()?;
        window::set_opacity(pin.window(), config.default_opacity);
        window::set_shadow(pin.window(), config.shadow);
        window::set_always_on_top(pin.window(), config.always_on_top);
        registry.borrow_mut().windows.push(PinnedWindow {
            id,
            ui: pin,
            state: window_state,
        });
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum PinTransform {
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
}

fn bind_pin_transform(
    pin: &PinWindow,
    state: Rc<RefCell<PinnedWindowState>>,
    main: slint::Weak<MainWindow>,
    app: RcWeak<RefCell<AppController>>,
    transform: PinTransform,
) {
    match transform {
        PinTransform::RotateLeft => {
            let pin_weak = pin.as_weak();
            pin.on_rotate_left(move || {
                if let Some(pin) = pin_weak.upgrade() {
                    let (image, source_path) = {
                        let state = state.borrow();
                        (state.image.rotate_left(), state.source_path.clone())
                    };
                    replace_pin_image(&pin, &state, image, source_path);
                    update_app_status(&main, &app, "图像已向左旋转".to_owned());
                }
            });
        }
        PinTransform::RotateRight => {
            let pin_weak = pin.as_weak();
            pin.on_rotate_right(move || {
                if let Some(pin) = pin_weak.upgrade() {
                    let (image, source_path) = {
                        let state = state.borrow();
                        (state.image.rotate_right(), state.source_path.clone())
                    };
                    replace_pin_image(&pin, &state, image, source_path);
                    update_app_status(&main, &app, "图像已向右旋转".to_owned());
                }
            });
        }
        PinTransform::FlipHorizontal => {
            let pin_weak = pin.as_weak();
            pin.on_flip_horizontal(move || {
                if let Some(pin) = pin_weak.upgrade() {
                    let (image, source_path) = {
                        let state = state.borrow();
                        (state.image.flip_horizontal(), state.source_path.clone())
                    };
                    replace_pin_image(&pin, &state, image, source_path);
                    update_app_status(&main, &app, "图像已水平翻转".to_owned());
                }
            });
        }
        PinTransform::FlipVertical => {
            let pin_weak = pin.as_weak();
            pin.on_flip_vertical(move || {
                if let Some(pin) = pin_weak.upgrade() {
                    let (image, source_path) = {
                        let state = state.borrow();
                        (state.image.flip_vertical(), state.source_path.clone())
                    };
                    replace_pin_image(&pin, &state, image, source_path);
                    update_app_status(&main, &app, "图像已垂直翻转".to_owned());
                }
            });
        }
    }
}

fn replace_pin_image(
    pin: &PinWindow,
    state: &Rc<RefCell<PinnedWindowState>>,
    image: CapturedImage,
    source_path: Option<PathBuf>,
) {
    let position = pin.window().position();
    let image = image.with_origin(position.x, position.y);
    let width = image.width();
    let height = image.height();
    {
        let mut state = state.borrow_mut();
        state.image = image.clone();
        state.source_path = source_path;
        state.scale_percent = 100;
    }
    pin.set_screenshot(image.slint_image());
    pin.set_original_size_text(format!("{width} × {height}").into());
    pin.set_scale_percent(100);
    pin.set_has_source_file(state.borrow().source_path.is_some());
    pin.window().set_size(PhysicalSize::new(width, height));
}

fn set_pin_scale(pin: &PinWindow, state: &Rc<RefCell<PinnedWindowState>>, percent: i32) {
    let percent = percent.clamp(10, 800);
    let state_ref = state.borrow();
    let width = ((state_ref.image.width() as u64 * percent as u64) / 100).max(1) as u32;
    let height = ((state_ref.image.height() as u64 * percent as u64) / 100).max(1) as u32;
    drop(state_ref);
    state.borrow_mut().scale_percent = percent;
    pin.set_scale_percent(percent);
    pin.window().set_size(PhysicalSize::new(width, height));
}

fn spawn_pin_ocr(pin: slint::Weak<PinWindow>, image: CapturedImage) {
    let width = image.width();
    let height = image.height();
    let bounds = image.bounds;
    let rgba = image.rgba_bytes();
    thread::spawn(move || {
        let result = CapturedImage::from_rgba(bounds.left, bounds.top, width, height, &rgba)
            .map_err(|error| OcrFailure::Failed(error.to_string()))
            .and_then(|image| system_engine().recognize(&image));
        let (code, text) = ocr_result_payload(result);
        let _ = pin.upgrade_in_event_loop(move |pin| {
            pin.invoke_ocr_result(code, text.into());
        });
    });
}

fn report_pin_result(
    main: &slint::Weak<MainWindow>,
    app: &RcWeak<RefCell<AppController>>,
    result: Result<()>,
    success: &str,
) {
    match result {
        Ok(()) => update_app_status(main, app, success.to_owned()),
        Err(error) => update_app_status(main, app, format!("操作失败：{error}")),
    }
}

fn update_app_status(
    main: &slint::Weak<MainWindow>,
    app: &RcWeak<RefCell<AppController>>,
    message: String,
) {
    if let Some(app) = app.upgrade() {
        set_status(main, &mut app.borrow_mut(), message);
    } else if let Some(main) = main.upgrade() {
        main.set_status_text(message.into());
    }
}

struct PinnedWindow {
    id: u64,
    #[allow(dead_code)]
    ui: PinWindow,
    #[allow(dead_code)]
    state: Rc<RefCell<PinnedWindowState>>,
}

struct PinnedWindowState {
    image: CapturedImage,
    source_path: Option<PathBuf>,
    opacity: u8,
    shadow: bool,
    always_on_top: bool,
    scale_percent: i32,
    zoom_step: u8,
    capture_config: CaptureConfig,
}

fn appearance_index(mode: AppearanceMode) -> i32 {
    match mode {
        AppearanceMode::System => 0,
        AppearanceMode::Light => 1,
        AppearanceMode::Dark => 2,
    }
}

fn appearance_from_index(index: i32) -> AppearanceMode {
    match index {
        1 => AppearanceMode::Light,
        2 => AppearanceMode::Dark,
        _ => AppearanceMode::System,
    }
}

fn completion_action_index(action: CompletionAction) -> i32 {
    match action {
        CompletionAction::Copy => 0,
        CompletionAction::Save => 1,
        CompletionAction::CopyAndSave => 2,
    }
}

fn completion_action_from_index(index: i32) -> CompletionAction {
    match index {
        1 => CompletionAction::Save,
        2 => CompletionAction::CopyAndSave,
        _ => CompletionAction::Copy,
    }
}

fn image_format_index(format: ImageFormat) -> i32 {
    match format {
        ImageFormat::Png => 0,
        ImageFormat::Jpeg => 1,
    }
}

fn image_format_from_index(index: i32) -> ImageFormat {
    if index == 1 {
        ImageFormat::Jpeg
    } else {
        ImageFormat::Png
    }
}

fn output_summary(config: &CaptureConfig) -> String {
    let action = match config.completion_action {
        CompletionAction::Copy => {
            if config.auto_save {
                "复制并自动保存"
            } else {
                "复制到剪贴板"
            }
        }
        CompletionAction::Save => "保存到文件",
        CompletionAction::CopyAndSave => "复制并保存",
    };
    let format = match config.format {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
    };
    format!("{action} · {format}")
}

#[cfg(test)]
mod tests {
    use super::normalized_selection;

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
}
