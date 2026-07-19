use std::{
    cell::RefCell,
    ops::{Deref, DerefMut},
    rc::Rc,
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use slint::{ComponentHandle, PhysicalPosition, PhysicalSize, Timer, TimerMode};

use crate::{
    MainWindow, SelectionWindow,
    application::{ApplicationState, Command, Event, RecorderHandle, RecordingOptions},
    config::Config,
    platform::{
        audio::SourceKind,
        begin_window_drag, native_window_handle, shell, show_native_choice_menu,
        target::{self, Bounds, RecordingTarget, WindowCandidates},
    },
};

struct UiState {
    application: ApplicationState,
    selector: Option<SelectionWindow>,
    candidates: Option<WindowCandidates>,
    selection_desktop: Option<Bounds>,
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
    let (config, load_error) = match Config::load() {
        Ok(config) => (config, None),
        Err(error) => (Config::default(), Some(error.to_string())),
    };
    let main = MainWindow::new()?;
    apply_config(&main, &config);
    if let Some(error) = load_error {
        set_status(&main, format!("配置未加载：{error}"), true);
    }

    let state = Rc::new(RefCell::new(UiState {
        application: ApplicationState::new(config),
        selector: None,
        candidates: None,
        selection_desktop: None,
    }));
    bind_callbacks(&main, Rc::clone(&state));

    let event_timer = Timer::default();
    {
        let main = main.as_weak();
        let state = Rc::clone(&state);
        event_timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
            if let Some(main) = main.upgrade() {
                handle_recorder_events(&main, &state);
            }
        });
    }

    main.run()
}

fn apply_config(main: &MainWindow, config: &Config) {
    main.set_source_mode(config.source_mode as i32);
    main.set_quality_preset(config.quality_preset as i32);
    main.set_frame_rate(config.frame_rate as i32);
    main.set_system_audio(config.system_audio);
    main.set_microphone(config.microphone);
    main.set_show_cursor(config.show_cursor);
    main.set_highlight_clicks(config.highlight_clicks);
    main.set_countdown_seconds(config.countdown_seconds as i32);
    main.set_save_directory(config.save_directory.to_string_lossy().into_owned().into());
}

fn bind_callbacks(main: &MainWindow, state: Rc<RefCell<UiState>>) {
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
                set_status(&main, "当前没有可暂停的录制", true);
            }
        });
    }
    {
        let main = main.as_weak();
        let state = Rc::clone(&state);
        main.unwrap().on_stop_recording(move || {
            let Some(main) = main.upgrade() else { return };
            if let Some(recorder) = state.borrow().recorder.as_ref() {
                recorder.send(Command::Stop);
                set_status(&main, "正在完成 MP4 文件...", false);
            } else if main.get_recording_state() == 3 {
                let mut state = state.borrow_mut();
                state.countdown_token = state.countdown_token.wrapping_add(1);
                state.pending_options = None;
                main.set_recording_state(0);
                main.set_elapsed_text("00:00:00".into());
                set_status(&main, "已取消开始录制", false);
            }
        });
    }
    #[cfg(target_os = "windows")]
    {
        let main = main.as_weak();
        main.unwrap().on_begin_window_drag(move || {
            if let Some(main) = main.upgrade() {
                begin_window_drag(main.window());
                let main = main.as_weak();
                Timer::single_shot(Duration::ZERO, move || {
                    if let Some(main) = main.upgrade() {
                        main.window()
                            .dispatch_event(slint::platform::WindowEvent::PointerExited);
                    }
                });
            }
        });
    }
    #[cfg(target_os = "windows")]
    {
        let main = main.as_weak();
        main.unwrap().on_show_source_menu(move || {
            let Some(main) = main.upgrade() else { return };
            match show_native_choice_menu(
                main.window(),
                &["全屏", "窗口", "区域"],
                main.get_source_mode() as usize,
            ) {
                Ok(Some(index)) => {
                    main.set_source_mode(index as i32);
                    main.invoke_choose_source();
                }
                Ok(None) => {}
                Err(error) => set_status(&main, error.to_string(), true),
            }
        });
    }
    #[cfg(target_os = "windows")]
    {
        let main = main.as_weak();
        main.unwrap().on_show_quality_menu(move || {
            let Some(main) = main.upgrade() else { return };
            match show_native_choice_menu(
                main.window(),
                &["自动", "720p", "1080p", "原始分辨率"],
                main.get_quality_preset() as usize,
            ) {
                Ok(Some(index)) => main.set_quality_preset(index as i32),
                Ok(None) => {}
                Err(error) => set_status(&main, error.to_string(), true),
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
                set_status(&main, "录制期间不能更改保存目录", true);
                return;
            }
            let current = state.borrow().config.save_directory.clone();
            let Some(directory) = rfd::FileDialog::new()
                .set_title("选择拾屏保存目录")
                .set_directory(current)
                .pick_folder()
            else {
                return;
            };
            state.borrow_mut().config.save_directory = directory.clone();
            main.set_save_directory(directory.to_string_lossy().into_owned().into());
            match state.borrow().config.save() {
                Ok(()) => set_status(&main, "保存目录已更新", false),
                Err(error) => set_status(&main, format!("保存配置失败：{error}"), true),
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
                .with_context(|| format!("创建保存目录失败：{}", directory.display()))
                .and_then(|_| shell::open_path(&directory));
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
                set_status(&main, "还没有可打开的录制文件", true);
                return;
            };
            if let Err(error) = shell::open_path(&path) {
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
}

fn open_target_selector(main: &MainWindow, state: &Rc<RefCell<UiState>>, mode: i32) -> Result<()> {
    if mode == 0 {
        state.borrow_mut().target = None;
        state.borrow_mut().config.source_mode = 0;
        set_status(main, "已选择主显示器", false);
        return Ok(());
    }
    if state.borrow().recorder.is_some() || main.get_recording_state() != 0 {
        return Err(anyhow!("录制期间不能更改目标"));
    }
    let desktop = target::virtual_desktop_bounds()?;
    let result = (|| -> Result<()> {
        let mut candidates = WindowCandidates::snapshot(desktop)?;
        if let Some(hwnd) = native_window_handle(main.window()) {
            candidates.exclude(hwnd);
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
        selector.invoke_take_keyboard_focus();
        Ok(())
    })();
    if result.is_err() {
        if let Some(selector) = state.borrow_mut().selector.take() {
            let _ = selector.hide();
        }
        state.borrow_mut().candidates = None;
        state.borrow_mut().selection_desktop = None;
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
                if let Some(candidate) = candidate {
                    selector.set_hover_left((candidate.bounds.left - desktop.left) as f32);
                    selector.set_hover_top((candidate.bounds.top - desktop.top) as f32);
                    selector.set_hover_right(
                        (candidate.bounds.left + candidate.bounds.width - desktop.left) as f32,
                    );
                    selector.set_hover_bottom(
                        (candidate.bounds.top + candidate.bounds.height - desktop.top) as f32,
                    );
                } else {
                    selector.set_hover_left(0.0);
                    selector.set_hover_top(0.0);
                    selector.set_hover_right(0.0);
                    selector.set_hover_bottom(0.0);
                }
            }
        });
    }
    {
        let main = main.clone();
        let state = Rc::clone(&state);
        selector.on_selected(move |left, top, right, bottom| {
            let result = selected_target(&state, mode, left, top, right, bottom);
            if let Some(main) = main.upgrade() {
                match result {
                    Ok(target) => finish_selector(&main, &state, mode, Some(target)),
                    Err(error) => set_status(&main, error.to_string(), true),
                }
            }
        });
    }
    {
        let state = Rc::clone(&state);
        selector.on_canceled(move || {
            if let Some(main) = main.upgrade() {
                finish_selector(&main, &state, mode, None);
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
    let desktop = state
        .selection_desktop
        .ok_or_else(|| anyhow!("目标选择会话已结束"))?;
    if mode == 1 {
        let x = desktop.left + (left + right) / 2;
        let y = desktop.top + (top + bottom) / 2;
        let candidate = state
            .candidates
            .as_ref()
            .and_then(|values| values.target_at(x, y))
            .ok_or_else(|| anyhow!("光标位置没有可录制窗口"))?;
        Ok(RecordingTarget::Window {
            hwnd: candidate.hwnd,
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
    main: &MainWindow,
    state: &Rc<RefCell<UiState>>,
    mode: i32,
    target: Option<RecordingTarget>,
) {
    let (selector, restored_mode, message) = {
        let mut state = state.borrow_mut();
        let selector = state.selector.take();
        state.candidates = None;
        state.selection_desktop = None;
        if let Some(target) = target {
            let bounds = target.initial_bounds();
            state.target = Some(target);
            state.config.source_mode = mode as u8;
            (
                selector,
                mode,
                format!("已选择 {} × {} 录制目标", bounds.width, bounds.height),
            )
        } else {
            (
                selector,
                state.config.source_mode as i32,
                "已取消目标选择".to_owned(),
            )
        }
    };
    main.set_source_mode(restored_mode);
    let _ = main.show();
    if let Some(selector) = selector {
        let _ = selector.hide();
    }
    set_status(main, message, false);
}

fn begin_countdown(main: &MainWindow, state: &Rc<RefCell<UiState>>) -> Result<()> {
    if state.borrow().recorder.is_some() || main.get_recording_state() != 0 {
        return Err(anyhow!("已有录制任务正在进行"));
    }
    let options = recording_options(main, state)?;
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
                set_status(&main_window, "正在初始化录制设备...", false);
            }
            Err(error) => {
                main_window.set_recording_state(0);
                set_status(&main_window, error.to_string(), true);
            }
        }
        return;
    }
    main_window.set_elapsed_text(format!("00:00:{remaining:02}").into());
    set_status(&main_window, format!("{remaining} 秒后开始录制"), false);
    Timer::single_shot(Duration::from_secs(1), move || {
        countdown_tick(main, state, token, remaining - 1);
    });
}

fn recording_options(main: &MainWindow, state: &Rc<RefCell<UiState>>) -> Result<RecordingOptions> {
    let source_mode = main.get_source_mode();
    let target = match source_mode {
        0 => RecordingTarget::Screen(target::primary_screen_bounds()?),
        1 => match state.borrow().target {
            Some(target @ RecordingTarget::Window { .. }) => target,
            _ => return Err(anyhow!("请先选择要录制的窗口")),
        },
        2 => match state.borrow().target {
            Some(target @ RecordingTarget::Region(_)) => target,
            _ => return Err(anyhow!("请先选择录制区域")),
        },
        _ => return Err(anyhow!("录制目标类型无效")),
    };
    target.current_bounds()?;
    Ok(RecordingOptions {
        target,
        quality_preset: main.get_quality_preset().clamp(0, 3) as u8,
        frames_per_second: if main.get_frame_rate() == 0 { 30 } else { 60 },
        system_audio: main.get_system_audio(),
        microphone: main.get_microphone(),
        show_cursor: main.get_show_cursor(),
        highlight_clicks: main.get_highlight_clicks(),
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
                if let Some(warning) = warnings.first() {
                    set_status(main, format!("录制中；{warning}"), false);
                } else {
                    set_status(main, "录制中", false);
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
                        "录制已暂停"
                    } else {
                        "录制已继续"
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
                state.borrow_mut().last_output = Some(output_path.clone());
                state.borrow_mut().recorder.take();
                main.set_recording_state(0);
                main.set_elapsed_text(format_duration(duration).into());
                let file_name = output_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("录制文件")
                    .to_owned();
                main.set_output_file_name(file_name.clone().into());
                set_status(main, format!("已保存：{file_name}（单击打开）"), false);
            }
            Event::Failed(message) => {
                state.borrow_mut().recorder.take();
                main.set_recording_state(0);
                main.set_output_file_name("".into());
                set_status(main, format!("录制失败：{message}"), true);
            }
        }
    }
}

fn update_config_from_main(main: &MainWindow, config: &mut Config) {
    config.source_mode = main.get_source_mode().clamp(0, 2) as u8;
    config.quality_preset = main.get_quality_preset().clamp(0, 3) as u8;
    config.frame_rate = main.get_frame_rate().clamp(0, 1) as u8;
    config.system_audio = main.get_system_audio();
    config.microphone = main.get_microphone();
    config.show_cursor = main.get_show_cursor();
    config.highlight_clicks = main.get_highlight_clicks();
    config.countdown_seconds = main.get_countdown_seconds().clamp(0, 10) as u8;
}

fn set_status(main: &MainWindow, message: impl Into<String>, error: bool) {
    main.set_status_text(message.into().into());
    main.set_status_level(if error { 2 } else { 0 });
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
