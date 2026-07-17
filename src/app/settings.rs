use std::{cell::RefCell, path::PathBuf, rc::Rc, thread};

use anyhow::{Result, anyhow};
use slint::ComponentHandle;

use super::{
    AppController, MainWindow, StatusLevel, appearance_index, refresh_main_if_available,
    set_status_level,
};
use crate::{
    config::{
        AppearanceMode, CaptureConfig, CompletionAction, Config, ImageFormat, OcrConfig,
        OcrEngineKind, PinConfig,
    },
    hotkey::{HotkeyState, validate_binding},
    logging,
    platform::{
        ocr::{AiOcrState, OcrFailure, prepare_ai},
        windows::{shell, startup},
    },
};

pub(super) fn bind(settings: &MainWindow, state: Rc<RefCell<AppController>>) {
    let main = settings.as_weak();
    {
        let main = main.clone();
        let state = Rc::clone(&state);
        settings.on_prepare_ai_ocr(move || {
            {
                let mut state = state.borrow_mut();
                state.ai_ocr_state = AiOcrState::Preparing;
                state.refresh_selected_ocr();
                set_status_level(
                    &main,
                    &mut state,
                    "正在准备 Windows AI OCR 模型...".to_owned(),
                    StatusLevel::Info,
                );
                refresh_main_if_available(&main, &state);
            }
            let main = main.clone();
            thread::spawn(move || {
                let payload = serde_json::to_string(&prepare_ai()).unwrap_or_else(|error| {
                    serde_json::to_string(&Err::<AiOcrState, _>(OcrFailure::Failed(
                        error.to_string(),
                    )))
                    .unwrap_or_default()
                });
                let _ = main.upgrade_in_event_loop(move |main| {
                    main.invoke_ai_ocr_prepared(payload.into());
                });
            });
        });
    }
    {
        let main = main.clone();
        let state = Rc::clone(&state);
        settings.on_ai_ocr_prepared(move |payload| {
            let result = serde_json::from_str::<Result<AiOcrState, OcrFailure>>(payload.as_str())
                .unwrap_or_else(|error| Err(OcrFailure::Failed(error.to_string())));
            let (ai_state, message, level) = match result {
                Ok(ai_state) => {
                    let message = ai_state.message();
                    let level = if ai_state.is_ready() {
                        StatusLevel::Success
                    } else {
                        StatusLevel::Error
                    };
                    (ai_state, message, level)
                }
                Err(OcrFailure::AiUnavailable(ai_state)) => {
                    let message = ai_state.message();
                    (ai_state, message, StatusLevel::Error)
                }
                Err(error) => {
                    let message = error.message();
                    (
                        AiOcrState::Failed(message.clone()),
                        message,
                        StatusLevel::Error,
                    )
                }
            };
            {
                let mut state = state.borrow_mut();
                state.ai_ocr_state = ai_state;
                state.refresh_selected_ocr();
                set_status_level(&main, &mut state, message, level);
                refresh_main_if_available(&main, &state);
                if let Some(main) = main.upgrade() {
                    populate(&main, &state);
                }
            }
        });
    }
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
        let main = main.clone();
        let state = Rc::clone(&state);
        settings.unwrap().on_restore_defaults(move |tab| {
            if let Some(settings) = settings.upgrade() {
                restore_settings_page(&settings, tab);
                set_status_level(
                    &main,
                    &mut state.borrow_mut(),
                    "已恢复当前页默认值，点击保存后生效".to_owned(),
                    StatusLevel::Info,
                );
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
        let main = main.clone();
        let state = Rc::clone(&state);
        settings.unwrap().on_open_save_directory(move || {
            let Some(settings) = settings.upgrade() else {
                return;
            };
            let path = PathBuf::from(settings.get_save_directory().as_str());
            let result = std::fs::create_dir_all(&path)
                .map_err(anyhow::Error::from)
                .and_then(|_| shell::open_path(&path));
            report_result(&main, &state, result, "已打开保存目录");
        });
    }
    {
        let settings = settings.as_weak();
        let main = main.clone();
        let state = Rc::clone(&state);
        settings.unwrap().on_open_log_directory(move || {
            let path = Config::log_directory();
            let result = std::fs::create_dir_all(&path)
                .map_err(anyhow::Error::from)
                .and_then(|_| shell::open_path(&path));
            report_result(&main, &state, result, "已打开日志文件夹");
        });
    }
    {
        let settings = settings.as_weak();
        let main = main.clone();
        let state = Rc::clone(&state);
        settings.unwrap().on_open_config_directory(move || {
            let path = Config::path();
            let directory = Config::directory();
            let result = if path.exists() {
                Ok(())
            } else {
                state.borrow().config.save()
            }
            .and_then(|_| shell::open_path(&directory));
            report_result(&main, &state, result, "已打开配置文件夹");
        });
    }
    {
        let main = main.clone();
        let state = Rc::clone(&state);
        settings.on_config_hint(move |visible| {
            let Some(main) = main.upgrade() else {
                return;
            };
            if visible {
                main.set_status_text("手动修改配置后需要重启应用。".into());
                main.set_status_level(StatusLevel::Info as i32);
            } else {
                let state = state.borrow();
                main.set_status_text(state.status.as_str().into());
                main.set_status_level(state.status_level as i32);
            }
        });
    }
    {
        let settings = settings.as_weak();
        let main = main.clone();
        settings.unwrap().on_clear_hotkey(move || {
            let Some(settings) = settings.upgrade() else {
                return;
            };
            let mut state = state.borrow_mut();
            match state.hotkey.set_binding(None) {
                Ok(()) => {
                    settings.set_hotkey_text("".into());
                    settings.set_hotkey_status(0);
                    settings.set_hotkey_status_tip("".into());
                    set_status_level(
                        &main,
                        &mut state,
                        "快捷键已注销，点击保存后永久生效".to_owned(),
                        StatusLevel::Info,
                    );
                }
                Err(error) => set_status_level(
                    &main,
                    &mut state,
                    format!("快捷键注销失败：{}", error.message()),
                    StatusLevel::Error,
                ),
            }
        });
    }
}

pub(super) fn populate(settings: &MainWindow, state: &AppController) {
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
    settings.set_ocr_engine(ocr_engine_index(state.config.ocr.engine));
    settings.set_ocr_minimum_confidence(state.config.ocr.minimum_confidence as i32);
    settings.set_pin_opacity(state.config.pin.default_opacity as i32);
    settings.set_pin_shadow(state.config.pin.shadow);
    settings.set_pin_always_on_top(state.config.pin.always_on_top);
    settings.set_pin_wheel_zoom(state.config.pin.wheel_zoom);
    settings.set_pin_zoom_step(state.config.pin.zoom_step as i32);
    settings.set_pin_double_click_close(state.config.pin.double_click_close);
    settings.set_hotkey_text(state.config.hotkey.clone().unwrap_or_default().into());
    set_hotkey_indicator(settings, state);
    settings.set_version_text(format!("版本 v{}", env!("CARGO_PKG_VERSION")).into());
    settings.set_build_text(build_information().into());
    settings.set_log_path(
        Config::log_directory()
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    settings.set_config_path(Config::directory().to_string_lossy().into_owned().into());
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
        set_status_level(
            main,
            &mut state.borrow_mut(),
            format!("设置无效：{error}"),
            StatusLevel::Error,
        );
        return;
    }
    if let Some(binding) = candidate.hotkey.as_deref()
        && let Err(error) = validate_binding(binding)
    {
        settings.set_hotkey_status(2);
        settings.set_hotkey_status_tip(error.message().into());
        set_status_level(
            main,
            &mut state.borrow_mut(),
            format!("快捷键无效：{}", error.message()),
            StatusLevel::Error,
        );
        return;
    }

    let old = state.borrow().config.clone();
    let result = {
        let mut state = state.borrow_mut();
        apply_transaction(&old, &candidate, &mut state.hotkey)
    };
    if let Err(error) = result {
        set_status_level(
            main,
            &mut state.borrow_mut(),
            error.to_string(),
            StatusLevel::Error,
        );
        logging::error(error.to_string());
        return;
    }

    {
        let mut state = state.borrow_mut();
        state.config = candidate;
        state.refresh_selected_ocr();
        set_status_level(
            main,
            &mut state,
            "设置已保存".to_owned(),
            StatusLevel::Success,
        );
        refresh_main_if_available(main, &state);
        populate(&settings, &state);
    }
    logging::info("settings saved");
}

fn apply_transaction(old: &Config, candidate: &Config, hotkey: &mut HotkeyState) -> Result<()> {
    startup::set_enabled(candidate.launch_at_startup)
        .map_err(|error| anyhow!("开机启动设置失败：{error}"))?;

    if let Err(error) = hotkey.set_binding(candidate.hotkey.as_deref()) {
        let rollback = startup::set_enabled(old.launch_at_startup).err();
        return Err(with_rollback(
            format!("快捷键设置失败：{}", error.message()),
            rollback.map(|error| format!("恢复开机启动失败：{error}")),
        ));
    }

    if let Err(error) = candidate.save() {
        let mut rollback_errors = Vec::new();
        if let Err(error) = startup::set_enabled(old.launch_at_startup) {
            rollback_errors.push(format!("恢复开机启动失败：{error}"));
        }
        if let Err(error) = hotkey.set_binding(old.hotkey.as_deref()) {
            rollback_errors.push(format!("恢复快捷键失败：{}", error.message()));
        }
        let rollback = (!rollback_errors.is_empty()).then(|| rollback_errors.join("；"));
        return Err(with_rollback(format!("配置保存失败：{error}"), rollback));
    }

    Ok(())
}

fn with_rollback(message: String, rollback: Option<String>) -> anyhow::Error {
    match rollback {
        Some(rollback) => anyhow!("{message}；回滚失败：{rollback}"),
        None => anyhow!(message),
    }
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
        ocr: OcrConfig {
            engine: ocr_engine_from_index(settings.get_ocr_engine()),
            minimum_confidence: settings.get_ocr_minimum_confidence().clamp(0, 100) as u8,
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
            settings.set_ocr_engine(ocr_engine_index(defaults.ocr.engine));
            settings.set_ocr_minimum_confidence(defaults.ocr.minimum_confidence as i32);
        }
        3 => {
            settings.set_pin_opacity(defaults.pin.default_opacity as i32);
            settings.set_pin_shadow(defaults.pin.shadow);
            settings.set_pin_always_on_top(defaults.pin.always_on_top);
            settings.set_pin_wheel_zoom(defaults.pin.wheel_zoom);
            settings.set_pin_zoom_step(defaults.pin.zoom_step as i32);
            settings.set_pin_double_click_close(defaults.pin.double_click_close);
        }
        4 => {
            settings.set_hotkey_text(defaults.hotkey.as_deref().unwrap_or_default().into());
            settings.set_hotkey_status(0);
            settings.set_hotkey_status_tip("".into());
        }
        _ => {}
    }
}

fn ocr_engine_index(engine: OcrEngineKind) -> i32 {
    match engine {
        OcrEngineKind::System => 0,
        OcrEngineKind::WindowsAi => 1,
    }
}

fn ocr_engine_from_index(index: i32) -> OcrEngineKind {
    if index == 1 {
        OcrEngineKind::WindowsAi
    } else {
        OcrEngineKind::System
    }
}

fn report_result(
    main: &slint::Weak<MainWindow>,
    state: &Rc<RefCell<AppController>>,
    result: Result<()>,
    success: &str,
) {
    match result {
        Ok(()) => set_status_level(
            main,
            &mut state.borrow_mut(),
            success.to_owned(),
            StatusLevel::Success,
        ),
        Err(error) => {
            logging::error(error.to_string());
            set_status_level(
                main,
                &mut state.borrow_mut(),
                format!("操作失败：{error}"),
                StatusLevel::Error,
            );
        }
    }
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

#[cfg(test)]
mod tests {
    use super::with_rollback;

    #[test]
    fn rollback_error_is_preserved() {
        let error = with_rollback("应用失败".to_owned(), Some("恢复失败".to_owned()));
        assert_eq!(error.to_string(), "应用失败；回滚失败：恢复失败");
    }
}
