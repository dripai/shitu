use std::{cell::RefCell, rc::Rc, thread, time::Duration};

use anyhow::{Result, anyhow};
use slint::{ComponentHandle, ModelRc, PhysicalPosition, PhysicalSize, Timer, VecModel};

use super::{
    AnnotationView, AppController, MainWindow, OverlayWindow, StatusLevel,
    annotation::AnnotationHistory,
    pin::{PinRegistry, PinRequest},
    present_ocr_error, present_ocr_notice, present_ocr_result, set_error_status, set_status,
    set_status_level, show_main_window,
};
use crate::{
    capture,
    config::{CaptureConfig, OcrConfig},
    i18n,
    image::{CapturedImage, DesktopBounds},
    logging, output,
    platform::{
        ocr::{OcrFailure, recognize},
        windows::window_target::WindowTargets,
    },
};

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
                i18n::text(
                    "已有截图任务正在进行，请先完成或取消",
                    "A screenshot is already in progress; finish or cancel it first",
                )
                .to_owned(),
            );
            return;
        }
        state.capturing = true;
        state.restore_main_after_capture = main_window.window().is_visible();
        set_status(
            &main,
            &mut state,
            i18n::text("正在准备截图...", "Preparing screenshot...").to_owned(),
        );
    }

    let delay = if main_window.window().is_visible() {
        Duration::from_millis(16)
    } else {
        Duration::ZERO
    };
    Timer::single_shot(delay, move || {
        let result = capture::virtual_desktop_bounds().and_then(|bounds| {
            let targets = WindowTargets::snapshot(bounds)?;
            let desktop_snapshot = capture::capture_region(bounds)?;
            open_overlay(
                main.clone(),
                Rc::clone(&state),
                bounds,
                targets,
                desktop_snapshot,
            )
        });
        state.borrow_mut().capturing = false;

        match result {
            Ok(()) => logging::info("capture snapshot prepared"),
            Err(error) => {
                logging::error(error.to_string());
                set_error_status(
                    &main,
                    &mut state.borrow_mut(),
                    format!(
                        "{}: {error}",
                        i18n::text("截图窗口打开失败", "Failed to open screenshot window")
                    ),
                );
                restore_main_after_capture(&main, &state);
            }
        }
    });
}

fn open_overlay(
    main: slint::Weak<MainWindow>,
    state: Rc<RefCell<AppController>>,
    bounds: DesktopBounds,
    targets: WindowTargets,
    desktop_snapshot: CapturedImage,
) -> Result<()> {
    let overlay = OverlayWindow::new()?;
    let initial_target = targets.target_at_cursor();
    overlay.set_capture_width(bounds.width);
    overlay.set_capture_height(bounds.height);
    overlay.set_desktop_image(desktop_snapshot.slint_image());
    overlay.set_annotations(empty_annotation_model());
    overlay.set_ocr_available(state.borrow().ocr_available);
    overlay.set_capture_ready(true);
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
        desktop_snapshot,
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
                            overlay.set_selection_info(
                                format!("{}: {error}", i18n::text("选区无效", "Invalid selection"))
                                    .into(),
                            );
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
        let overlay = overlay.as_weak();
        let state = Rc::clone(&state);
        overlay.unwrap().on_add_text(move |x, y, text, font_size| {
            state.borrow_mut().add_text(x, y, text.as_str(), font_size);
            refresh_annotations(&overlay, &state);
        });
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
        overlay.unwrap().on_save_output(move || {
            let (image, save_directory, format, jpeg_quality) = {
                let state = state.borrow();
                (
                    state.rendered_selection(),
                    state.config.capture.save_directory.clone(),
                    state.config.capture.format,
                    state.config.capture.jpeg_quality,
                )
            };
            match image.and_then(|image| {
                output::save_as_dialog(&image, &save_directory, format, jpeg_quality)
            }) {
                Ok(Some(path)) => finish_capture(
                    &overlay,
                    &main,
                    &state,
                    format!("{} {}", i18n::text("已保存到", "Saved to"), path.display()),
                    StatusLevel::Success,
                ),
                Ok(None) => {}
                Err(error) => {
                    logging::error(error.to_string());
                    set_error_status(
                        &main,
                        &mut state.borrow_mut(),
                        format!("{}: {error}", i18n::text("保存失败", "Save failed")),
                    );
                }
            }
        });
    }
    {
        let overlay = overlay.as_weak();
        let main = main.clone();
        let state = Rc::clone(&state);
        overlay.unwrap().on_copy_output(move || {
            let (image, config) = {
                let state = state.borrow();
                (state.rendered_selection(), state.config.capture.clone())
            };
            match image.and_then(|image| copy_output(&image, &config)) {
                Ok(status) => finish_capture(&overlay, &main, &state, status, StatusLevel::Success),
                Err(error) => {
                    logging::error(error.to_string());
                    set_error_status(
                        &main,
                        &mut state.borrow_mut(),
                        format!("{}: {error}", i18n::text("复制失败", "Copy failed")),
                    );
                }
            }
        });
    }
    {
        let overlay = overlay.as_weak();
        let state = Rc::clone(&state);
        overlay.unwrap().on_recognize_text(move || {
            let (image, ocr_config) = {
                let state = state.borrow();
                (state.original_selection(), state.config.ocr.clone())
            };
            match image {
                Ok(image) => {
                    if let Some(overlay) = overlay.upgrade() {
                        overlay.set_selection_info(
                            i18n::text("正在识别文字...", "Recognizing text...").into(),
                        );
                    }
                    spawn_overlay_ocr(overlay.clone(), image, ocr_config);
                }
                Err(error) => {
                    if let Some(overlay) = overlay.upgrade() {
                        overlay.set_selection_info(
                            format!("{}: {error}", i18n::text("OCR 失败", "OCR failed")).into(),
                        );
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
            let (image, pin_config, capture_config, ocr_config, pins, ocr_available) = {
                let state = state.borrow();
                (
                    state.rendered_selection(),
                    state.config.pin.clone(),
                    state.config.capture.clone(),
                    state.config.ocr.clone(),
                    state.pins.clone(),
                    state.ocr_available,
                )
            };
            let image = match image {
                Ok(image) => image,
                Err(error) => {
                    set_error_status(
                        &main,
                        &mut state.borrow_mut(),
                        format!("{}: {error}", i18n::text("钉住失败", "Pin failed")),
                    );
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
                    ocr_config,
                    ocr_available,
                },
                main.clone(),
                app_state.clone(),
            ) {
                Ok(()) => {
                    let (status, level) = match auto_save_error {
                        Some(error) if show_save_result => (
                            format!(
                                "{}: {error}",
                                i18n::text(
                                    "已钉住，但自动保存失败",
                                    "Pinned, but auto-save failed"
                                )
                            ),
                            StatusLevel::Error,
                        ),
                        _ => (
                            i18n::text("已将截图钉在屏幕上", "Screenshot pinned").to_owned(),
                            StatusLevel::Success,
                        ),
                    };
                    finish_capture(&overlay, &main, &state, status, level);
                }
                Err(error) => {
                    set_error_status(
                        &main,
                        &mut state.borrow_mut(),
                        format!("{}: {error}", i18n::text("钉住失败", "Pin failed")),
                    );
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
                i18n::text("已取消截图", "Screenshot canceled").to_owned(),
                StatusLevel::Info,
            );
        });
    }
}

fn spawn_overlay_ocr(overlay: slint::Weak<OverlayWindow>, image: CapturedImage, config: OcrConfig) {
    let width = image.width();
    let height = image.height();
    let bounds = image.bounds;
    let rgba = image.rgba_bytes();
    thread::spawn(move || {
        logging::info(format!("selection OCR started: {width}x{height}"));
        let result = CapturedImage::from_rgba(bounds.left, bounds.top, width, height, &rgba)
            .map_err(|error| OcrFailure::Failed(error.to_string()))
            .and_then(|image| recognize(&image, &config));
        let (code, text) = ocr_result_payload(result);
        logging::info(format!("selection OCR completed with code {code}"));
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
        0 => {
            let result_window = state.borrow().ocr_result.clone();
            if let Err(error) = present_ocr_result(&result_window, text) {
                logging::error(error.to_string());
                set_overlay_message(
                    overlay,
                    main,
                    state,
                    &format!("OCR 结果窗口打开失败：{error}"),
                    StatusLevel::Error,
                );
                return;
            }
            finish_capture(
                overlay,
                main,
                state,
                i18n::text("OCR 识别完成", "OCR completed").to_owned(),
                StatusLevel::Success,
            );
        }
        1 => finish_with_ocr_message(
            overlay,
            main,
            state,
            i18n::text("未识别到文字", "No text was recognized"),
            StatusLevel::Info,
        ),
        2 => finish_with_ocr_message(
            overlay,
            main,
            state,
            i18n::text(
                "缺少可用的 Windows OCR 语言包",
                "No compatible Windows OCR language pack is installed",
            ),
            StatusLevel::Error,
        ),
        3 => finish_with_ocr_message(
            overlay,
            main,
            state,
            i18n::text(
                "当前系统或程序安装方式不支持 Windows 系统 OCR",
                "Windows system OCR is not supported by this system or installation",
            ),
            StatusLevel::Error,
        ),
        _ => finish_with_ocr_message(
            overlay,
            main,
            state,
            if text.is_empty() {
                i18n::text("OCR 识别失败", "OCR failed")
            } else {
                text
            },
            StatusLevel::Error,
        ),
    }
}

fn finish_with_ocr_message(
    overlay: &slint::Weak<OverlayWindow>,
    main: &slint::Weak<MainWindow>,
    state: &Rc<RefCell<AppController>>,
    message: &str,
    level: StatusLevel,
) {
    let result_window = state.borrow().ocr_result.clone();
    let presentation = if level == StatusLevel::Error {
        logging::error(format!("OCR failed: {message}"));
        present_ocr_error(&result_window, message)
    } else {
        present_ocr_notice(&result_window, message)
    };
    if let Err(error) = presentation {
        logging::error(format!("OCR result window failed: {error}"));
        set_overlay_message(
            overlay,
            main,
            state,
            &format!("OCR 结果窗口打开失败：{error}"),
            StatusLevel::Error,
        );
        return;
    }
    finish_capture(overlay, main, state, message.to_owned(), level);
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
        Err(OcrFailure::AiUnavailable(state)) => (4, state.message()),
        Err(OcrFailure::Failed(message)) => {
            logging::error(message.as_str());
            (5, message)
        }
    }
}

fn copy_output(image: &CapturedImage, config: &CaptureConfig) -> Result<String> {
    capture::copy_to_clipboard(image)?;
    let saved = if config.auto_save {
        Some(output::save_quick(image, config)?)
    } else {
        None
    };

    Ok(match (saved, config.save_notification) {
        (Some(path), true) => format!(
            "{} {}",
            i18n::text("已复制并保存到", "Copied and saved to"),
            path.display()
        ),
        (Some(_), false) => i18n::text("已复制到剪贴板", "Copied to clipboard").to_owned(),
        (None, _) => i18n::text("已复制到剪贴板", "Copied to clipboard").to_owned(),
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
        let selected = session.desktop_snapshot.crop(left, top, width, height)?;
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

    fn add_text(&mut self, x: f32, y: f32, text: &str, font_size: i32) {
        let style = self.draw_style;
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let Some(selected) = session.selected.as_ref() else {
            return;
        };
        let point = clamp_point(x, y, selected);
        session
            .annotations
            .add_text(point, text, style, font_size.clamp(8, 96) as u32);
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
        session.annotations.render(selected)
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
    desktop_snapshot: CapturedImage,
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
