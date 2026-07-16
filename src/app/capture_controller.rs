use std::{cell::RefCell, rc::Rc, thread, time::Duration};

use anyhow::{Result, anyhow};
use slint::{ComponentHandle, ModelRc, PhysicalPosition, PhysicalSize, Timer, VecModel};

use super::{
    AnnotationView, AppController, MainWindow, OverlayWindow, StatusLevel,
    annotation::AnnotationHistory,
    pin::{PinRegistry, PinRequest},
    set_error_status, set_status, set_status_level, show_main_window,
};
use crate::{
    capture,
    config::{CaptureConfig, CompletionAction},
    image::{CapturedImage, DesktopBounds},
    logging, output,
    platform::{
        ocr::{OcrEngine, OcrFailure, system_engine},
        windows::{window, window_target::WindowTargets},
    },
};

const OVERLAY_HANDLE_RETRY_LIMIT: u8 = 10;

pub(super) fn start_capture(main: slint::Weak<MainWindow>, state: Rc<RefCell<AppController>>) {
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

    let delay = if main_window.window().is_visible() {
        Duration::from_millis(16)
    } else {
        Duration::ZERO
    };
    Timer::single_shot(delay, move || {
        let result = capture::virtual_desktop_bounds();
        state.borrow_mut().capturing = false;

        match result {
            Ok(bounds) => {
                logging::info("capture overlay prepared");
                if let Err(error) = open_overlay(main.clone(), Rc::clone(&state), bounds) {
                    set_error_status(
                        &main,
                        &mut state.borrow_mut(),
                        format!("截图窗口打开失败：{error}"),
                    );
                    restore_main_after_capture(&main, &state);
                }
            }
            Err(error) => {
                logging::error(error.to_string());
                set_error_status(&main, &mut state.borrow_mut(), format!("截图失败：{error}"));
                restore_main_after_capture(&main, &state);
            }
        }
    });
}

fn open_overlay(
    main: slint::Weak<MainWindow>,
    state: Rc<RefCell<AppController>>,
    bounds: DesktopBounds,
) -> Result<()> {
    let overlay = OverlayWindow::new()?;
    let targets = WindowTargets::snapshot(bounds)?;
    let initial_target = targets.target_at_cursor();
    let output_mode = completion_action_index(state.borrow().config.capture.completion_action);

    overlay.set_capture_width(bounds.width);
    overlay.set_capture_height(bounds.height);
    overlay.set_annotations(empty_annotation_model());
    overlay.set_output_mode(output_mode);
    overlay.set_ocr_available(cfg!(windows));
    set_hover_target(
        &overlay,
        initial_target.map(|target| relative_bounds(bounds, target)),
    );
    overlay
        .window()
        .set_position(PhysicalPosition::new(bounds.left, bounds.top));
    overlay
        .window()
        .set_size(PhysicalSize::new(bounds.width as u32, bounds.height as u32));

    bind_overlay(&overlay, main.clone(), Rc::clone(&state));
    state.borrow_mut().session = Some(CaptureSession {
        desktop_bounds: bounds,
        window_targets: targets,
        selected: None,
        annotations: AnnotationHistory::default(),
        _overlay: overlay.clone_strong(),
    });

    if let Err(error) = overlay.show() {
        state.borrow_mut().session = None;
        return Err(error.into());
    }
    prepare_overlay_capture(overlay.as_weak(), main, Rc::clone(&state), 0);
    Ok(())
}

fn prepare_overlay_capture(
    overlay: slint::Weak<OverlayWindow>,
    main: slint::Weak<MainWindow>,
    state: Rc<RefCell<AppController>>,
    attempt: u8,
) {
    let delay = if attempt == 0 {
        Duration::ZERO
    } else {
        Duration::from_millis(16)
    };
    Timer::single_shot(delay, move || {
        let Some(overlay_window) = overlay.upgrade() else {
            return;
        };
        if window::hwnd(overlay_window.window()).is_none() {
            if attempt < OVERLAY_HANDLE_RETRY_LIMIT {
                prepare_overlay_capture(overlay_window.as_weak(), main, state, attempt + 1);
            } else {
                fail_overlay_preparation(
                    &overlay_window,
                    &main,
                    &state,
                    "截图遮罩窗口句柄未就绪".to_owned(),
                );
            }
            return;
        }

        match window::set_excluded_from_capture(overlay_window.window(), true) {
            Ok(()) => {
                overlay_window.set_capture_ready(true);
                overlay_window.window().request_redraw();
            }
            Err(error) => fail_overlay_preparation(
                &overlay_window,
                &main,
                &state,
                format!("无法将截图遮罩排除在截图外：{error}"),
            ),
        }
    });
}

fn fail_overlay_preparation(
    overlay: &OverlayWindow,
    main: &slint::Weak<MainWindow>,
    state: &Rc<RefCell<AppController>>,
    message: String,
) {
    logging::error(message.as_str());
    let _ = overlay.hide();
    {
        let mut state = state.borrow_mut();
        state.session = None;
        set_error_status(main, &mut state, format!("截图窗口打开失败：{message}"));
    }
    restore_main_after_capture(main, state);
}

fn bind_overlay(
    overlay: &OverlayWindow,
    main: slint::Weak<MainWindow>,
    state: Rc<RefCell<AppController>>,
) {
    {
        let overlay = overlay.as_weak();
        let state = Rc::clone(&state);
        overlay.unwrap().on_probe_window(move |x, y| {
            let target = state.borrow().window_target_at(x, y);
            if let Some(overlay) = overlay.upgrade() {
                set_hover_target(&overlay, target);
            }
        });
    }
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
                            overlay.set_completed(false);
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
                Ok(status) => finish_capture(&overlay, &main, &state, status, StatusLevel::Success),
                Err(error) => {
                    logging::error(error.to_string());
                    set_error_status(&main, &mut state.borrow_mut(), format!("输出失败：{error}"));
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
                    set_error_status(&main, &mut state.borrow_mut(), format!("钉住失败：{error}"));
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
                PinRequest {
                    image,
                    source_path,
                    pin_config,
                    capture_config,
                },
                main.clone(),
                app_state.clone(),
            ) {
                Ok(()) => {
                    let (status, level) = match auto_save_error {
                        Some(error) if show_save_result => (
                            format!("已钉住，但自动保存失败：{error}"),
                            StatusLevel::Error,
                        ),
                        _ => ("已将截图钉在屏幕上".to_owned(), StatusLevel::Success),
                    };
                    finish_capture(&overlay, &main, &state, status, level);
                }
                Err(error) => {
                    set_error_status(&main, &mut state.borrow_mut(), format!("钉住失败：{error}"));
                }
            }
        });
    }
    {
        let overlay = overlay.as_weak();
        overlay.unwrap().on_cancel_selection(move || {
            finish_capture(
                &overlay,
                &main,
                &state,
                "已取消截图".to_owned(),
                StatusLevel::Info,
            );
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
            Ok(()) => finish_capture(
                overlay,
                main,
                state,
                "已识别并复制文字".to_owned(),
                StatusLevel::Success,
            ),
            Err(error) => {
                set_error_status(
                    main,
                    &mut state.borrow_mut(),
                    format!("OCR 文字复制失败：{error}"),
                );
            }
        },
        1 => set_overlay_message(overlay, main, state, "未识别到文字", StatusLevel::Info),
        2 => set_overlay_message(
            overlay,
            main,
            state,
            "缺少中文 OCR 语言包",
            StatusLevel::Error,
        ),
        3 => set_overlay_message(
            overlay,
            main,
            state,
            "当前平台不支持系统 OCR",
            StatusLevel::Error,
        ),
        _ => set_overlay_message(overlay, main, state, "OCR 识别失败", StatusLevel::Error),
    }
}

fn set_overlay_message(
    overlay: &slint::Weak<OverlayWindow>,
    main: &slint::Weak<MainWindow>,
    state: &Rc<RefCell<AppController>>,
    message: &str,
    level: StatusLevel,
) {
    if let Some(overlay) = overlay.upgrade() {
        overlay.set_selection_info(message.into());
    }
    set_status_level(main, &mut state.borrow_mut(), message.to_owned(), level);
}

pub(super) fn ocr_result_payload(result: Result<String, OcrFailure>) -> (i32, String) {
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

fn completion_action_index(action: CompletionAction) -> i32 {
    match action {
        CompletionAction::Copy => 0,
        CompletionAction::Save => 1,
        CompletionAction::CopyAndSave => 2,
    }
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
    level: StatusLevel,
) {
    if let Some(overlay) = overlay.upgrade() {
        let _ = overlay.hide();
    }
    {
        let mut state = state.borrow_mut();
        state.session = None;
        set_status_level(main, &mut state, status, level);
    }
    restore_main_after_capture(main, state);
}

fn restore_main_after_capture(main: &slint::Weak<MainWindow>, state: &Rc<RefCell<AppController>>) {
    if state.borrow().restore_main_after_capture {
        show_main_window(main);
    }
}

impl AppController {
    fn window_target_at(&self, x: f32, y: f32) -> Option<DesktopBounds> {
        let session = self.session.as_ref()?;
        let local_x = x
            .round()
            .clamp(0.0, session.desktop_bounds.width.saturating_sub(1) as f32)
            as i32;
        let local_y = y
            .round()
            .clamp(0.0, session.desktop_bounds.height.saturating_sub(1) as f32)
            as i32;
        session
            .window_targets
            .target_at(
                session.desktop_bounds.left + local_x,
                session.desktop_bounds.top + local_y,
            )
            .map(|target| relative_bounds(session.desktop_bounds, target))
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
            session.desktop_bounds.width as u32,
            session.desktop_bounds.height as u32,
        )
        .ok_or_else(|| anyhow!("选区过小"))?;
        let selected = capture::capture_region(DesktopBounds {
            left: session.desktop_bounds.left + left as i32,
            top: session.desktop_bounds.top + top as i32,
            width: width as i32,
            height: height as i32,
        })?;
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

pub(super) fn normalized_selection(
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

pub(super) struct CaptureSession {
    desktop_bounds: DesktopBounds,
    window_targets: WindowTargets,
    selected: Option<CapturedImage>,
    annotations: AnnotationHistory,
    _overlay: OverlayWindow,
}

fn relative_bounds(desktop: DesktopBounds, target: DesktopBounds) -> DesktopBounds {
    DesktopBounds {
        left: target.left - desktop.left,
        top: target.top - desktop.top,
        width: target.width,
        height: target.height,
    }
}

fn set_hover_target(overlay: &OverlayWindow, target: Option<DesktopBounds>) {
    let target = target.unwrap_or(DesktopBounds {
        left: 0,
        top: 0,
        width: 0,
        height: 0,
    });
    overlay.set_hover_left(target.left as f32);
    overlay.set_hover_top(target.top as f32);
    overlay.set_hover_right((target.left + target.width) as f32);
    overlay.set_hover_bottom((target.top + target.height) as f32);
}
