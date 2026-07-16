use std::{
    cell::RefCell,
    path::PathBuf,
    rc::{Rc, Weak as RcWeak},
    thread,
    time::Duration,
};

use anyhow::Result;
use slint::{ComponentHandle, PhysicalPosition, PhysicalSize, Timer};

use super::{
    AppController, MainWindow, PinMenuWindow, PinWindow, StatusLevel,
    capture_controller::ocr_result_payload, set_status_level,
};
use crate::{
    capture,
    config::{CaptureConfig, PinConfig},
    image::CapturedImage,
    logging, output,
    platform::{
        ocr::{OcrEngine, OcrFailure, system_engine},
        windows::{shell, window},
    },
};

pub(super) struct PinRequest {
    pub image: CapturedImage,
    pub source_path: Option<PathBuf>,
    pub pin_config: PinConfig,
    pub capture_config: CaptureConfig,
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
        } = request;
        let pin = PinWindow::new()?;
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
        }));
        let controller = PinController {
            id,
            pin: pin.as_weak(),
            state: Rc::clone(&state),
            registry: Rc::downgrade(registry),
            main,
            app,
        };
        controller.bind(&pin);

        pin.show()?;
        window::set_opacity(pin.window(), pin_config.default_opacity);
        window::set_shadow(pin.window(), pin_config.shadow);
        window::set_always_on_top(pin.window(), pin_config.always_on_top);
        registry.borrow_mut().windows.push(PinnedWindow {
            id,
            _ui: pin,
            _state: state,
        });
        Ok(())
    }
}

#[derive(Clone)]
struct PinController {
    id: u64,
    pin: slint::Weak<PinWindow>,
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
    OpenMenu,
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
    ReplaceClipboard,
    ReplaceFile,
    RevealFile,
    Transform(PinTransform),
}

impl PinController {
    fn bind(&self, pin: &PinWindow) {
        macro_rules! bind {
            ($method:ident, $command:expr) => {{
                let controller = self.clone();
                pin.$method(move || controller.dispatch($command));
            }};
        }

        bind!(on_close_pin, PinCommand::Close);
        bind!(on_open_menu, PinCommand::OpenMenu);
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
    }

    fn dispatch(&self, command: PinCommand) {
        match command {
            PinCommand::Close => self.close(),
            PinCommand::OpenMenu => {
                if let Err(error) = self.open_menu() {
                    self.status(format!("打开钉住菜单失败：{error}"), StatusLevel::Error);
                }
            }
            PinCommand::Drag => {
                if let Some(pin) = self.pin.upgrade() {
                    window::drag(pin.window());
                }
            }
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
            PinCommand::SetToolbar(enabled) => {
                if let Some(pin) = self.pin.upgrade() {
                    pin.set_toolbar_visible(enabled);
                }
            }
            PinCommand::ReplaceClipboard => self.replace_clipboard(),
            PinCommand::ReplaceFile => self.replace_file(),
            PinCommand::RevealFile => self.reveal_file(),
            PinCommand::Transform(transform) => self.transform(transform),
        }
    }

    fn close(&self) {
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

    fn copy(&self) {
        let result = capture::copy_to_clipboard(&self.state.borrow().image);
        self.report(result, "已复制钉住图像");
    }

    fn save(&self) {
        let state = self.state.borrow();
        let result = output::save_as_dialog(
            &state.image,
            &state.capture_config.save_directory,
            state.capture_config.format,
            state.capture_config.jpeg_quality,
        );
        drop(state);
        match result {
            Ok(Some(path)) => {
                self.state.borrow_mut().source_path = Some(path.clone());
                if let Some(pin) = self.pin.upgrade() {
                    pin.set_has_source_file(true);
                }
                self.status(
                    format!("图像已保存到 {}", path.display()),
                    StatusLevel::Success,
                );
            }
            Ok(None) => {}
            Err(error) => self.status(format!("图像保存失败：{error}"), StatusLevel::Error),
        }
    }

    fn ocr(&self) {
        let Some(pin) = self.pin.upgrade() else {
            return;
        };
        spawn_pin_ocr(pin.as_weak(), self.state.borrow().image.clone());
    }

    fn handle_ocr_result(&self, code: i32, text: &str) {
        match code {
            0 => self.report(capture::copy_text_to_clipboard(text), "已识别并复制文字"),
            1 => self.status("未识别到文字".to_owned(), StatusLevel::Info),
            2 => self.status("缺少中文 OCR 语言包".to_owned(), StatusLevel::Error),
            3 => self.status("当前平台不支持系统 OCR".to_owned(), StatusLevel::Error),
            _ => self.status("OCR 识别失败".to_owned(), StatusLevel::Error),
        }
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
    }

    fn replace_clipboard(&self) {
        let Some(pin) = self.pin.upgrade() else {
            return;
        };
        let position = pin.window().position();
        match capture::image_from_clipboard(position.x, position.y) {
            Ok(image) => {
                replace_pin_image(&pin, &self.state, image, None);
                self.status("已从剪贴板替换图像".to_owned(), StatusLevel::Success);
            }
            Err(error) => self.status(format!("替换图像失败：{error}"), StatusLevel::Error),
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
                self.status("已从文件替换图像".to_owned(), StatusLevel::Success);
            }
            Err(error) => self.status(format!("替换图像失败：{error}"), StatusLevel::Error),
        }
    }

    fn reveal_file(&self) {
        match self.state.borrow().source_path.clone() {
            Some(path) => self.report(shell::reveal_in_folder(&path), "已在文件夹中显示"),
            None => self.status("当前图像尚未保存".to_owned(), StatusLevel::Info),
        }
    }

    fn transform(&self, transform: PinTransform) {
        let Some(pin) = self.pin.upgrade() else {
            return;
        };
        let (image, source_path, message) = {
            let state = self.state.borrow();
            let image = match transform {
                PinTransform::RotateLeft => state.image.rotate_left(),
                PinTransform::RotateRight => state.image.rotate_right(),
                PinTransform::FlipHorizontal => state.image.flip_horizontal(),
                PinTransform::FlipVertical => state.image.flip_vertical(),
            };
            let message = match transform {
                PinTransform::RotateLeft => "图像已向左旋转",
                PinTransform::RotateRight => "图像已向右旋转",
                PinTransform::FlipHorizontal => "图像已水平翻转",
                PinTransform::FlipVertical => "图像已垂直翻转",
            };
            (image, state.source_path.clone(), message)
        };
        replace_pin_image(&pin, &self.state, image, source_path);
        self.status(message.to_owned(), StatusLevel::Success);
    }

    fn open_menu(&self) -> Result<()> {
        let Some(pin) = self.pin.upgrade() else {
            return Ok(());
        };
        let menu = PinMenuWindow::new()?;
        {
            let state = self.state.borrow();
            menu.set_alpha_percent(state.opacity as i32);
            menu.set_shadow_enabled(state.shadow);
            menu.set_top_enabled(state.always_on_top);
            menu.set_scale_percent(state.scale_percent);
            menu.set_original_size_text(
                format!("{} × {}", state.image.width(), state.image.height()).into(),
            );
            menu.set_has_source_file(state.source_path.is_some());
        }
        menu.set_toolbar_visible(pin.get_toolbar_visible());

        {
            let menu = menu.as_weak();
            menu.unwrap().on_close_menu(move || {
                if let Some(menu) = menu.upgrade() {
                    let _ = menu.hide();
                }
            });
        }

        macro_rules! bind {
            ($method:ident, $command:expr) => {{
                let controller = self.clone();
                menu.$method(move || controller.dispatch($command));
            }};
        }

        bind!(on_close_pin, PinCommand::Close);
        bind!(on_copy_image, PinCommand::Copy);
        bind!(on_save_image, PinCommand::Save);
        bind!(on_recognize_text, PinCommand::Ocr);
        bind!(on_fit_screen, PinCommand::FitScreen);
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
            menu.on_set_toolbar(move |enabled| {
                controller.dispatch(PinCommand::SetToolbar(enabled));
            });
        }
        {
            let controller = self.clone();
            menu.on_set_opacity(move |percent| {
                controller.dispatch(PinCommand::SetOpacity(percent));
            });
        }
        {
            let controller = self.clone();
            menu.on_set_shadow(move |enabled| {
                controller.dispatch(PinCommand::SetShadow(enabled));
            });
        }
        {
            let controller = self.clone();
            menu.on_set_top(move |enabled| {
                controller.dispatch(PinCommand::SetTop(enabled));
            });
        }
        {
            let controller = self.clone();
            menu.on_set_scale(move |percent| {
                controller.dispatch(PinCommand::SetScale(percent));
            });
        }

        menu.show()?;
        window::configure_context_menu(menu.window(), pin.window());
        window::place_context_menu_at_cursor(menu.window());
        window::activate(menu.window());
        monitor_pin_menu(menu.as_weak(), 0);
        logging::info("independent pin context menu opened");
        Ok(())
    }

    fn report(&self, result: Result<()>, success: &str) {
        match result {
            Ok(()) => self.status(success.to_owned(), StatusLevel::Success),
            Err(error) => self.status(format!("操作失败：{error}"), StatusLevel::Error),
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

fn monitor_pin_menu(menu: slint::Weak<PinMenuWindow>, attempts: u8) {
    Timer::single_shot(Duration::from_millis(120), move || {
        let Some(menu) = menu.upgrade() else {
            return;
        };
        if !menu.window().is_visible() {
            return;
        }
        if window::is_foreground(menu.window()) {
            monitor_pin_menu(menu.as_weak(), 0);
        } else if attempts < 2 {
            window::activate(menu.window());
            monitor_pin_menu(menu.as_weak(), attempts + 1);
        } else {
            let _ = menu.hide();
        }
    });
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

struct PinnedWindow {
    id: u64,
    _ui: PinWindow,
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
}
