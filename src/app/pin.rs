use std::{
    cell::RefCell,
    path::PathBuf,
    rc::{Rc, Weak as RcWeak},
    thread,
};

use anyhow::Result;
use slint::{ComponentHandle, ModelRc, PhysicalPosition, PhysicalSize, VecModel};

use super::{
    AnnotationView, AppController, MainWindow, PinToolbarWindow, PinWindow, StatusLevel,
    annotation::AnnotationHistory, capture_controller::ocr_result_payload, present_ocr_error,
    present_ocr_result, set_status_level,
};
use crate::{
    capture,
    config::{CaptureConfig, OcrConfig, PinConfig},
    i18n,
    image::{CapturedImage, DrawStyle},
    logging, output,
    platform::{
        ocr::{OcrFailure, recognize},
        windows::{shell, window},
    },
};

pub(super) struct PinRequest {
    pub image: CapturedImage,
    pub source_path: Option<PathBuf>,
    pub pin_config: PinConfig,
    pub capture_config: CaptureConfig,
    pub ocr_config: OcrConfig,
    pub ocr_available: bool,
}

#[derive(Default)]
pub(super) struct PinRegistry {
    next_id: u64,
    windows: Vec<PinnedWindow>,
}

impl PinRegistry {
    pub(super) fn add(
        registry: &Rc<RefCell<Self>>,
        request: PinRequest,
        main: slint::Weak<MainWindow>,
        app: RcWeak<RefCell<AppController>>,
    ) -> Result<()> {
        let PinRequest {
            image,
            source_path,
            pin_config,
            capture_config,
            ocr_config,
            ocr_available,
        } = request;
        let pin = PinWindow::new()?;
        let toolbar = PinToolbarWindow::new()?;
        pin.set_screenshot(image.slint_image());
        pin.set_alpha_percent(pin_config.default_opacity as i32);
        pin.set_shadow_enabled(pin_config.shadow);
        pin.set_top_enabled(pin_config.always_on_top);
        pin.set_wheel_zoom(pin_config.wheel_zoom);
        pin.set_zoom_step(pin_config.zoom_step as i32);
        pin.set_double_click_close(pin_config.double_click_close);
        pin.set_scale_percent(100);
        pin.set_original_size_text(format!("{} × {}", image.width(), image.height()).into());
        pin.set_has_source_file(source_path.is_some());
        pin.set_annotations(empty_annotation_model());
        pin.set_ocr_available(ocr_available);
        pin.set_color_index(0);
        pin.set_stroke_radius(2);
        toolbar.set_active_tool(0);
        toolbar.set_color_index(0);
        toolbar.set_stroke_radius(2);
        toolbar.set_ocr_available(ocr_available);
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
        let state = Rc::new(RefCell::new(PinnedWindowState {
            image,
            source_path,
            opacity: pin_config.default_opacity,
            shadow: pin_config.shadow,
            always_on_top: pin_config.always_on_top,
            scale_percent: 100,
            zoom_step: pin_config.zoom_step,
            capture_config,
            ocr_config,
            annotations: AnnotationHistory::default(),
            draw_style: DrawStyle {
                rgba: [236, 92, 102, 255],
                radius: 2,
            },
        }));
        let controller = PinController {
            id,
            pin: pin.as_weak(),
            toolbar: toolbar.as_weak(),
            state: Rc::clone(&state),
            registry: Rc::downgrade(registry),
            main,
            app,
        };
        controller.bind(&pin, &toolbar);

        pin.show()?;
        window::set_opacity(pin.window(), pin_config.default_opacity);
        window::set_shadow(pin.window(), pin_config.shadow);
        window::set_always_on_top(pin.window(), pin_config.always_on_top);
        registry.borrow_mut().windows.push(PinnedWindow {
            id,
            _ui: pin,
            _toolbar_ui: toolbar,
            _state: state,
        });
        Ok(())
    }
}

#[derive(Clone)]
struct PinController {
    id: u64,
    pin: slint::Weak<PinWindow>,
    toolbar: slint::Weak<PinToolbarWindow>,
    state: Rc<RefCell<PinnedWindowState>>,
    registry: RcWeak<RefCell<PinRegistry>>,
    main: slint::Weak<MainWindow>,
    app: RcWeak<RefCell<AppController>>,
}

#[derive(Clone, Copy)]
enum PinTransform {
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
}

enum PinCommand {
    Close,
    Drag,
    ScaleBy(i32),
    SetScale(i32),
    FitScreen,
    Copy,
    Save,
    Ocr,
    OcrResult(i32, String),
    SetOpacity(i32),
    SetShadow(bool),
    SetTop(bool),
    SetToolbar(bool),
    SetTool(i32),
    BeginAnnotation(f32, f32, i32),
    UpdateAnnotation(f32, f32),
    FinishAnnotation,
    AddText(f32, f32, String, i32),
    Undo,
    Redo,
    SetColor(i32),
    SetWidth(i32),
    ReplaceClipboard,
    ReplaceFile,
    RevealFile,
    Transform(PinTransform),
}

impl PinController {
    fn bind(&self, pin: &PinWindow, toolbar: &PinToolbarWindow) {
        macro_rules! bind {
            ($method:ident, $command:expr) => {{
                let controller = self.clone();
                pin.$method(move || controller.dispatch($command));
            }};
        }

        bind!(on_close_pin, PinCommand::Close);
        bind!(on_drag_pin, PinCommand::Drag);
        bind!(on_fit_screen, PinCommand::FitScreen);
        bind!(on_copy_image, PinCommand::Copy);
        bind!(on_save_image, PinCommand::Save);
        bind!(on_recognize_text, PinCommand::Ocr);
        bind!(on_replace_clipboard, PinCommand::ReplaceClipboard);
        bind!(on_replace_file, PinCommand::ReplaceFile);
        bind!(on_reveal_file, PinCommand::RevealFile);
        bind!(
            on_rotate_left,
            PinCommand::Transform(PinTransform::RotateLeft)
        );
        bind!(
            on_rotate_right,
            PinCommand::Transform(PinTransform::RotateRight)
        );
        bind!(
            on_flip_horizontal,
            PinCommand::Transform(PinTransform::FlipHorizontal)
        );
        bind!(
            on_flip_vertical,
            PinCommand::Transform(PinTransform::FlipVertical)
        );

        {
            let controller = self.clone();
            pin.on_set_toolbar(move |enabled| {
                controller.dispatch(PinCommand::SetToolbar(enabled));
            });
        }
        {
            let controller = self.clone();
            pin.on_scale_pin(move |direction| {
                controller.dispatch(PinCommand::ScaleBy(direction));
            });
        }
        {
            let controller = self.clone();
            pin.on_set_scale(move |percent| {
                controller.dispatch(PinCommand::SetScale(percent));
            });
        }
        {
            let controller = self.clone();
            pin.on_set_opacity(move |percent| {
                controller.dispatch(PinCommand::SetOpacity(percent));
            });
        }
        {
            let controller = self.clone();
            pin.on_set_shadow(move |enabled| {
                controller.dispatch(PinCommand::SetShadow(enabled));
            });
        }
        {
            let controller = self.clone();
            pin.on_set_top(move |enabled| {
                controller.dispatch(PinCommand::SetTop(enabled));
            });
        }
        {
            let controller = self.clone();
            pin.on_ocr_result(move |code, text| {
                controller.dispatch(PinCommand::OcrResult(code, text.to_string()));
            });
        }
        {
            let controller = self.clone();
            pin.on_begin_annotation(move |x, y, tool| {
                controller.dispatch(PinCommand::BeginAnnotation(x, y, tool));
            });
        }
        {
            let controller = self.clone();
            pin.on_update_annotation(move |x, y| {
                controller.dispatch(PinCommand::UpdateAnnotation(x, y));
            });
        }
        bind!(on_finish_annotation, PinCommand::FinishAnnotation);
        {
            let controller = self.clone();
            pin.on_add_text(move |x, y, text, font_size| {
                controller.dispatch(PinCommand::AddText(x, y, text.to_string(), font_size));
            });
        }
        bind!(on_undo, PinCommand::Undo);
        bind!(on_redo, PinCommand::Redo);

        {
            let controller = self.clone();
            toolbar.on_close_pin(move || controller.dispatch(PinCommand::Close));
        }
        {
            let controller = self.clone();
            toolbar.on_copy_image(move || controller.dispatch(PinCommand::Copy));
        }
        {
            let controller = self.clone();
            toolbar.on_save_image(move || controller.dispatch(PinCommand::Save));
        }
        {
            let controller = self.clone();
            toolbar.on_recognize_text(move || controller.dispatch(PinCommand::Ocr));
        }
        {
            let controller = self.clone();
            toolbar.on_undo(move || controller.dispatch(PinCommand::Undo));
        }
        {
            let controller = self.clone();
            toolbar.on_redo(move || controller.dispatch(PinCommand::Redo));
        }
        {
            let controller = self.clone();
            toolbar.on_set_tool(move |tool| {
                controller.dispatch(PinCommand::SetTool(tool));
            });
        }
        {
            let controller = self.clone();
            toolbar.on_select_color(move |index| {
                controller.dispatch(PinCommand::SetColor(index));
            });
        }
        {
            let controller = self.clone();
            toolbar.on_select_width(move |radius| {
                controller.dispatch(PinCommand::SetWidth(radius));
            });
        }
    }

    fn dispatch(&self, command: PinCommand) {
        match command {
            PinCommand::Close => self.close(),
            PinCommand::Drag => self.drag(),
            PinCommand::ScaleBy(direction) => {
                let current = self.state.borrow().scale_percent;
                let step = self.state.borrow().zoom_step as i32;
                self.set_scale(current + direction.signum() * step);
            }
            PinCommand::SetScale(percent) => self.set_scale(percent),
            PinCommand::FitScreen => self.fit_screen(),
            PinCommand::Copy => self.copy(),
            PinCommand::Save => self.save(),
            PinCommand::Ocr => self.ocr(),
            PinCommand::OcrResult(code, text) => self.handle_ocr_result(code, &text),
            PinCommand::SetOpacity(percent) => self.set_opacity(percent),
            PinCommand::SetShadow(enabled) => self.set_shadow(enabled),
            PinCommand::SetTop(enabled) => self.set_top(enabled),
            PinCommand::SetToolbar(enabled) => self.set_toolbar(enabled),
            PinCommand::SetTool(tool) => self.set_tool(tool),
            PinCommand::BeginAnnotation(x, y, tool) => self.begin_annotation(x, y, tool),
            PinCommand::UpdateAnnotation(x, y) => self.update_annotation(x, y),
            PinCommand::FinishAnnotation => self.finish_annotation(),
            PinCommand::AddText(x, y, text, font_size) => self.add_text(x, y, &text, font_size),
            PinCommand::Undo => self.undo(),
            PinCommand::Redo => self.redo(),
            PinCommand::SetColor(index) => self.set_color(index),
            PinCommand::SetWidth(radius) => self.set_width(radius),
            PinCommand::ReplaceClipboard => self.replace_clipboard(),
            PinCommand::ReplaceFile => self.replace_file(),
            PinCommand::RevealFile => self.reveal_file(),
            PinCommand::Transform(transform) => self.transform(transform),
        }
    }

    fn close(&self) {
        if let Some(toolbar) = self.toolbar.upgrade() {
            let _ = toolbar.hide();
        }
        if let Some(pin) = self.pin.upgrade() {
            let _ = pin.hide();
        }
        if let Some(registry) = self.registry.upgrade() {
            registry
                .borrow_mut()
                .windows
                .retain(|item| item.id != self.id);
        }
    }

    fn drag(&self) {
        let (Some(pin), Some(toolbar)) = (self.pin.upgrade(), self.toolbar.upgrade()) else {
            return;
        };
        let toolbar_visible = pin.get_toolbar_visible();
        if toolbar_visible {
            let _ = toolbar.hide();
        }
        window::drag(pin.window());
        if toolbar_visible {
            self.show_toolbar(&pin, &toolbar);
        }
    }

    fn set_toolbar(&self, enabled: bool) {
        let (Some(pin), Some(toolbar)) = (self.pin.upgrade(), self.toolbar.upgrade()) else {
            return;
        };
        pin.set_toolbar_visible(enabled);
        if enabled {
            self.show_toolbar(&pin, &toolbar);
        } else {
            pin.set_active_tool(0);
            toolbar.set_active_tool(0);
            let _ = toolbar.hide();
        }
    }

    fn show_toolbar(&self, pin: &PinWindow, toolbar: &PinToolbarWindow) {
        if let Err(error) = toolbar.show() {
            pin.set_toolbar_visible(false);
            self.status(
                format!(
                    "{}: {error}",
                    i18n::text("工具栏显示失败", "Failed to show toolbar")
                ),
                StatusLevel::Error,
            );
            return;
        }
        self.resize_toolbar(toolbar);
        window::set_owner(toolbar.window(), pin.window());
        window::set_always_on_top(toolbar.window(), self.state.borrow().always_on_top);
        window::position_below(pin.window(), toolbar.window(), 6);
    }

    fn set_tool(&self, tool: i32) {
        let tool = tool.clamp(0, 5);
        if let Some(pin) = self.pin.upgrade() {
            if tool != 4 {
                pin.invoke_commit_text_editor();
            }
            pin.set_active_tool(tool);
        }
        if let Some(toolbar) = self.toolbar.upgrade() {
            toolbar.set_active_tool(tool);
            self.resize_toolbar(&toolbar);
            if let Some(pin) = self.pin.upgrade() {
                window::position_below(pin.window(), toolbar.window(), 6);
            }
        }
    }

    fn resize_toolbar(&self, toolbar: &PinToolbarWindow) {
        let scale = toolbar.window().scale_factor();
        let logical_width = if toolbar.get_ocr_available() {
            342.0
        } else {
            312.0
        };
        let width = (logical_width * scale).round().max(1.0) as u32;
        let toolbar_height = if toolbar.get_active_tool() > 0 && toolbar.get_active_tool() < 5 {
            68.0
        } else {
            38.0
        };
        let logical_height = toolbar_height + 34.0;
        let height = (logical_height * scale).round().max(1.0) as u32;
        toolbar.window().set_size(PhysicalSize::new(width, height));
    }

    fn copy(&self) {
        if let Some(pin) = self.pin.upgrade() {
            pin.invoke_commit_text_editor();
        }
        let result = self
            .state
            .borrow()
            .rendered_image()
            .and_then(|image| capture::copy_to_clipboard(&image));
        self.report(result, i18n::text("已复制钉住图像", "Copied pinned image"));
    }

    fn save(&self) {
        if let Some(pin) = self.pin.upgrade() {
            pin.invoke_commit_text_editor();
        }
        let (image, save_directory, format, jpeg_quality) = {
            let state = self.state.borrow();
            (
                state.rendered_image(),
                state.capture_config.save_directory.clone(),
                state.capture_config.format,
                state.capture_config.jpeg_quality,
            )
        };
        let result = image.and_then(|image| {
            output::save_as_dialog(&image, &save_directory, format, jpeg_quality)
        });
        match result {
            Ok(Some(path)) => {
                self.state.borrow_mut().source_path = Some(path.clone());
                if let Some(pin) = self.pin.upgrade() {
                    pin.set_has_source_file(true);
                }
                self.status(
                    format!(
                        "{} {}",
                        i18n::text("图像已保存到", "Image saved to"),
                        path.display()
                    ),
                    StatusLevel::Success,
                );
            }
            Ok(None) => {}
            Err(error) => self.status(
                format!(
                    "{}: {error}",
                    i18n::text("图像保存失败", "Failed to save image")
                ),
                StatusLevel::Error,
            ),
        }
    }

    fn ocr(&self) {
        let Some(pin) = self.pin.upgrade() else {
            return;
        };
        self.status(
            i18n::text("正在识别文字...", "Recognizing text...").to_owned(),
            StatusLevel::Info,
        );
        let (image, config) = {
            let state = self.state.borrow();
            (state.image.clone(), state.ocr_config.clone())
        };
        spawn_pin_ocr(pin.as_weak(), image, config);
    }

    fn begin_annotation(&self, x: f32, y: f32, tool: i32) {
        {
            let mut state = self.state.borrow_mut();
            let point = annotation_point(x, y, &state.image);
            let style = state.draw_style;
            state.annotations.begin(tool, point, style);
        }
        self.refresh_annotations();
    }

    fn update_annotation(&self, x: f32, y: f32) {
        {
            let mut state = self.state.borrow_mut();
            let point = annotation_point(x, y, &state.image);
            state.annotations.update(point);
        }
        self.refresh_annotations();
    }

    fn finish_annotation(&self) {
        self.state.borrow_mut().annotations.finish();
    }

    fn add_text(&self, x: f32, y: f32, text: &str, font_size: i32) {
        {
            let mut state = self.state.borrow_mut();
            let point = annotation_point(x, y, &state.image);
            let style = state.draw_style;
            state
                .annotations
                .add_text(point, text, style, font_size.clamp(8, 96) as u32);
        }
        self.refresh_annotations();
    }

    fn undo(&self) {
        self.state.borrow_mut().annotations.undo();
        self.refresh_annotations();
    }

    fn redo(&self) {
        self.state.borrow_mut().annotations.redo();
        self.refresh_annotations();
    }

    fn set_color(&self, index: i32) {
        let mut state = self.state.borrow_mut();
        state.draw_style.rgba = match index {
            0 => [236, 92, 102, 255],
            1 => [74, 144, 226, 255],
            2 => [49, 163, 107, 255],
            3 => [245, 197, 66, 255],
            _ => state.draw_style.rgba,
        };
        drop(state);
        if let Some(toolbar) = self.toolbar.upgrade() {
            toolbar.set_color_index(index.clamp(0, 3));
        }
        if let Some(pin) = self.pin.upgrade() {
            pin.set_color_index(index.clamp(0, 3));
        }
    }

    fn set_width(&self, radius: i32) {
        let radius = radius.clamp(1, 12);
        self.state.borrow_mut().draw_style.radius = radius;
        if let Some(toolbar) = self.toolbar.upgrade() {
            toolbar.set_stroke_radius(radius);
        }
        if let Some(pin) = self.pin.upgrade() {
            pin.set_stroke_radius(radius);
        }
    }

    fn refresh_annotations(&self) {
        let Some(pin) = self.pin.upgrade() else {
            return;
        };
        let views = self.state.borrow().annotations.views();
        pin.set_annotations(ModelRc::new(VecModel::from(views)));
    }

    fn handle_ocr_result(&self, code: i32, text: &str) {
        match code {
            0 => {
                let Some(app) = self.app.upgrade() else {
                    self.status(
                        i18n::text(
                            "OCR 结果窗口已不可用",
                            "The OCR result window is unavailable",
                        )
                        .to_owned(),
                        StatusLevel::Error,
                    );
                    return;
                };
                let result_window = app.borrow().ocr_result.clone();
                match present_ocr_result(&result_window, text) {
                    Ok(()) => self.status(
                        i18n::text("OCR 识别完成", "OCR completed").to_owned(),
                        StatusLevel::Success,
                    ),
                    Err(error) => {
                        self.status(format!("OCR 结果窗口打开失败：{error}"), StatusLevel::Error)
                    }
                }
            }
            1 => self.status(
                i18n::text("未识别到文字", "No text was recognized").to_owned(),
                StatusLevel::Info,
            ),
            2 => self.show_ocr_error(i18n::text(
                "缺少可用的 Windows OCR 语言包",
                "No compatible Windows OCR language pack is installed",
            )),
            3 => self.show_ocr_error(i18n::text(
                "当前系统或程序安装方式不支持 Windows 系统 OCR",
                "Windows system OCR is not supported by this system or installation",
            )),
            _ => self.show_ocr_error(if text.is_empty() {
                i18n::text("OCR 识别失败", "OCR failed")
            } else {
                text
            }),
        }
    }

    fn show_ocr_error(&self, message: &str) {
        logging::error(format!("OCR failed: {message}"));
        if let Some(app) = self.app.upgrade() {
            let result_window = app.borrow().ocr_result.clone();
            if let Err(error) = present_ocr_error(&result_window, message) {
                logging::error(format!("OCR result window failed: {error}"));
            }
        }
        self.status(
            format!("{}: {message}", i18n::text("OCR 识别失败", "OCR failed")),
            StatusLevel::Error,
        );
    }

    fn set_opacity(&self, percent: i32) {
        let Some(pin) = self.pin.upgrade() else {
            return;
        };
        let percent = percent.clamp(25, 100) as u8;
        self.state.borrow_mut().opacity = percent;
        pin.set_alpha_percent(percent as i32);
        window::set_opacity(pin.window(), percent);
    }

    fn set_shadow(&self, enabled: bool) {
        let Some(pin) = self.pin.upgrade() else {
            return;
        };
        self.state.borrow_mut().shadow = enabled;
        pin.set_shadow_enabled(enabled);
        window::set_shadow(pin.window(), enabled);
    }

    fn set_top(&self, enabled: bool) {
        let Some(pin) = self.pin.upgrade() else {
            return;
        };
        self.state.borrow_mut().always_on_top = enabled;
        pin.set_top_enabled(enabled);
        window::set_always_on_top(pin.window(), enabled);
        if let Some(toolbar) = self.toolbar.upgrade() {
            window::set_always_on_top(toolbar.window(), enabled);
        }
    }

    fn set_scale(&self, percent: i32) {
        let Some(pin) = self.pin.upgrade() else {
            return;
        };
        let percent = percent.clamp(10, 800);
        let state = self.state.borrow();
        let width = ((state.image.width() as u64 * percent as u64) / 100).max(1) as u32;
        let height = ((state.image.height() as u64 * percent as u64) / 100).max(1) as u32;
        drop(state);
        self.state.borrow_mut().scale_percent = percent;
        pin.set_scale_percent(percent);
        pin.window().set_size(PhysicalSize::new(width, height));
        self.position_toolbar();
    }

    fn fit_screen(&self) {
        let Some(pin) = self.pin.upgrade() else {
            return;
        };
        let state = self.state.borrow();
        window::fit_to_work_area(pin.window(), state.image.width(), state.image.height());
        let size = pin.window().size();
        let scale = ((size.width as f64 / state.image.width() as f64) * 100.0).round() as i32;
        drop(state);
        self.state.borrow_mut().scale_percent = scale;
        pin.set_scale_percent(scale);
        self.position_toolbar();
    }

    fn replace_clipboard(&self) {
        let Some(pin) = self.pin.upgrade() else {
            return;
        };
        let position = pin.window().position();
        match capture::image_from_clipboard(position.x, position.y) {
            Ok(image) => {
                replace_pin_image(&pin, &self.state, image, None);
                self.reset_toolbar_tool();
                self.position_toolbar();
                self.status(
                    i18n::text("已从剪贴板替换图像", "Image replaced from clipboard").to_owned(),
                    StatusLevel::Success,
                );
            }
            Err(error) => self.status(
                format!(
                    "{}: {error}",
                    i18n::text("替换图像失败", "Failed to replace image")
                ),
                StatusLevel::Error,
            ),
        }
    }

    fn replace_file(&self) {
        let Some(pin) = self.pin.upgrade() else {
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
                replace_pin_image(&pin, &self.state, image, Some(path));
                self.reset_toolbar_tool();
                self.position_toolbar();
                self.status(
                    i18n::text("已从文件替换图像", "Image replaced from file").to_owned(),
                    StatusLevel::Success,
                );
            }
            Err(error) => self.status(
                format!(
                    "{}: {error}",
                    i18n::text("替换图像失败", "Failed to replace image")
                ),
                StatusLevel::Error,
            ),
        }
    }

    fn reveal_file(&self) {
        match self.state.borrow().source_path.clone() {
            Some(path) => self.report(
                shell::reveal_in_folder(&path),
                i18n::text("已在文件夹中显示", "Shown in folder"),
            ),
            None => self.status(
                i18n::text("当前图像尚未保存", "The current image has not been saved").to_owned(),
                StatusLevel::Info,
            ),
        }
    }

    fn transform(&self, transform: PinTransform) {
        let Some(pin) = self.pin.upgrade() else {
            return;
        };
        let rendered = match self.state.borrow().rendered_image() {
            Ok(image) => image,
            Err(error) => {
                self.status(
                    format!(
                        "{}: {error}",
                        i18n::text("图像处理失败", "Image processing failed")
                    ),
                    StatusLevel::Error,
                );
                return;
            }
        };
        let (image, source_path, message) = {
            let state = self.state.borrow();
            let image = match transform {
                PinTransform::RotateLeft => rendered.rotate_left(),
                PinTransform::RotateRight => rendered.rotate_right(),
                PinTransform::FlipHorizontal => rendered.flip_horizontal(),
                PinTransform::FlipVertical => rendered.flip_vertical(),
            };
            let message = match transform {
                PinTransform::RotateLeft => i18n::text("图像已向左旋转", "Image rotated left"),
                PinTransform::RotateRight => i18n::text("图像已向右旋转", "Image rotated right"),
                PinTransform::FlipHorizontal => {
                    i18n::text("图像已水平翻转", "Image flipped horizontally")
                }
                PinTransform::FlipVertical => {
                    i18n::text("图像已垂直翻转", "Image flipped vertically")
                }
            };
            (image, state.source_path.clone(), message)
        };
        replace_pin_image(&pin, &self.state, image, source_path);
        self.reset_toolbar_tool();
        self.position_toolbar();
        self.status(message.to_owned(), StatusLevel::Success);
    }

    fn reset_toolbar_tool(&self) {
        if let Some(toolbar) = self.toolbar.upgrade() {
            toolbar.set_active_tool(0);
            self.resize_toolbar(&toolbar);
        }
    }

    fn position_toolbar(&self) {
        let (Some(pin), Some(toolbar)) = (self.pin.upgrade(), self.toolbar.upgrade()) else {
            return;
        };
        if pin.get_toolbar_visible() {
            window::position_below(pin.window(), toolbar.window(), 6);
        }
    }

    fn report(&self, result: Result<()>, success: &str) {
        match result {
            Ok(()) => self.status(success.to_owned(), StatusLevel::Success),
            Err(error) => self.status(
                format!("{}: {error}", i18n::text("操作失败", "Operation failed")),
                StatusLevel::Error,
            ),
        }
    }

    fn status(&self, message: String, level: StatusLevel) {
        if let Some(app) = self.app.upgrade() {
            set_status_level(&self.main, &mut app.borrow_mut(), message, level);
        } else if let Some(main) = self.main.upgrade() {
            main.set_status_text(message.into());
            main.set_status_level(level as i32);
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
        state.annotations.clear();
    }
    pin.set_screenshot(image.slint_image());
    pin.set_annotations(empty_annotation_model());
    pin.set_active_tool(0);
    pin.set_original_size_text(format!("{width} × {height}").into());
    pin.set_scale_percent(100);
    pin.set_has_source_file(state.borrow().source_path.is_some());
    pin.window().set_size(PhysicalSize::new(width, height));
}

fn spawn_pin_ocr(pin: slint::Weak<PinWindow>, image: CapturedImage, config: OcrConfig) {
    let width = image.width();
    let height = image.height();
    let bounds = image.bounds;
    let rgba = image.rgba_bytes();
    thread::spawn(move || {
        logging::info(format!("pin OCR started: {width}x{height}"));
        let result = CapturedImage::from_rgba(bounds.left, bounds.top, width, height, &rgba)
            .map_err(|error| OcrFailure::Failed(error.to_string()))
            .and_then(|image| recognize(&image, &config));
        let (code, text) = ocr_result_payload(result);
        logging::info(format!("pin OCR completed with code {code}"));
        let _ = pin.upgrade_in_event_loop(move |pin| {
            pin.invoke_ocr_result(code, text.into());
        });
    });
}

struct PinnedWindow {
    id: u64,
    _ui: PinWindow,
    _toolbar_ui: PinToolbarWindow,
    _state: Rc<RefCell<PinnedWindowState>>,
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
    ocr_config: OcrConfig,
    annotations: AnnotationHistory,
    draw_style: DrawStyle,
}

impl PinnedWindowState {
    fn rendered_image(&self) -> Result<CapturedImage> {
        self.annotations.render(&self.image)
    }
}

fn annotation_point(x: f32, y: f32, image: &CapturedImage) -> (u32, u32) {
    (
        x.clamp(0.0, image.width().saturating_sub(1) as f32) as u32,
        y.clamp(0.0, image.height().saturating_sub(1) as f32) as u32,
    )
}

fn empty_annotation_model() -> ModelRc<AnnotationView> {
    ModelRc::new(VecModel::from(Vec::<AnnotationView>::new()))
}
