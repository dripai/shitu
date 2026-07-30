use std::{
    cell::RefCell,
    ops::{Deref, DerefMut},
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use shi_foundation::i18n;
use slint::{
    ComponentHandle, ModelRc, PhysicalPosition, PhysicalSize, SharedString, Timer, TimerMode,
    VecModel,
};

use crate::{
    MainWindow, PreferencesDialog, RecordingTray, SelectionWindow, TargetIndicatorWindow,
    application::{ApplicationState, Command, Event, RecorderHandle, RecordingOptions},
    config::{Config, LanguageMode, OutputFormat},
    domain::{
        AudioSourceKind as SourceKind, Bounds, MonitorCandidates, RecordingTarget, WindowCandidates,
    },
    platform::{
        begin_window_drag, configure_visual_overlay, desktop_integration, recording_backend,
        target_selection,
    },
    ports::RecordingCapabilities,
};

use super::hotkeys::{
    RecordingHotkeys, ShortcutIssue, display_shortcut, shortcut_from_key_event,
    skip_conflicting_shortcuts,
};

struct UiState {
    application: ApplicationState,
    selector: Option<SelectionWindow>,
    target_indicator: Option<TargetIndicatorWindow>,
    target_indicator_bounds: Option<Bounds>,
    target_indicator_generation: u64,
    candidates: Option<WindowCandidates>,
    monitors: Option<MonitorCandidates>,
    selected_screen: Option<Bounds>,
    selection_desktop: Option<Bounds>,
    hotkey_issue: Option<ShortcutIssue>,
}

impl Deref for UiState {
    type Target = ApplicationState;

    fn deref(&self) -> &Self::Target {
        &self.application
    }
}

impl DerefMut for UiState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.application
    }
}

pub(crate) fn run() -> Result<(), slint::PlatformError> {
    shi_foundation::i18n::prepare(LanguageMode::English);
    let config_result = Config::load();
    shi_foundation::i18n::prepare(
        config_result
            .as_ref()
            .map(|config| config.language)
            .unwrap_or(LanguageMode::English),
    );
    let (mut config, load_error) = match config_result {
        Ok(config) => (config, None),
        Err(error) => (Config::default(), Some(error.to_string())),
    };
    let capabilities = recording_backend().capabilities();
    normalize_recording_capabilities(&mut config, capabilities);
    let main = MainWindow::new()?;
    let language_error = shi_foundation::i18n::apply(config.language).err();
    let tray = RecordingTray::new()?;
    let preferences = PreferencesDialog::new()?;
    preferences.set_version_text(format!("v{}", env!("CARGO_PKG_VERSION")).into());
    preferences.set_build_text(build_information().into());
    main.set_system_audio_available(capabilities.system_audio);
    main.set_microphone_available(capabilities.microphone);
    preferences.set_system_audio_available(capabilities.system_audio);
    preferences.set_microphone_available(capabilities.microphone);
    preferences.set_highlight_clicks_available(capabilities.highlight_clicks);
    apply_config(&main, &config);
    apply_shortcut_labels(&main, &tray, &config);
    let initial_hotkeys = config.hotkeys();
    if let Some(error) = load_error {
        set_status(
            &main,
            format!(
                "{}: {error}",
                shi_foundation::i18n::text("配置未加载", "Settings were not loaded")
            ),
            true,
        );
    } else if let Some(error) = language_error {
        set_status(
            &main,
            format!(
                "{}: {error}",
                shi_foundation::i18n::text("语言初始化失败", "Failed to initialize the language")
            ),
            true,
        );
    }

    let state = Rc::new(RefCell::new(UiState {
        application: ApplicationState::new(config),
        selector: None,
        target_indicator: None,
        target_indicator_bounds: None,
        target_indicator_generation: 0,
        candidates: None,
        monitors: None,
        selected_screen: None,
        selection_desktop: None,
        hotkey_issue: None,
    }));
    main.show()?;
    if let Err(error) = refresh_screens(&main, &state) {
        set_status(&main, error.to_string(), true);
    }
    let mut startup_shortcut_notice = None;
    let (hotkeys, hotkey_issue) = match RecordingHotkeys::new() {
        Ok(mut hotkeys) => {
            hotkeys.bind_events(main.as_weak());
            match skip_conflicting_shortcuts(initial_hotkeys.clone(), |configured| {
                hotkeys.reconfigure(configured)
            }) {
                Ok((registered, conflicts)) => {
                    let issue = if registered != initial_hotkeys {
                        let config = {
                            let mut state = state.borrow_mut();
                            state.config.set_hotkeys(registered);
                            state.config.clone()
                        };
                        apply_shortcut_labels(&main, &tray, &config);
                        config.save().err().map(|error| ShortcutIssue {
                            action: None,
                            message: format!(
                                "{}: {error}",
                                i18n::text(
                                    "保存已清除的冲突快捷键失败",
                                    "Failed to save the cleared conflicting shortcuts"
                                )
                            ),
                            is_conflict: false,
                        })
                    } else {
                        None
                    };
                    if !conflicts.is_empty() {
                        startup_shortcut_notice = Some(format!(
                            "{}: {}",
                            i18n::text(
                                "冲突的快捷键已留空",
                                "Conflicting shortcuts were left unset"
                            ),
                            conflicts
                                .into_iter()
                                .map(|conflict| conflict.message)
                                .collect::<Vec<_>>()
                                .join(i18n::text("；", "; "))
                        ));
                    }
                    (Some(hotkeys), issue)
                }
                Err(issue) => (Some(hotkeys), Some(issue)),
            }
        }
        Err(error) => (
            None,
            Some(ShortcutIssue {
                action: None,
                message: error.to_string(),
                is_conflict: false,
            }),
        ),
    };
    let hotkeys = Rc::new(RefCell::new(hotkeys));
    if let Some(issue) = hotkey_issue {
        set_status(
            &main,
            format!(
                "{}: {}",
                i18n::text("快捷键不可用", "Shortcuts are unavailable"),
                issue.message
            ),
            true,
        );
        state.borrow_mut().hotkey_issue = Some(issue);
    } else if let Some(notice) = startup_shortcut_notice {
        set_status(&main, notice, false);
    }

    bind_callbacks(&main, Rc::clone(&state));
    bind_preferences(
        &main,
        &tray,
        &preferences,
        Rc::clone(&state),
        Rc::clone(&hotkeys),
    );
    bind_tray(&tray, main.as_weak());

    let event_timer = Timer::default();
    {
        let main = main.as_weak();
        let preferences = preferences.as_weak();
        let state = Rc::clone(&state);
        event_timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
            if let Some(main) = main.upgrade() {
                handle_recorder_events(&main, &state);
                if let Err(error) = refresh_target_indicator(&state) {
                    let _ = clear_target_indicator(&state);
                    set_status(&main, error.to_string(), true);
                }
                if let Some(preferences) = preferences.upgrade() {
                    preferences.set_recording_active(
                        state.borrow().recorder.is_some() || main.get_recording_state() != 0,
                    );
                }
            }
        });
    }

    tray.show()?;
    let result = slint::run_event_loop();
    drop((preferences, hotkeys));
    result
}

fn apply_config(main: &MainWindow, config: &Config) {
    main.set_source_mode(config.source_mode as i32);
    main.set_quality_preset(config.quality_preset as i32);
    main.set_frame_rate(config.frame_rate as i32);
    main.set_output_format(config.output_format.index());
    main.set_system_audio(config.system_audio);
    main.set_microphone(config.microphone);
    main.set_show_cursor(config.show_cursor);
    main.set_highlight_clicks(config.highlight_clicks);
    main.set_countdown_seconds(config.countdown_seconds as i32);
    main.set_auto_minimize_after_start(config.auto_minimize_after_start);
    main.set_save_directory(config.save_directory.to_string_lossy().into_owned().into());
}

fn apply_shortcut_labels(main: &MainWindow, tray: &RecordingTray, config: &Config) {
    let start = display_shortcut(config.start_hotkey.as_deref());
    let pause = display_shortcut(config.pause_hotkey.as_deref());
    let stop = display_shortcut(config.stop_hotkey.as_deref());
    main.set_start_shortcut_label(start.clone().into());
    main.set_pause_shortcut_label(pause.clone().into());
    main.set_stop_shortcut_label(stop.clone().into());
    tray.set_start_shortcut_label(start.into());
    tray.set_pause_shortcut_label(pause.into());
    tray.set_stop_shortcut_label(stop.into());
}

fn bind_tray(tray: &RecordingTray, main: slint::Weak<MainWindow>) {
    {
        let main = main.clone();
        tray.on_restore_window(move || restore_main_window(&main));
    }
    {
        let main = main.clone();
        tray.on_start_recording(move || {
            if let Some(main) = main.upgrade() {
                main.invoke_start_recording();
            }
        });
    }
    {
        let main = main.clone();
        tray.on_pause_recording(move || {
            if let Some(main) = main.upgrade() {
                main.invoke_pause_recording();
            }
        });
    }
    {
        let main = main.clone();
        tray.on_stop_recording(move || {
            if let Some(main) = main.upgrade() {
                main.invoke_stop_recording();
            }
        });
    }
    {
        let main = main.clone();
        tray.on_open_preferences(move || {
            if let Some(main) = main.upgrade() {
                main.invoke_open_preferences();
            }
        });
    }
    {
        let main = main.clone();
        tray.on_open_output_directory(move || {
            if let Some(main) = main.upgrade() {
                main.invoke_open_output_directory();
            }
        });
    }
    tray.on_quit_application(move || {
        if let Some(main) = main.upgrade() {
            main.invoke_quit_application();
        }
    });
}

fn bind_preferences(
    main: &MainWindow,
    tray: &RecordingTray,
    preferences: &PreferencesDialog,
    state: Rc<RefCell<UiState>>,
    hotkeys: Rc<RefCell<Option<RecordingHotkeys>>>,
) {
    {
        let main = main.as_weak();
        let preferences = preferences.as_weak();
        let state = Rc::clone(&state);
        main.unwrap().on_open_preferences(move || {
            let (Some(main), Some(preferences)) = (main.upgrade(), preferences.upgrade()) else {
                return;
            };
            let mut draft = state.borrow().config.clone();
            update_config_from_main(&main, &mut draft);
            sync_preferences(
                &preferences,
                &draft,
                state.borrow().recorder.is_some() || main.get_recording_state() != 0,
            );
            if let Some(issue) = state.borrow().hotkey_issue.as_ref() {
                show_shortcut_issue(&preferences, issue);
            }
            preferences.window().set_minimized(false);
            let _ = preferences.show();
            preferences.window().request_redraw();
        });
    }
    {
        let main = main.as_weak();
        let preferences = preferences.as_weak();
        let state = Rc::clone(&state);
        preferences.unwrap().on_cancel_settings(move || {
            let (Some(main), Some(preferences)) = (main.upgrade(), preferences.upgrade()) else {
                return;
            };
            match restore_saved_language(&main, &preferences, &state) {
                Ok(()) => {
                    set_status(
                        &main,
                        i18n::text("已取消首选项修改", "Preferences changes discarded"),
                        false,
                    );
                    let _ = preferences.hide();
                }
                Err(error) => {
                    preferences.set_status_text(
                        format!(
                            "{}: {error}",
                            i18n::text("设置恢复失败", "Failed to restore settings")
                        )
                        .into(),
                    );
                    preferences.set_status_error(true);
                }
            }
        });
    }
    {
        let main = main.as_weak();
        let preferences = preferences.as_weak();
        let state = Rc::clone(&state);
        preferences.unwrap().on_reset_settings(move || {
            let (Some(main), Some(preferences)) = (main.upgrade(), preferences.upgrade()) else {
                return;
            };
            let mut defaults = Config::default();
            normalize_recording_capabilities(&mut defaults, recording_backend().capabilities());
            if let Err(error) = i18n::apply(defaults.language) {
                preferences.set_status_text(
                    format!(
                        "{}: {error}",
                        i18n::text("语言切换失败", "Failed to change the language")
                    )
                    .into(),
                );
                preferences.set_status_error(true);
                return;
            }
            sync_preferences(&preferences, &defaults, false);
            preferences.set_status_text(
                i18n::text(
                    "已恢复默认值，保存后生效",
                    "Defaults restored; save to apply them",
                )
                .into(),
            );
            if let Err(error) = refresh_screens(&main, &state) {
                set_status(&main, error.to_string(), true);
            }
        });
    }
    {
        let main = main.as_weak();
        let preferences = preferences.as_weak();
        let state = Rc::clone(&state);
        preferences.unwrap().on_preview_language(move |index| {
            let (Some(main), Some(preferences)) = (main.upgrade(), preferences.upgrade()) else {
                return;
            };
            match i18n::apply(language_from_index(index)) {
                Ok(()) => {
                    if let Err(error) = refresh_screens(&main, &state) {
                        set_status(&main, error.to_string(), true);
                    } else {
                        set_status(
                            &main,
                            i18n::text("正在预览语言", "Language preview active"),
                            false,
                        );
                    }
                    preferences.set_status_text(
                        i18n::text(
                            "语言已预览，保存后生效",
                            "Language previewed; save to apply it",
                        )
                        .into(),
                    );
                    preferences.set_status_error(false);
                }
                Err(error) => {
                    preferences.set_status_text(
                        format!(
                            "{}: {error}",
                            i18n::text("语言切换失败", "Failed to change the language")
                        )
                        .into(),
                    );
                    preferences.set_status_error(true);
                }
            }
        });
    }
    {
        let preferences = preferences.as_weak();
        preferences.unwrap().on_choose_output_directory(move || {
            let Some(preferences) = preferences.upgrade() else {
                return;
            };
            let current = PathBuf::from(preferences.get_save_directory().to_string());
            if let Some(directory) = rfd::FileDialog::new()
                .set_title(i18n::text(
                    "选择拾屏保存目录",
                    "Select the ShiPing save folder",
                ))
                .set_directory(current)
                .pick_folder()
            {
                preferences.set_save_directory(directory.to_string_lossy().into_owned().into());
                preferences.set_status_text(
                    i18n::text(
                        "保存目录将在保存后生效",
                        "The save folder will change after saving",
                    )
                    .into(),
                );
                preferences.set_status_error(false);
            }
        });
    }
    {
        let preferences = preferences.as_weak();
        preferences.unwrap().on_open_output_directory(move || {
            let Some(preferences) = preferences.upgrade() else {
                return;
            };
            let directory = PathBuf::from(preferences.get_save_directory().to_string());
            let result = if directory.as_os_str().is_empty() {
                Err(anyhow!(i18n::text(
                    "保存目录不能为空",
                    "The save folder cannot be empty"
                )))
            } else {
                std::fs::create_dir_all(&directory)
                    .with_context(|| {
                        format!(
                            "{}: {}",
                            i18n::text("创建保存目录失败", "Failed to create the save folder"),
                            directory.display()
                        )
                    })
                    .and_then(|_| desktop_integration().open_path(&directory))
            };
            if let Err(error) = result {
                preferences.set_status_text(error.to_string().into());
                preferences.set_status_error(true);
            }
        });
    }
    {
        let preferences = preferences.as_weak();
        preferences.unwrap().on_shortcut_captured(
            move |action, text, control, alt, shift, meta| {
                let Some(preferences) = preferences.upgrade() else {
                    return;
                };
                clear_shortcut_errors(&preferences);
                match shortcut_from_key_event(&text, control, alt, shift, meta) {
                    Ok(shortcut) => {
                        set_shortcut_value(&preferences, action, display_shortcut(Some(&shortcut)));
                        preferences.set_status_text(
                            i18n::text(
                                "快捷键将在保存后生效",
                                "The shortcut will change after saving",
                            )
                            .into(),
                        );
                        preferences.set_status_error(false);
                    }
                    Err(mut issue) => {
                        issue.action = usize::try_from(action).ok();
                        show_shortcut_issue(&preferences, &issue);
                    }
                }
            },
        );
    }
    {
        let main = main.as_weak();
        let tray = tray.as_weak();
        let preferences = preferences.as_weak();
        let state = Rc::clone(&state);
        preferences.unwrap().on_save_settings(move || {
            let (Some(main), Some(tray), Some(preferences)) =
                (main.upgrade(), tray.upgrade(), preferences.upgrade())
            else {
                return;
            };
            clear_shortcut_errors(&preferences);
            match apply_preferences(&main, &tray, &preferences, &state, &hotkeys) {
                Ok(()) => {
                    set_status(&main, "", false);
                    let _ = preferences.hide();
                }
                Err(mut issue) => {
                    if let Err(error) = restore_saved_language(&main, &preferences, &state) {
                        issue.message.push_str(&format!(
                            "{}{}: {error}",
                            i18n::text("；", "; "),
                            i18n::text("设置恢复失败", "Failed to restore settings")
                        ));
                    }
                    show_shortcut_issue(&preferences, &issue);
                }
            }
        });
    }
}

fn restore_saved_language(
    main: &MainWindow,
    preferences: &PreferencesDialog,
    state: &Rc<RefCell<UiState>>,
) -> std::result::Result<(), String> {
    let language = state.borrow().config.language;
    i18n::apply(language).map_err(|error| error.to_string())?;
    preferences.set_language_mode(language_index(language));
    refresh_screens(main, state).map_err(|error| error.to_string())
}

fn sync_preferences(preferences: &PreferencesDialog, config: &Config, recording_active: bool) {
    preferences.set_recording_active(recording_active);
    preferences.set_language_mode(language_index(config.language));
    preferences.set_auto_minimize_after_start(config.auto_minimize_after_start);
    preferences.set_open_directory_after_stop(config.open_directory_after_stop);
    preferences.set_countdown_seconds(config.countdown_seconds as i32);
    preferences.set_save_directory(config.save_directory.to_string_lossy().into_owned().into());
    preferences.set_quality_preset(config.quality_preset as i32);
    preferences.set_frame_rate(config.frame_rate as i32);
    preferences.set_output_format(config.output_format.index());
    preferences.set_system_audio(config.system_audio);
    preferences.set_microphone(config.microphone);
    preferences.set_show_cursor(config.show_cursor);
    preferences.set_highlight_clicks(config.highlight_clicks);
    preferences.set_start_shortcut(display_shortcut(config.start_hotkey.as_deref()).into());
    preferences.set_pause_shortcut(display_shortcut(config.pause_hotkey.as_deref()).into());
    preferences.set_stop_shortcut(display_shortcut(config.stop_hotkey.as_deref()).into());
    clear_shortcut_errors(preferences);
    preferences.set_status_text("".into());
    preferences.set_status_error(false);
}

fn apply_preferences(
    main: &MainWindow,
    tray: &RecordingTray,
    preferences: &PreferencesDialog,
    state: &Rc<RefCell<UiState>>,
    hotkeys: &Rc<RefCell<Option<RecordingHotkeys>>>,
) -> std::result::Result<(), ShortcutIssue> {
    if state.borrow().recorder.is_some() || main.get_recording_state() != 0 {
        return Err(ShortcutIssue {
            action: None,
            message: i18n::text(
                "录制期间不能保存首选项",
                "Preferences cannot be saved while recording",
            )
            .to_owned(),
            is_conflict: false,
        });
    }

    let save_directory = PathBuf::from(preferences.get_save_directory().to_string());
    if save_directory.as_os_str().is_empty() {
        return Err(ShortcutIssue {
            action: None,
            message: i18n::text("保存目录不能为空", "The save folder cannot be empty").to_owned(),
            is_conflict: false,
        });
    }

    let old_config = state.borrow().config.clone();
    let mut new_config = old_config.clone();
    new_config.language = language_from_index(preferences.get_language_mode());
    new_config.auto_minimize_after_start = preferences.get_auto_minimize_after_start();
    new_config.open_directory_after_stop = preferences.get_open_directory_after_stop();
    new_config.countdown_seconds = preferences.get_countdown_seconds().clamp(0, 10) as u8;
    new_config.save_directory = save_directory;
    new_config.quality_preset = preferences.get_quality_preset().clamp(0, 3) as u8;
    new_config.frame_rate = preferences.get_frame_rate().clamp(0, 1) as u8;
    new_config.output_format = OutputFormat::from_index(preferences.get_output_format());
    new_config.system_audio =
        new_config.output_format.supports_audio() && preferences.get_system_audio();
    new_config.microphone =
        new_config.output_format.supports_audio() && preferences.get_microphone();
    new_config.show_cursor = preferences.get_show_cursor();
    new_config.highlight_clicks = preferences.get_highlight_clicks();
    normalize_recording_capabilities(&mut new_config, recording_backend().capabilities());

    if let Err(error) = i18n::apply(new_config.language) {
        return Err(ShortcutIssue {
            action: None,
            message: format!(
                "{}: {error}",
                i18n::text("语言切换失败", "Failed to change the language")
            ),
            is_conflict: false,
        });
    }

    let requested = preference_hotkeys(preferences);
    let canonical = {
        let mut hotkeys = hotkeys.borrow_mut();
        match hotkeys.as_mut() {
            Some(hotkeys) => hotkeys.reconfigure(requested)?,
            None if requested.iter().all(Option::is_none) => requested,
            None => {
                return Err(ShortcutIssue {
                    action: None,
                    message: i18n::text(
                        "全局快捷键管理器不可用；可以清除全部快捷键后保存其他设置",
                        "The global shortcut manager is unavailable; clear all shortcuts to save the other settings"
                    )
                    .to_owned(),
                    is_conflict: false,
                });
            }
        }
    };
    new_config.set_hotkeys(canonical);

    if let Err(error) = new_config.save() {
        let rollback_message = match hotkeys.borrow_mut().as_mut() {
            Some(hotkeys) => match hotkeys.reconfigure(old_config.hotkeys()) {
                Ok(_) => String::new(),
                Err(issue) => {
                    let disabled = hotkeys.reconfigure([None, None, None]);
                    format!(
                        "{}{}: {}{}",
                        i18n::text("；", "; "),
                        i18n::text(
                            "旧快捷键恢复失败",
                            "Failed to restore the previous shortcuts"
                        ),
                        issue.message,
                        if disabled.is_ok() {
                            i18n::text("；已停用本次快捷键", "; the new shortcuts were disabled")
                        } else {
                            i18n::text(
                                "；停用本次快捷键也失败",
                                "; disabling the new shortcuts also failed",
                            )
                        }
                    )
                }
            },
            None => String::new(),
        };
        return Err(ShortcutIssue {
            action: None,
            message: format!(
                "{}: {error}{rollback_message}",
                i18n::text("保存配置失败", "Failed to save settings")
            ),
            is_conflict: false,
        });
    }

    state.borrow_mut().config = new_config.clone();
    state.borrow_mut().hotkey_issue = None;
    apply_config(main, &new_config);
    apply_shortcut_labels(main, tray, &new_config);
    sync_preferences(preferences, &new_config, false);
    Ok(())
}

fn preference_hotkeys(preferences: &PreferencesDialog) -> [Option<String>; 3] {
    [
        configured_shortcut(preferences.get_start_shortcut().as_str()),
        configured_shortcut(preferences.get_pause_shortcut().as_str()),
        configured_shortcut(preferences.get_stop_shortcut().as_str()),
    ]
}

fn configured_shortcut(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn language_index(language: LanguageMode) -> i32 {
    match language {
        LanguageMode::Chinese => 1,
        LanguageMode::English | LanguageMode::System => 0,
    }
}

fn language_from_index(index: i32) -> LanguageMode {
    if index == 1 {
        LanguageMode::Chinese
    } else {
        LanguageMode::English
    }
}

fn set_shortcut_value(preferences: &PreferencesDialog, action: i32, value: String) {
    match action {
        0 => {
            preferences.set_start_shortcut(value.into());
        }
        1 => {
            preferences.set_pause_shortcut(value.into());
        }
        2 => {
            preferences.set_stop_shortcut(value.into());
        }
        _ => {}
    }
}

fn clear_shortcut_errors(preferences: &PreferencesDialog) {
    preferences.set_start_shortcut_error("".into());
    preferences.set_pause_shortcut_error("".into());
    preferences.set_stop_shortcut_error("".into());
}

fn show_shortcut_issue(preferences: &PreferencesDialog, issue: &ShortcutIssue) {
    clear_shortcut_errors(preferences);
    match issue.action {
        Some(0) => preferences.set_start_shortcut_error(issue.message.clone().into()),
        Some(1) => preferences.set_pause_shortcut_error(issue.message.clone().into()),
        Some(2) => preferences.set_stop_shortcut_error(issue.message.clone().into()),
        _ => {}
    }
    preferences.set_status_text(issue.message.clone().into());
    preferences.set_status_error(true);
}

fn restore_main_window(main: &slint::Weak<MainWindow>) {
    let Some(main) = main.upgrade() else { return };
    if let Err(error) = restore_main_window_handle(&main) {
        set_status(&main, error.to_string(), true);
    }
}

fn restore_main_window_handle(main: &MainWindow) -> Result<()> {
    main.window().set_minimized(false);
    main.show().context(i18n::text(
        "无法恢复拾屏主窗口",
        "Could not restore the ShiPing window",
    ))?;
    main.window().request_redraw();
    Ok(())
}

fn bind_callbacks(main: &MainWindow, state: Rc<RefCell<UiState>>) {
    {
        let main = main.as_weak();
        let state = Rc::clone(&state);
        main.unwrap().on_refresh_screens(move || {
            let Some(main) = main.upgrade() else { return };
            if let Err(error) = refresh_screens(&main, &state) {
                set_status(&main, error.to_string(), true);
            }
        });
    }
    {
        let main = main.as_weak();
        let state = Rc::clone(&state);
        main.unwrap().on_screen_selected(move |index| {
            let Some(main) = main.upgrade() else { return };
            if let Err(error) = select_screen(&main, &state, index) {
                set_status(&main, error.to_string(), true);
            }
        });
    }
    {
        let main = main.as_weak();
        let state = Rc::clone(&state);
        main.unwrap().on_start_recording(move || {
            let Some(main) = main.upgrade() else { return };
            if let Err(error) = begin_countdown(&main, &state) {
                set_status(&main, error.to_string(), true);
            }
        });
    }
    {
        let main = main.as_weak();
        let state = Rc::clone(&state);
        main.unwrap().on_pause_recording(move || {
            if let Some(recorder) = state.borrow().recorder.as_ref() {
                recorder.send(Command::TogglePause);
            } else if let Some(main) = main.upgrade() {
                set_status(
                    &main,
                    i18n::text("当前没有可暂停的录制", "There is no recording to pause"),
                    true,
                );
            }
        });
    }
    {
        let main = main.as_weak();
        let state = Rc::clone(&state);
        main.unwrap().on_stop_recording(move || {
            let Some(main) = main.upgrade() else { return };
            if state.borrow().recorder.is_some() {
                if let Some(recorder) = state.borrow().recorder.as_ref() {
                    recorder.send(Command::Stop);
                }
                let clear_result = clear_target_indicator(&state);
                let restore_result = restore_main_window_handle(&main);
                match clear_result.and(restore_result) {
                    Ok(()) => set_status(
                        &main,
                        i18n::text("正在完成录制文件...", "Finalizing the recording file..."),
                        false,
                    ),
                    Err(error) => set_status(&main, error.to_string(), true),
                }
            } else if main.get_recording_state() == 3 {
                {
                    let mut state = state.borrow_mut();
                    state.countdown_token = state.countdown_token.wrapping_add(1);
                    state.pending_options = None;
                }
                main.set_recording_state(0);
                main.set_elapsed_text("00:00:00".into());
                let clear_result = clear_target_indicator(&state);
                let restore_result = restore_main_window_handle(&main);
                match clear_result.and(restore_result) {
                    Ok(()) => set_status(
                        &main,
                        i18n::text("已取消开始录制", "Recording start canceled"),
                        false,
                    ),
                    Err(error) => set_status(&main, error.to_string(), true),
                }
            }
        });
    }
    {
        let main = main.as_weak();
        main.unwrap().on_begin_window_drag(move || {
            if let Some(main) = main.upgrade()
                && let Err(error) = begin_window_drag(main.window())
            {
                set_status(&main, error.to_string(), true);
            }
        });
    }
    {
        let main = main.as_weak();
        let state = Rc::clone(&state);
        main.unwrap().on_choose_source(move || {
            let Some(main) = main.upgrade() else { return };
            let mode = main.get_source_mode();
            if let Err(error) = open_target_selector(&main, &state, mode) {
                main.set_source_mode(state.borrow().config.source_mode as i32);
                let _ = main.show();
                set_status(&main, error.to_string(), true);
            }
        });
    }
    {
        let main = main.as_weak();
        let state = Rc::clone(&state);
        main.unwrap().on_choose_output_directory(move || {
            let Some(main) = main.upgrade() else { return };
            if state.borrow().recorder.is_some() || main.get_recording_state() != 0 {
                set_status(
                    &main,
                    i18n::text(
                        "录制期间不能更改保存目录",
                        "The save folder cannot be changed while recording",
                    ),
                    true,
                );
                return;
            }
            let current = state.borrow().config.save_directory.clone();
            let Some(directory) = rfd::FileDialog::new()
                .set_title(i18n::text(
                    "选择拾屏保存目录",
                    "Select the ShiPing save folder",
                ))
                .set_directory(current)
                .pick_folder()
            else {
                return;
            };
            state.borrow_mut().config.save_directory = directory.clone();
            main.set_save_directory(directory.to_string_lossy().into_owned().into());
            match state.borrow().config.save() {
                Ok(()) => set_status(
                    &main,
                    i18n::text("保存目录已更新", "Save folder updated"),
                    false,
                ),
                Err(error) => set_status(
                    &main,
                    format!(
                        "{}: {error}",
                        i18n::text("保存配置失败", "Failed to save settings")
                    ),
                    true,
                ),
            }
        });
    }
    {
        let main = main.as_weak();
        let state = Rc::clone(&state);
        main.unwrap().on_open_output_directory(move || {
            let Some(main) = main.upgrade() else { return };
            let directory = state.borrow().config.save_directory.clone();
            let result = std::fs::create_dir_all(&directory)
                .with_context(|| {
                    format!(
                        "{}: {}",
                        i18n::text("创建保存目录失败", "Failed to create the save folder"),
                        directory.display()
                    )
                })
                .and_then(|_| desktop_integration().open_path(&directory));
            if let Err(error) = result {
                set_status(&main, error.to_string(), true);
            }
        });
    }
    {
        let main = main.as_weak();
        let state = Rc::clone(&state);
        main.unwrap().on_open_output_file(move || {
            let Some(main) = main.upgrade() else { return };
            let Some(path) = state.borrow().last_output.clone() else {
                set_status(
                    &main,
                    i18n::text(
                        "还没有可打开的录制文件",
                        "There is no recorded file to open yet",
                    ),
                    true,
                );
                return;
            };
            if let Err(error) = desktop_integration().open_path(&path) {
                set_status(&main, error.to_string(), true);
            }
        });
    }
    bind_live_option_callbacks(main, Rc::clone(&state));
    {
        let main = main.as_weak();
        let state = Rc::clone(&state);
        main.unwrap().on_quit_application(move || {
            if let Some(main) = main.upgrade() {
                update_config_from_main(&main, &mut state.borrow_mut().config);
                let _ = state.borrow().config.save();
            }
            if let Some(recorder) = state.borrow().recorder.as_ref() {
                recorder.send(Command::Stop);
            }
            let _ = slint::quit_event_loop();
        });
    }
}

fn refresh_screens(main: &MainWindow, state: &Rc<RefCell<UiState>>) -> Result<()> {
    let monitors = target_selection().monitors(Some(main.window()))?;
    let labels = monitors.labels();
    let previous = state.borrow().selected_screen;
    let selected_index = previous
        .and_then(|bounds| monitors.index_of(bounds))
        .unwrap_or_else(|| monitors.primary_index());
    let selected = monitors
        .get(selected_index)
        .ok_or_else(|| anyhow!(i18n::text("显示器列表为空", "The display list is empty")))?;

    main.set_screen_options(ModelRc::new(VecModel::from(
        labels
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<_>>(),
    )));
    main.set_selected_screen_index(selected_index as i32);
    main.set_selected_screen_label(
        format!("{} {}", i18n::text("显示器", "Display"), selected_index + 1).into(),
    );

    let mut state = state.borrow_mut();
    state.selected_screen = Some(selected.bounds);
    state.monitors = Some(monitors);
    Ok(())
}

fn select_screen(main: &MainWindow, state: &Rc<RefCell<UiState>>, index: i32) -> Result<()> {
    if state.borrow().recorder.is_some() || main.get_recording_state() != 0 {
        return Err(anyhow!(i18n::text(
            "录制期间不能更改显示器",
            "The display cannot be changed while recording"
        )));
    }
    let index = usize::try_from(index)
        .map_err(|_| anyhow!(i18n::text("显示器索引无效", "The display index is invalid")))?;
    let monitor = state
        .borrow()
        .monitors
        .as_ref()
        .and_then(|monitors| monitors.get(index))
        .ok_or_else(|| {
            anyhow!(i18n::text(
                "所选显示器已不存在",
                "The selected display is no longer available"
            ))
        })?;

    clear_target_indicator(state)?;
    {
        let mut state = state.borrow_mut();
        state.selected_screen = Some(monitor.bounds);
        state.target = Some(RecordingTarget::Screen(monitor.bounds));
        state.config.source_mode = 0;
    }
    main.set_source_mode(0);
    main.set_selected_screen_index(index as i32);
    main.set_selected_screen_label(
        format!("{} {}", i18n::text("显示器", "Display"), index + 1).into(),
    );
    set_status(
        main,
        format!(
            "{} {}: {} × {}",
            i18n::text("已选择显示器", "Selected display"),
            index + 1,
            monitor.bounds.width,
            monitor.bounds.height
        ),
        false,
    );
    Ok(())
}

fn bind_live_option_callbacks(main: &MainWindow, state: Rc<RefCell<UiState>>) {
    {
        let state = Rc::clone(&state);
        main.on_system_audio_changed(move |enabled| {
            state.borrow_mut().config.system_audio = enabled;
            if let Some(recorder) = state.borrow().recorder.as_ref() {
                recorder.send(Command::SystemAudio(enabled));
            }
        });
    }
    {
        let state = Rc::clone(&state);
        main.on_microphone_changed(move |enabled| {
            state.borrow_mut().config.microphone = enabled;
            if let Some(recorder) = state.borrow().recorder.as_ref() {
                recorder.send(Command::Microphone(enabled));
            }
        });
    }
    {
        let state = Rc::clone(&state);
        main.on_show_cursor_changed(move |enabled| {
            state.borrow_mut().config.show_cursor = enabled;
            if let Some(recorder) = state.borrow().recorder.as_ref() {
                recorder.send(Command::ShowCursor(enabled));
            }
        });
    }
    {
        let state = Rc::clone(&state);
        main.on_highlight_clicks_changed(move |enabled| {
            state.borrow_mut().config.highlight_clicks = enabled;
            if let Some(recorder) = state.borrow().recorder.as_ref() {
                recorder.send(Command::HighlightClicks(enabled));
            }
        });
    }
    {
        let state = Rc::clone(&state);
        main.on_countdown_changed(move |seconds| {
            state.borrow_mut().config.countdown_seconds = seconds.clamp(0, 10) as u8;
        });
    }
    {
        let state = Rc::clone(&state);
        main.on_auto_minimize_after_start_changed(move |enabled| {
            state.borrow_mut().config.auto_minimize_after_start = enabled;
        });
    }
}

const TARGET_INDICATOR_BORDER_PIXELS: i32 = 3;
const TARGET_INDICATOR_LABEL_HEIGHT_PIXELS: i32 = 28;
const TARGET_INDICATOR_LABEL_GAP_PIXELS: i32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TargetIndicatorGeometry {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TargetIndicatorKind {
    Window,
    Region,
}

impl TargetIndicatorKind {
    fn label(self) -> &'static str {
        match self {
            Self::Window => i18n::text("录制窗口", "Recording window"),
            Self::Region => i18n::text("录制区域", "Recording region"),
        }
    }
}

fn target_indicator_geometry(bounds: Bounds) -> Result<TargetIndicatorGeometry> {
    let horizontal_margin = TARGET_INDICATOR_BORDER_PIXELS
        .checked_mul(2)
        .ok_or_else(|| anyhow!("target indicator horizontal margin overflow"))?;
    let vertical_margin = TARGET_INDICATOR_LABEL_HEIGHT_PIXELS
        .checked_add(TARGET_INDICATOR_LABEL_GAP_PIXELS)
        .and_then(|value| value.checked_add(TARGET_INDICATOR_BORDER_PIXELS * 2))
        .ok_or_else(|| anyhow!("target indicator vertical margin overflow"))?;
    let width = bounds
        .width
        .checked_add(horizontal_margin)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| anyhow!("target indicator width is invalid"))?;
    let height = bounds
        .height
        .checked_add(vertical_margin)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| anyhow!("target indicator height is invalid"))?;
    let left = bounds
        .left
        .checked_sub(TARGET_INDICATOR_BORDER_PIXELS)
        .ok_or_else(|| anyhow!("target indicator left coordinate overflow"))?;
    let top_margin = TARGET_INDICATOR_LABEL_HEIGHT_PIXELS
        + TARGET_INDICATOR_LABEL_GAP_PIXELS
        + TARGET_INDICATOR_BORDER_PIXELS;
    let top = bounds
        .top
        .checked_sub(top_margin)
        .ok_or_else(|| anyhow!("target indicator top coordinate overflow"))?;

    Ok(TargetIndicatorGeometry {
        left,
        top,
        width,
        height,
    })
}

fn apply_target_indicator_geometry(
    indicator: &TargetIndicatorWindow,
    bounds: Bounds,
) -> Result<()> {
    let geometry = target_indicator_geometry(bounds)?;
    indicator.set_region_width(bounds.width);
    indicator.set_region_height(bounds.height);
    indicator.set_border_pixels(TARGET_INDICATOR_BORDER_PIXELS);
    indicator.set_label_height_pixels(TARGET_INDICATOR_LABEL_HEIGHT_PIXELS);
    indicator.set_label_gap_pixels(TARGET_INDICATOR_LABEL_GAP_PIXELS);
    indicator
        .window()
        .set_position(PhysicalPosition::new(geometry.left, geometry.top));
    indicator
        .window()
        .set_size(PhysicalSize::new(geometry.width, geometry.height));
    Ok(())
}

fn schedule_target_indicator_configuration(
    main: slint::Weak<MainWindow>,
    state: Rc<RefCell<UiState>>,
    indicator: slint::Weak<TargetIndicatorWindow>,
    generation: u64,
) -> Result<()> {
    slint::spawn_local(async move {
        let Some(indicator) = indicator.upgrade() else {
            return;
        };
        if let Err(error) = configure_visual_overlay(indicator.window()).await {
            let should_report = {
                let mut state = state.borrow_mut();
                if state.target_indicator_generation == generation {
                    state.target_indicator.take();
                    state.target_indicator_bounds = None;
                    state.target_indicator_generation =
                        state.target_indicator_generation.wrapping_add(1);
                    true
                } else {
                    false
                }
            };
            if should_report {
                let _ = indicator.hide();
                if let Some(main) = main.upgrade() {
                    set_status(&main, error.to_string(), true);
                }
            }
        }
    })
    .context(i18n::text(
        "无法安排录制目标边框初始化",
        "Could not schedule target indicator initialization",
    ))?;
    Ok(())
}

fn set_target_indicator_visible(
    main: &MainWindow,
    state: &Rc<RefCell<UiState>>,
    visible: bool,
) -> Result<()> {
    let (indicator, generation) = {
        let mut state = state.borrow_mut();
        state.target_indicator_generation = state.target_indicator_generation.wrapping_add(1);
        (
            state
                .target_indicator
                .as_ref()
                .map(ComponentHandle::clone_strong),
            state.target_indicator_generation,
        )
    };
    let Some(indicator) = indicator else {
        return Ok(());
    };
    if visible {
        indicator.show().context(i18n::text(
            "无法显示录制目标边框",
            "Could not show the target indicator",
        ))?;
        schedule_target_indicator_configuration(
            main.as_weak(),
            Rc::clone(state),
            indicator.as_weak(),
            generation,
        )?;
    } else {
        indicator.hide().context(i18n::text(
            "无法隐藏录制目标边框",
            "Could not hide the target indicator",
        ))?;
    }
    Ok(())
}

fn clear_target_indicator(state: &Rc<RefCell<UiState>>) -> Result<()> {
    let indicator = {
        let mut state = state.borrow_mut();
        state.target_indicator_generation = state.target_indicator_generation.wrapping_add(1);
        state.target_indicator_bounds = None;
        state.target_indicator.take()
    };
    if let Some(indicator) = indicator {
        indicator.hide().context(i18n::text(
            "无法移除录制目标边框",
            "Could not remove the target indicator",
        ))?;
    }
    Ok(())
}

fn replace_target_indicator(
    main: &MainWindow,
    state: &Rc<RefCell<UiState>>,
    bounds: Bounds,
    kind: TargetIndicatorKind,
) -> Result<()> {
    clear_target_indicator(state)?;
    let indicator = TargetIndicatorWindow::new().context(i18n::text(
        "无法创建录制目标边框",
        "Could not create the target indicator",
    ))?;
    indicator.set_indicator_label(kind.label().into());
    apply_target_indicator_geometry(&indicator, bounds)?;
    indicator.show().context(i18n::text(
        "无法显示录制目标边框",
        "Could not show the target indicator",
    ))?;
    let generation = {
        let mut state = state.borrow_mut();
        state.target_indicator_generation = state.target_indicator_generation.wrapping_add(1);
        state.target_indicator_bounds = Some(bounds);
        state.target_indicator = Some(indicator.clone_strong());
        state.target_indicator_generation
    };
    if let Err(error) = schedule_target_indicator_configuration(
        main.as_weak(),
        Rc::clone(state),
        indicator.as_weak(),
        generation,
    ) {
        let _ = clear_target_indicator(state);
        return Err(error);
    }
    Ok(())
}

fn ensure_target_indicator(main: &MainWindow, state: &Rc<RefCell<UiState>>) -> Result<()> {
    if state.borrow().target_indicator.is_some() {
        return Ok(());
    }
    let Some(target) = state.borrow().target else {
        return Ok(());
    };
    let kind = match target {
        RecordingTarget::Window { .. } => TargetIndicatorKind::Window,
        RecordingTarget::Region(_) => TargetIndicatorKind::Region,
        RecordingTarget::Screen(_) => return Ok(()),
    };
    replace_target_indicator(
        main,
        state,
        target_selection().current_bounds(target)?,
        kind,
    )
}

fn refresh_target_indicator(state: &Rc<RefCell<UiState>>) -> Result<()> {
    let (indicator, previous_bounds, target) = {
        let state = state.borrow();
        (
            state
                .target_indicator
                .as_ref()
                .map(ComponentHandle::clone_strong),
            state.target_indicator_bounds,
            state.target,
        )
    };
    let (Some(indicator), Some(target @ RecordingTarget::Window { .. })) = (indicator, target)
    else {
        return Ok(());
    };
    let bounds = target_selection().current_bounds(target)?;
    if previous_bounds == Some(bounds) {
        return Ok(());
    }
    apply_target_indicator_geometry(&indicator, bounds)?;
    state.borrow_mut().target_indicator_bounds = Some(bounds);
    Ok(())
}

fn open_target_selector(main: &MainWindow, state: &Rc<RefCell<UiState>>, mode: i32) -> Result<()> {
    if mode == 0 {
        let bounds = state
            .borrow()
            .selected_screen
            .unwrap_or(target_selection().primary_screen_bounds()?);
        clear_target_indicator(state)?;
        state.borrow_mut().target = Some(RecordingTarget::Screen(bounds));
        state.borrow_mut().config.source_mode = 0;
        set_status(
            main,
            i18n::text("已选择当前显示器", "Current display selected"),
            false,
        );
        return Ok(());
    }
    if state.borrow().recorder.is_some() || main.get_recording_state() != 0 {
        return Err(anyhow!(i18n::text(
            "录制期间不能更改目标",
            "The recording target cannot be changed while recording"
        )));
    }
    set_target_indicator_visible(main, state, false)?;
    let result = (|| -> Result<()> {
        let desktop = target_selection().virtual_desktop_bounds()?;
        let mut candidates = target_selection().windows(desktop)?;
        if let Some(id) = desktop_integration().native_window_id(main.window()) {
            candidates.exclude(id);
        }
        let selector = SelectionWindow::new()?;
        selector.set_mode(mode);
        selector.set_capture_width(desktop.width);
        selector.set_capture_height(desktop.height);
        selector
            .window()
            .set_position(PhysicalPosition::new(desktop.left, desktop.top));
        selector.window().set_size(PhysicalSize::new(
            desktop.width as u32,
            desktop.height as u32,
        ));

        bind_selector(&selector, main.as_weak(), Rc::clone(state), mode);
        {
            let mut state = state.borrow_mut();
            state.candidates = Some(candidates);
            state.selection_desktop = Some(desktop);
            state.selector = Some(selector.clone_strong());
        }
        selector.show()?;
        if let Err(error) = main.hide() {
            let _ = selector.hide();
            return Err(error.into());
        }
        desktop_integration().activate_window(selector.window());
        selector.invoke_take_keyboard_focus();
        Ok(())
    })();
    if result.is_err() {
        if let Some(selector) = state.borrow_mut().selector.take() {
            let _ = selector.hide();
        }
        state.borrow_mut().candidates = None;
        state.borrow_mut().selection_desktop = None;
        if let Err(restore_error) = set_target_indicator_visible(main, state, true) {
            return Err(result
                .expect_err("selection setup failed")
                .context(restore_error.to_string()));
        }
    }
    result
}

fn bind_selector(
    selector: &SelectionWindow,
    main: slint::Weak<MainWindow>,
    state: Rc<RefCell<UiState>>,
    mode: i32,
) {
    {
        let selector = selector.as_weak();
        let state = Rc::clone(&state);
        selector.unwrap().on_probe_window(move |x, y| {
            let state_ref = state.borrow();
            let Some(desktop) = state_ref.selection_desktop else {
                return;
            };
            let candidate = state_ref.candidates.as_ref().and_then(|values| {
                values.target_at(desktop.left + x as i32, desktop.top + y as i32)
            });
            if let Some(selector) = selector.upgrade() {
                if let Some(candidate) = candidate.as_ref() {
                    selector.set_hover_left((candidate.bounds.left - desktop.left) as f32);
                    selector.set_hover_top((candidate.bounds.top - desktop.top) as f32);
                    selector.set_hover_right(
                        (candidate.bounds.left + candidate.bounds.width - desktop.left) as f32,
                    );
                    selector.set_hover_bottom(
                        (candidate.bounds.top + candidate.bounds.height - desktop.top) as f32,
                    );
                    selector.set_hover_window_title(
                        if candidate.title.is_empty() {
                            i18n::text("未命名窗口", "Untitled window")
                        } else {
                            &candidate.title
                        }
                        .into(),
                    );
                    selector.set_hover_window_detail(
                        format!(
                            "{} × {} px",
                            candidate.bounds.width, candidate.bounds.height
                        )
                        .into(),
                    );
                } else {
                    selector.set_hover_left(0.0);
                    selector.set_hover_top(0.0);
                    selector.set_hover_right(0.0);
                    selector.set_hover_bottom(0.0);
                    selector.set_hover_window_title("".into());
                    selector.set_hover_window_detail("".into());
                }
            }
        });
    }
    {
        let main = main.clone();
        let state = Rc::clone(&state);
        selector.on_selected(move |left, top, right, bottom| {
            let result = selected_target(&state, mode, left, top, right, bottom)
                .and_then(|target| finish_selector(&main, &state, mode, Some(target)));
            if let Some(main) = main.upgrade()
                && let Err(error) = result
            {
                set_status(&main, error.to_string(), true);
            }
        });
    }
    {
        let state = Rc::clone(&state);
        selector.on_canceled(move || {
            let result = finish_selector(&main, &state, mode, None);
            if let Err(error) = result
                && let Some(main) = main.upgrade()
            {
                set_status(&main, error.to_string(), true);
            }
        });
    }
}

fn selected_target(
    state: &Rc<RefCell<UiState>>,
    mode: i32,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> Result<RecordingTarget> {
    let state = state.borrow();
    let desktop = state.selection_desktop.ok_or_else(|| {
        anyhow!(i18n::text(
            "目标选择会话已结束",
            "The target selection session has ended"
        ))
    })?;
    if mode == 1 {
        let x = desktop.left + (left + right) / 2;
        let y = desktop.top + (top + bottom) / 2;
        let candidate = state
            .candidates
            .as_ref()
            .and_then(|values| values.target_at(x, y))
            .ok_or_else(|| {
                anyhow!(i18n::text(
                    "光标位置没有可录制窗口",
                    "There is no recordable window at the pointer position"
                ))
            })?;
        Ok(RecordingTarget::Window {
            id: candidate.id,
            initial_bounds: candidate.bounds,
        })
    } else {
        let bounds = Bounds {
            left: desktop.left + left,
            top: desktop.top + top,
            width: right.saturating_sub(left),
            height: bottom.saturating_sub(top),
        }
        .validate()?;
        Ok(RecordingTarget::Region(bounds))
    }
}

fn finish_selector(
    main: &slint::Weak<MainWindow>,
    state: &Rc<RefCell<UiState>>,
    mode: i32,
    target: Option<RecordingTarget>,
) -> Result<()> {
    let main = main
        .upgrade()
        .ok_or_else(|| anyhow!("main window was destroyed during target selection"))?;
    let (selector, restored_mode, message, indicator_target) = {
        let mut state = state.borrow_mut();
        let selector = state.selector.take();
        state.candidates = None;
        state.selection_desktop = None;
        if let Some(target) = target {
            let bounds = target.initial_bounds();
            let indicator_target = match target {
                RecordingTarget::Window { .. } => Some(Some((bounds, TargetIndicatorKind::Window))),
                RecordingTarget::Region(bounds) => {
                    Some(Some((bounds, TargetIndicatorKind::Region)))
                }
                RecordingTarget::Screen(_) => Some(None),
            };
            state.target = Some(target);
            state.config.source_mode = mode as u8;
            (
                selector,
                mode,
                format!(
                    "{}: {} × {}",
                    i18n::text("已选择录制目标", "Recording target selected"),
                    bounds.width,
                    bounds.height
                ),
                indicator_target,
            )
        } else {
            (
                selector,
                state.config.source_mode as i32,
                i18n::text("已取消目标选择", "Target selection canceled").to_owned(),
                None,
            )
        }
    };
    main.set_source_mode(restored_mode);
    if let Some(selector) = selector {
        selector.hide().context(i18n::text(
            "无法关闭目标选择窗口",
            "Could not close the target selection window",
        ))?;
    }
    let indicator_result = match indicator_target {
        Some(Some((bounds, kind))) => replace_target_indicator(&main, state, bounds, kind),
        Some(None) => clear_target_indicator(state),
        None => set_target_indicator_visible(&main, state, true),
    };
    main.show().context(i18n::text(
        "无法恢复拾屏主窗口",
        "Could not restore the ShiPing window",
    ))?;
    indicator_result?;
    set_status(&main, message, false);
    Ok(())
}

fn begin_countdown(main: &MainWindow, state: &Rc<RefCell<UiState>>) -> Result<()> {
    if state.borrow().recorder.is_some() || main.get_recording_state() != 0 {
        return Err(anyhow!(i18n::text(
            "已有录制任务正在进行",
            "A recording task is already active"
        )));
    }
    let options = recording_options(main, state)?;
    ensure_target_indicator(main, state)?;
    {
        let mut state = state.borrow_mut();
        update_config_from_main(main, &mut state.config);
        state.config.save()?;
        state.countdown_token = state.countdown_token.wrapping_add(1);
        state.pending_options = Some(options);
    }
    main.set_output_file_name("".into());
    main.set_recording_state(3);
    let seconds = main.get_countdown_seconds().clamp(0, 10) as u8;
    let token = state.borrow().countdown_token;
    countdown_tick(main.as_weak(), Rc::clone(state), token, seconds);
    Ok(())
}

fn countdown_tick(
    main: slint::Weak<MainWindow>,
    state: Rc<RefCell<UiState>>,
    token: u64,
    remaining: u8,
) {
    let Some(main_window) = main.upgrade() else {
        return;
    };
    if state.borrow().countdown_token != token || main_window.get_recording_state() != 3 {
        return;
    }
    if remaining == 0 {
        let options = state.borrow_mut().pending_options.take();
        let Some(options) = options else { return };
        match RecorderHandle::start(options) {
            Ok(recorder) => {
                state.borrow_mut().recorder = Some(recorder);
                main_window.set_elapsed_text("00:00:00".into());
                set_status(
                    &main_window,
                    i18n::text("正在初始化录制设备...", "Initializing recording devices..."),
                    false,
                );
            }
            Err(error) => {
                main_window.set_recording_state(0);
                let indicator_error = clear_target_indicator(&state).err();
                set_status(
                    &main_window,
                    indicator_error
                        .map(|indicator_error| format!("{error}; {indicator_error}"))
                        .unwrap_or_else(|| error.to_string()),
                    true,
                );
            }
        }
        return;
    }
    main_window.set_elapsed_text(format!("00:00:{remaining:02}").into());
    set_status(
        &main_window,
        format!(
            "{remaining} {}",
            i18n::text("秒后开始录制", "seconds until recording starts")
        ),
        false,
    );
    Timer::single_shot(Duration::from_secs(1), move || {
        countdown_tick(main, state, token, remaining - 1);
    });
}

fn recording_options(main: &MainWindow, state: &Rc<RefCell<UiState>>) -> Result<RecordingOptions> {
    let output_format = OutputFormat::from_index(main.get_output_format());
    let source_mode = main.get_source_mode();
    let target = match source_mode {
        0 => RecordingTarget::Screen(
            state
                .borrow()
                .selected_screen
                .unwrap_or(target_selection().primary_screen_bounds()?),
        ),
        1 => match state.borrow().target {
            Some(target @ RecordingTarget::Window { .. }) => target,
            _ => {
                return Err(anyhow!(i18n::text(
                    "请先选择要录制的窗口",
                    "Select a window to record first"
                )));
            }
        },
        2 => match state.borrow().target {
            Some(target @ RecordingTarget::Region(_)) => target,
            _ => {
                return Err(anyhow!(i18n::text(
                    "请先选择录制区域",
                    "Select a recording region first"
                )));
            }
        },
        _ => {
            return Err(anyhow!(i18n::text(
                "录制目标类型无效",
                "The recording target type is invalid"
            )));
        }
    };
    target_selection().current_bounds(target)?;
    Ok(RecordingOptions {
        target,
        quality_preset: main.get_quality_preset().clamp(0, 3) as u8,
        frames_per_second: output_format.frames_per_second(main.get_frame_rate().clamp(0, 1) as u8),
        output_format,
        system_audio: output_format.supports_audio()
            && recording_backend().capabilities().system_audio
            && main.get_system_audio(),
        microphone: output_format.supports_audio()
            && recording_backend().capabilities().microphone
            && main.get_microphone(),
        show_cursor: main.get_show_cursor(),
        highlight_clicks: recording_backend().capabilities().highlight_clicks
            && main.get_highlight_clicks(),
        save_directory: state.borrow().config.save_directory.clone(),
    })
}

fn handle_recorder_events(main: &MainWindow, state: &Rc<RefCell<UiState>>) {
    let events = state
        .borrow()
        .recorder
        .as_ref()
        .map(RecorderHandle::drain_events)
        .unwrap_or_default();
    for event in events {
        match event {
            Event::Started {
                output_path,
                system_available,
                microphone_available,
                warnings,
            } => {
                let _ = (output_path, system_available, microphone_available);
                main.set_recording_state(1);
                if main.get_auto_minimize_after_start() {
                    main.window().set_minimized(true);
                }
                if let Some(warning) = warnings.first() {
                    set_status(
                        main,
                        format!("{}; {warning}", i18n::text("录制中", "Recording")),
                        false,
                    );
                } else {
                    set_status(main, i18n::text("录制中", "Recording"), false);
                }
            }
            Event::Progress(duration) => {
                main.set_elapsed_text(format_duration(duration).into());
            }
            Event::Paused(paused) => {
                main.set_recording_state(if paused { 2 } else { 1 });
                set_status(
                    main,
                    if paused {
                        i18n::text("录制已暂停", "Recording paused")
                    } else {
                        i18n::text("录制已继续", "Recording resumed")
                    },
                    false,
                );
            }
            Event::AudioRejected(kind, reason) => {
                match kind {
                    SourceKind::System => {
                        main.set_system_audio(false);
                        state.borrow_mut().config.system_audio = false;
                    }
                    SourceKind::Microphone => {
                        main.set_microphone(false);
                        state.borrow_mut().config.microphone = false;
                    }
                }
                set_status(main, reason, true);
            }
            Event::Completed {
                output_path,
                duration,
            } => {
                let indicator_error = clear_target_indicator(state).err();
                state.borrow_mut().last_output = Some(output_path.clone());
                state.borrow_mut().recorder.take();
                main.set_recording_state(0);
                main.set_elapsed_text(format_duration(duration).into());
                let file_name = output_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or_else(|| i18n::text("录制文件", "recording"))
                    .to_owned();
                main.set_output_file_name(file_name.clone().into());
                let open_error = state
                    .borrow()
                    .config
                    .open_directory_after_stop
                    .then(|| {
                        output_path
                            .parent()
                            .ok_or_else(|| {
                                anyhow!(i18n::text(
                                    "录制文件没有父目录",
                                    "The recorded file has no parent directory"
                                ))
                            })
                            .and_then(|path| desktop_integration().open_path(path))
                    })
                    .and_then(Result::err);
                match (open_error, indicator_error) {
                    (Some(open_error), Some(indicator_error)) => set_status(
                        main,
                        format!(
                            "{}: {file_name}; {}: {open_error}; {indicator_error}",
                            i18n::text("已保存", "Saved"),
                            i18n::text("打开目录失败", "Failed to open the save folder")
                        ),
                        true,
                    ),
                    (Some(error), None) => set_status(
                        main,
                        format!(
                            "{}: {file_name}; {}: {error}",
                            i18n::text("已保存", "Saved"),
                            i18n::text("打开目录失败", "Failed to open the save folder")
                        ),
                        true,
                    ),
                    (None, Some(error)) => set_status(
                        main,
                        format!("{}: {file_name}; {error}", i18n::text("已保存", "Saved")),
                        true,
                    ),
                    (None, None) => set_status(
                        main,
                        format!(
                            "{}: {file_name} {}",
                            i18n::text("已保存", "Saved"),
                            i18n::text("（单击打开）", "(click to open)")
                        ),
                        false,
                    ),
                }
            }
            Event::Failed(message) => {
                let indicator_error = clear_target_indicator(state).err();
                state.borrow_mut().recorder.take();
                main.set_recording_state(0);
                main.set_output_file_name("".into());
                set_status(
                    main,
                    indicator_error
                        .map(|error| {
                            format!(
                                "{}: {message}; {error}",
                                i18n::text("录制失败", "Recording failed")
                            )
                        })
                        .unwrap_or_else(|| {
                            format!("{}: {message}", i18n::text("录制失败", "Recording failed"))
                        }),
                    true,
                );
            }
        }
    }
}

fn update_config_from_main(main: &MainWindow, config: &mut Config) {
    config.source_mode = main.get_source_mode().clamp(0, 2) as u8;
    config.quality_preset = main.get_quality_preset().clamp(0, 3) as u8;
    config.frame_rate = main.get_frame_rate().clamp(0, 1) as u8;
    config.output_format = OutputFormat::from_index(main.get_output_format());
    config.system_audio = config.output_format.supports_audio() && main.get_system_audio();
    config.microphone = config.output_format.supports_audio() && main.get_microphone();
    config.show_cursor = main.get_show_cursor();
    config.highlight_clicks = main.get_highlight_clicks();
    config.countdown_seconds = main.get_countdown_seconds().clamp(0, 10) as u8;
    config.auto_minimize_after_start = main.get_auto_minimize_after_start();
    normalize_recording_capabilities(config, recording_backend().capabilities());
}

fn normalize_recording_capabilities(config: &mut Config, capabilities: RecordingCapabilities) {
    config.system_audio &= capabilities.system_audio;
    config.microphone &= capabilities.microphone;
    config.highlight_clicks &= capabilities.highlight_clicks;
}

fn set_status(main: &MainWindow, message: impl Into<String>, error: bool) {
    main.set_status_text(message.into().into());
    main.set_status_level(if error { 2 } else { 0 });
}

fn build_information() -> String {
    format!(
        "{} {} · Slint 1.17.0 · {}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        if cfg!(debug_assertions) {
            "Debug"
        } else {
            "Release"
        }
    )
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod capability_tests {
    use super::normalize_recording_capabilities;
    use crate::{config::Config, ports::RecordingCapabilities};

    #[test]
    fn unavailable_platform_features_are_disabled_in_configuration() {
        let mut config = Config {
            system_audio: true,
            microphone: true,
            highlight_clicks: true,
            ..Config::default()
        };
        normalize_recording_capabilities(
            &mut config,
            RecordingCapabilities {
                system_audio: false,
                microphone: true,
                highlight_clicks: false,
            },
        );
        assert!(!config.system_audio);
        assert!(config.microphone);
        assert!(!config.highlight_clicks);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TARGET_INDICATOR_BORDER_PIXELS, TARGET_INDICATOR_LABEL_GAP_PIXELS,
        TARGET_INDICATOR_LABEL_HEIGHT_PIXELS, language_from_index, language_index,
        target_indicator_geometry,
    };
    use crate::config::LanguageMode;
    use crate::domain::Bounds;

    #[test]
    fn preferences_map_only_english_and_chinese() {
        assert_eq!(language_index(LanguageMode::English), 0);
        assert_eq!(language_index(LanguageMode::Chinese), 1);
        assert_eq!(language_from_index(0), LanguageMode::English);
        assert_eq!(language_from_index(1), LanguageMode::Chinese);
        assert_eq!(language_from_index(99), LanguageMode::English);
    }

    #[test]
    fn target_indicator_window_keeps_visible_pixels_outside_capture_bounds() {
        let capture = Bounds {
            left: -640,
            top: 120,
            width: 1280,
            height: 720,
        };
        let indicator = target_indicator_geometry(capture).unwrap();
        let capture_left_in_indicator = capture.left - indicator.left;
        let capture_top_in_indicator = capture.top - indicator.top;

        assert_eq!(capture_left_in_indicator, TARGET_INDICATOR_BORDER_PIXELS);
        assert_eq!(
            capture_top_in_indicator,
            TARGET_INDICATOR_LABEL_HEIGHT_PIXELS
                + TARGET_INDICATOR_LABEL_GAP_PIXELS
                + TARGET_INDICATOR_BORDER_PIXELS
        );
        assert_eq!(
            indicator.width as i32,
            capture.width + TARGET_INDICATOR_BORDER_PIXELS * 2
        );
        assert_eq!(
            indicator.height as i32,
            capture.height
                + TARGET_INDICATOR_LABEL_HEIGHT_PIXELS
                + TARGET_INDICATOR_LABEL_GAP_PIXELS
                + TARGET_INDICATOR_BORDER_PIXELS * 2
        );
    }
}
