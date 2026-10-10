//! Screenshot and pin interaction. Both modes share one annotation editor.
//! GPUI provides input/focus/canvas; Win32 integration is restricted to physical
//! desktop positioning and toolbar ownership. The toolbar is a separate window so small pins need not
//! be enlarged to accommodate controls; its controls use official components.
use super::{
    Message, SharedApp,
    annotation::AnnotationHistory,
    input,
    toolbar_layout::{Metrics, Rect, ToolbarLayout},
};
use crate::{
    capture, i18n,
    image::{CapturedImage, DrawStyle, TextFont},
    logging, output,
    platform::{
        ocr,
        windows::{
            shell,
            text::{self as native_text, FontChoice},
            tooltip::{NativeTooltips, TipRegion},
            window as native,
            window_target::WindowTargets,
        },
    },
};
use anyhow::{Result, anyhow};
use gpui_kit::component::{
    button::*,
    input::{InputEvent, InputState, NumberInput, NumberStep},
    native_menu::NativeMenu,
    radio::{Radio, RadioGroup},
    scroll::ScrollableElement,
    *,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::{Arc, mpsc},
    time::Duration,
};
use windows::Win32::Foundation::RECT;

gpui_kit::actions!(
    shitu_editor,
    [
        CopyImage,
        SaveImage,
        Recognize,
        Close,
        ToggleToolbar,
        ToggleTop,
        ToggleShadow,
        OriginalSize,
        FitScreen,
        RotateLeft,
        RotateRight,
        FlipHorizontal,
        FlipVertical,
        ReplaceClipboard,
        ReplaceFile,
        RevealFile,
        Opacity25,
        Opacity50,
        Opacity75,
        Opacity90,
        Opacity100,
        Scale25,
        Scale50,
        Scale75,
        Scale125,
        Scale150,
        Scale200
    ]
);

const COLORS: [[u8; 4]; 12] = [
    [236, 92, 102, 255],
    [74, 144, 226, 255],
    [49, 163, 107, 255],
    [245, 197, 66, 255],
    [242, 153, 74, 255],
    [155, 81, 224, 255],
    [235, 101, 160, 255],
    [25, 181, 197, 255],
    [32, 33, 36, 255],
    [255, 255, 255, 255],
    [139, 149, 165, 255],
    [155, 107, 67, 255],
];

// One compact command row; expand only the active tool's relevant properties.
const TOOLBAR_WIDTH: f64 = 462.;
const TOOLBAR_MAIN_HEIGHT: f64 = 38.;
fn property_height(tool: i32, font_size: f32) -> f64 {
    match tool {
        5 | 6 => 40.,
        4 => (font_size as f64 * 1.5 + 8.).max(72.),
        1..=3 | 7 => 64.,
        _ => 0.,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Region {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}
impl Region {
    fn contains(self, p: (u32, u32)) -> bool {
        p.0 >= self.x && p.1 >= self.y && p.0 < self.x + self.width && p.1 < self.y + self.height
    }
}
#[derive(Clone, Copy)]
enum Gesture {
    Select((u32, u32)),
    Move(Region, (u32, u32)),
    Resize(Region, i32),
    Draw,
}
#[derive(Clone)]
enum Command {
    Tool(i32),
    Color(usize),
    Number(NumberKind, i32),
    AdjustNumber(NumberKind, i32),
    Dashed(bool),
    Undo,
    Redo,
    Copy,
    Save,
    Ocr,
    Pin,
    Close,
    ToggleToolbar,
    ToggleTop,
    ToggleShadow,
    Scale(i32),
    Fit,
    Transform(u8),
    ReplaceClipboard,
    ReplaceFile,
    RevealFile,
    Opacity(u8),
    CommitText,
    ChooseFont,
    FontChosen(std::result::Result<Option<FontChoice>, String>),
}

struct Editor {
    shared: SharedApp,
    source: CapturedImage,
    source_path: Option<PathBuf>,
    desktop: Option<WindowTargets>,
    region: Option<Region>,
    hover: Option<Region>,
    base: Option<CapturedImage>,
    background: Arc<RenderImage>,
    preview: Option<Arc<RenderImage>>,
    annotations: AnnotationHistory,
    gesture: Option<Gesture>,
    region_before: Option<Region>,
    canvas_bounds: Rc<Cell<Bounds<Pixels>>>,
    focus: FocusHandle,
    tool: i32,
    style: DrawStyle,
    radii: [i32; 9],
    brush_position: Option<(u32, u32)>,
    text_size: u32,
    text_font: TextFont,
    font_dialog_open: bool,
    text: Entity<InputState>,
    text_position: Option<(u32, u32)>,
    commands: mpsc::Receiver<Command>,
    sender: mpsc::Sender<Command>,
    ocr_receiver: mpsc::Receiver<Result<String, ocr::OcrFailure>>,
    ocr_sender: mpsc::Sender<Result<String, ocr::OcrFailure>>,
    busy: bool,
    toolbar: Option<AnyWindowHandle>,
    toolbar_visible: bool,
    layout: ToolbarLayout,
    properties_above: bool,
    scale: i32,
    opacity: u8,
    top: bool,
    shadow: bool,
    status: String,
    _subscriptions: Vec<Subscription>,
}

pub(super) fn open_capture(shared: SharedApp, cx: &mut App) -> Result<()> {
    let bounds = capture::virtual_desktop_bounds()?;
    let targets = WindowTargets::snapshot(bounds)?;
    let image = capture::capture_region(bounds)?;
    logging::info("capture snapshot prepared");
    open(image, None, Some(targets), shared, cx)
}
pub(super) fn open_pin(
    image: CapturedImage,
    path: Option<PathBuf>,
    shared: SharedApp,
    cx: &mut App,
) -> Result<()> {
    open(image, path, None, shared, cx)
}

fn open(
    image: CapturedImage,
    path: Option<PathBuf>,
    targets: Option<WindowTargets>,
    shared: SharedApp,
    cx: &mut App,
) -> Result<()> {
    let bounds = image.bounds;
    let is_capture = targets.is_some();
    let pin_config = shared.borrow().config.pin.clone();
    let options = WindowOptions {
        titlebar: None,
        kind: WindowKind::PopUp,
        is_resizable: false,
        is_minimizable: false,
        focus: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let (handle, editor) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| Editor::new(image, path, targets, shared, window, cx))
    })?;
    logging::info("editor window created");
    let setup = handle
        .update(cx, |_, window, cx| -> Result<()> {
            // Do not use WindowOptions::show=false here: in GPUI 0.3.8 a
            // later activate() reapplies its deferred initial placement and
            // overrides our virtual-desktop rectangle. Hide the created window
            // while configuring it instead, after that placement is consumed.
            native::hide(window)?;
            // Component's root plugin paints an opaque theme surface by
            // default. Its documented Styled override keeps pin alpha intact.
            let root = window
                .root::<base::Root>()
                .flatten()
                .ok_or_else(|| anyhow!("Editor requires GPUI Base Root"))?;
            root.update(cx, |root, cx| {
                root.style()
                    .refine(&StyleRefinement::default().bg(rgba(0x00000000)));
                cx.notify();
            });
            native::prepare_image_window(window, is_capture)?;
            native::place(
                window,
                RECT {
                    left: bounds.left,
                    top: bounds.top,
                    right: bounds.left + bounds.width,
                    bottom: bounds.top + bounds.height,
                },
            )?;
            if is_capture {
                logging::info(format!(
                    "capture geometry: source={}x{} at {},{}; window={:?}; viewport={:?}; scale={}",
                    bounds.width,
                    bounds.height,
                    bounds.left,
                    bounds.top,
                    native::bounds(window)?,
                    window.viewport_size(),
                    window.scale_factor()
                ));
            }
            if !is_capture {
                native::set_always_on_top(window, pin_config.always_on_top)?;
                native::set_shadow(window, pin_config.shadow)?;
            } else {
                native::set_always_on_top(window, true)?;
            }
            editor.update(cx, |editor, cx| {
                editor.focus.focus(window, cx);
                cx.notify();
            });
            window.set_window_title(if is_capture {
                i18n::text("截图")
            } else {
                i18n::text("钉住")
            });
            Ok(())
        })
        .and_then(|r| r);
    if let Err(error) = setup {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
        return Err(error);
    }
    // Resize callbacks update GPUI's viewport after the setup borrow ends.
    // Show only then, rather than flashing the default-sized stretched image.
    cx.defer(move |cx| {
        if let Err(error) = handle
            .update(cx, |_, window, _| {
                native::show_without_activation(window)?;
                window.activate_window();
                Ok::<_, anyhow::Error>(())
            })
            .and_then(|r| r)
        {
            logging::error(format!("Show image editor: {error:#}"));
        }
    });
    if !is_capture
        && let Err(error) = handle
            .update(cx, |_, window, cx| {
                editor.update(cx, |editor, cx| editor.ensure_toolbar(window, cx))
            })
            .and_then(|r| r)
    {
        let _ = handle.update(cx, |_, window, _| window.remove_window());
        return Err(error);
    }
    Ok(())
}

fn render_image(image: &CapturedImage) -> Arc<RenderImage> {
    let mut bytes = image.rgba_bytes();
    // RenderImage explicitly expects BGRA, while our business image is RGBA.
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let image = image::RgbaImage::from_raw(image.width(), image.height(), bytes)
        .expect("validated image dimensions");
    Arc::new(RenderImage::new(vec![image::Frame::new(image)]))
}

impl Editor {
    fn new(
        source: CapturedImage,
        path: Option<PathBuf>,
        desktop: Option<WindowTargets>,
        shared: SharedApp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let capture = desktop.is_some();
        let config = shared.borrow().config.pin.clone();
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let (sender, commands) = mpsc::channel();
        let (ocr_sender, ocr_receiver) = mpsc::channel();
        let text = input("", window, cx);
        let text_sender = sender.clone();
        let text_subscription = cx.subscribe(&text, move |_, _, event, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                let _ = text_sender.send(Command::CommitText);
                cx.notify();
            }
        });
        let release_subscription = cx.on_release(|editor, cx| {
            if let Some(toolbar) = editor.toolbar.take() {
                let _ = toolbar.update(cx, |_, window, _| window.remove_window());
            }
            if editor.desktop.is_some() {
                let _ = editor.shared.borrow().sender.send(Message::CaptureClosed);
            }
        });
        let bounds_subscription = cx.observe_window_bounds(window, |editor, window, cx| {
            if let Err(error) = editor.position_toolbar(window, cx) {
                editor.error(error);
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(30))
                    .await;
                if cx
                    .update(|window, cx| this.update(cx, |editor, cx| editor.poll(window, cx)))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let base = (!capture).then(|| source.clone());
        let background = render_image(&source);
        Self {
            shared,
            source,
            source_path: path,
            desktop,
            region: None,
            hover: None,
            base,
            background: background.clone(),
            preview: (!capture).then_some(background),
            annotations: AnnotationHistory::default(),
            gesture: None,
            region_before: None,
            canvas_bounds: Rc::new(Cell::new(Bounds::default())),
            focus,
            tool: 0,
            style: DrawStyle {
                rgba: COLORS[0],
                radius: 2,
                dashed: false,
            },
            radii: [2, 2, 2, 2, 2, 10, 18, 2, 2],
            brush_position: None,
            text_size: 20,
            text_font: TextFont::default(),
            font_dialog_open: false,
            text,
            text_position: None,
            commands,
            sender,
            ocr_receiver,
            ocr_sender,
            busy: false,
            toolbar: None,
            toolbar_visible: !capture,
            layout: ToolbarLayout::default(),
            properties_above: false,
            scale: 100,
            opacity: if capture { 100 } else { config.default_opacity },
            top: config.always_on_top,
            shadow: config.shadow,
            status: String::new(),
            _subscriptions: vec![text_subscription, release_subscription, bounds_subscription],
        }
    }
    fn error(&mut self, error: anyhow::Error) {
        self.status = format!("{}: {error:#}", i18n::text("操作失败"));
        logging::error(&self.status);
        let _ = self
            .shared
            .borrow()
            .sender
            .send(Message::Status(self.status.clone()));
    }
    fn status(&mut self, message: String) {
        self.status = message.clone();
        let _ = self.shared.borrow().sender.send(Message::Status(message));
    }
    fn refresh(&mut self) -> Result<()> {
        if let Some(base) = &self.base {
            self.preview = Some(render_image(&self.annotations.render(base)?));
        }
        Ok(())
    }
    fn rendered(&self) -> Result<CapturedImage> {
        self.annotations.render(
            self.base
                .as_ref()
                .ok_or_else(|| anyhow!(i18n::text("尚未选择截图区域")))?,
        )
    }
    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Ok(command) = self.commands.try_recv() {
            let reflow = matches!(
                command,
                Command::Tool(_)
                    | Command::Number(NumberKind::Font, _)
                    | Command::AdjustNumber(NumberKind::Font, _)
                    | Command::FontChosen(_)
            );
            if let Err(error) = self.command(command, window, cx) {
                self.error(error);
            }
            if reflow && let Err(error) = self.position_toolbar(window, cx) {
                self.error(error);
            }
            cx.notify();
        }
        while let Ok(result) = self.ocr_receiver.try_recv() {
            self.busy = false;
            let _ = self.shared.borrow().sender.send(Message::Ocr(result));
            if self.desktop.is_some() {
                window.remove_window();
            }
            cx.notify();
        }
    }
    fn command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        if self.font_dialog_open && !matches!(command, Command::FontChosen(_)) {
            return Ok(());
        }
        if self.busy && !matches!(command, Command::Close) {
            return Ok(());
        }
        match command {
            Command::ChooseFont => {
                if self.tool != 4 {
                    return Ok(());
                }
                let owner = native::hwnd(window)?.0 as isize;
                let choice = FontChoice {
                    font: self.text_font.clone(),
                    size: self.text_size,
                };
                let sender = self.sender.clone();
                std::thread::Builder::new()
                    .name("font-dialog".into())
                    .spawn(move || {
                        let result = native_text::choose_font(owner, choice)
                            .map_err(|error| format!("{error:#}"));
                        let _ = sender.send(Command::FontChosen(result));
                    })?;
                self.font_dialog_open = true;
            }
            Command::FontChosen(result) => {
                self.font_dialog_open = false;
                if let Some(choice) = result.map_err(|error| anyhow!(error))? {
                    self.text_font = choice.font;
                    self.text_size = choice.size;
                }
                if self.text_position.is_some() {
                    self.text.update(cx, |input, cx| input.focus(window, cx));
                } else {
                    self.focus.focus(window, cx);
                }
            }
            Command::Close => {
                window.remove_window();
                return Ok(());
            }
            Command::Tool(tool) => {
                self.commit_text(window, cx)?;
                self.annotations.finish();
                self.tool = if self.tool == tool { 0 } else { tool };
                self.style.radius = self.radii[self.tool as usize];
                self.brush_position = None;
                window.activate_window();
                self.focus.focus(window, cx);
            }
            Command::Color(index) => self.style.rgba = COLORS[index],
            Command::Number(kind, value) => self.set_number(kind, value),
            Command::AdjustNumber(kind, steps) => {
                let current = match kind {
                    NumberKind::Font => self.text_size as i32,
                    NumberKind::Width(tool) => self.radii[tool as usize] * 2,
                };
                self.set_number(kind, current + steps * kind.step());
            }
            Command::Dashed(value) => self.style.dashed = value,
            Command::CommitText => self.commit_text(window, cx)?,
            Command::Undo => {
                self.annotations.undo();
                self.refresh()?;
            }
            Command::Redo => {
                self.annotations.redo();
                self.refresh()?;
            }
            Command::Copy | Command::Save | Command::Pin | Command::Ocr => {
                self.commit_text(window, cx)?;
                let image = self.rendered()?;
                let config = self.shared.borrow().config.clone();
                match command {
                    Command::Copy => {
                        capture::copy_to_clipboard(&image)?;
                        let path = if config.capture.auto_save {
                            Some(output::save_quick(&image, &config.capture)?)
                        } else {
                            None
                        };
                        self.status(if config.capture.save_notification {
                            path.map(|p| {
                                format!("{} {}", i18n::text("已复制并保存到"), p.display())
                            })
                            .unwrap_or_else(|| i18n::text("已复制到剪贴板").to_owned())
                        } else {
                            i18n::text("已复制到剪贴板").to_owned()
                        });
                        if self.desktop.is_some() {
                            window.remove_window();
                        }
                    }
                    Command::Save => {
                        if let Some(path) = output::save_as_dialog(
                            window,
                            &image,
                            &config.capture.save_directory,
                            config.capture.format,
                            config.capture.jpeg_quality,
                        )? {
                            self.source_path = Some(path.clone());
                            self.status(path.display().to_string());
                            if self.desktop.is_some() {
                                window.remove_window();
                            }
                        }
                    }
                    Command::Pin => {
                        let path = if config.capture.auto_save {
                            Some(output::save_quick(&image, &config.capture)?)
                        } else {
                            None
                        };
                        self.shared
                            .borrow()
                            .sender
                            .send(Message::Pin(image, path))?;
                        window.remove_window();
                    }
                    Command::Ocr => {
                        self.busy = true;
                        let sender = self.ocr_sender.clone();
                        std::thread::spawn(move || {
                            let _ = sender.send(ocr::recognize(&image, &config.ocr));
                        });
                    }
                    _ => unreachable!(),
                }
            }
            Command::ToggleToolbar => {
                self.toolbar_visible = !self.toolbar_visible;
                if self.toolbar_visible {
                    self.ensure_toolbar(window, cx)?;
                } else if let Some(toolbar) = self.toolbar {
                    toolbar.update(cx, |_, w, _| native::hide(w))??;
                }
            }
            Command::ToggleTop => {
                let value = !self.top;
                native::set_always_on_top(window, value)?;
                self.top = value;
                if let Some(toolbar) = self.toolbar {
                    toolbar.update(cx, |_, w, _| native::set_always_on_top(w, value))??;
                }
            }
            Command::ToggleShadow => {
                let value = !self.shadow;
                native::set_shadow(window, value)?;
                self.shadow = value;
            }
            Command::Opacity(value) => self.opacity = value.clamp(25, 100),
            Command::Scale(percent) => self.set_scale(percent, window, cx)?,
            Command::Fit => {
                let work = native::work_area_for_rect(native::bounds(window)?)?;
                let (width, height) = fitted_size(
                    self.source.width(),
                    self.source.height(),
                    (work.right - work.left - 24).max(1) as u32,
                    (work.bottom - work.top - 24).max(1) as u32,
                );
                let x = work.left + (work.right - work.left - width as i32) / 2;
                let y = work.top + (work.bottom - work.top - height as i32) / 2;
                native::place(
                    window,
                    RECT {
                        left: x,
                        top: y,
                        right: x + width as i32,
                        bottom: y + height as i32,
                    },
                )?;
                self.scale = (width as f64 / self.source.width() as f64 * 100.).round() as i32;
                self.position_toolbar(window, cx)?;
            }
            Command::Transform(kind) => {
                let image = self.rendered()?;
                let image = match kind {
                    0 => image.rotate_left(),
                    1 => image.rotate_right(),
                    2 => image.flip_horizontal(),
                    _ => image.flip_vertical(),
                };
                self.replace(image, self.source_path.clone(), window, cx)?;
            }
            Command::ReplaceClipboard => {
                let bounds = native::bounds(window)?;
                let image = capture::image_from_clipboard(bounds.left, bounds.top)?;
                self.replace(image, None, window, cx)?;
            }
            Command::ReplaceFile => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_parent(window)
                    .add_filter(i18n::text("图像"), &["png", "jpg", "jpeg"])
                    .pick_file()
                {
                    let bounds = native::bounds(window)?;
                    let image = CapturedImage::from_file(&path, bounds.left, bounds.top)?;
                    self.replace(image, Some(path), window, cx)?;
                }
            }
            Command::RevealFile => {
                shell::reveal_in_folder(
                    self.source_path
                        .as_ref()
                        .ok_or_else(|| anyhow!(i18n::text("当前图像尚未保存")))?,
                )?;
            }
        }
        Ok(())
    }
    fn replace(
        &mut self,
        image: CapturedImage,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        self.source = image.clone();
        self.base = Some(image);
        self.background = render_image(&self.source);
        self.source_path = path;
        self.annotations.clear();
        self.tool = 0;
        self.text_position = None;
        self.refresh()?;
        self.set_scale(100, window, cx)
    }
    fn set_scale(
        &mut self,
        percent: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let percent = percent.clamp(10, 800);
        let old = native::bounds(window)?;
        let width = (self.source.width() as f64 * percent as f64 / 100.)
            .round()
            .max(1.) as i32;
        let height = (self.source.height() as f64 * percent as f64 / 100.)
            .round()
            .max(1.) as i32;
        native::place(
            window,
            RECT {
                left: old.left,
                top: old.top,
                right: old.left + width,
                bottom: old.top + height,
            },
        )?;
        self.scale = percent;
        self.position_toolbar(window, cx)
    }
    fn point(&self, point: Point<Pixels>) -> (u32, u32) {
        let bounds = self.canvas_bounds.get();
        let x = f32::from(point.x - bounds.origin.x) / f32::from(bounds.size.width).max(1.);
        let y = f32::from(point.y - bounds.origin.y) / f32::from(bounds.size.height).max(1.);
        (
            (x * self.source.width() as f32)
                .round()
                .clamp(0., self.source.width() as f32) as u32,
            (y * self.source.height() as f32)
                .round()
                .clamp(0., self.source.height() as f32) as u32,
        )
    }
    fn set_number(&mut self, kind: NumberKind, value: i32) {
        let value = kind.normalize(value);
        match kind {
            NumberKind::Font => self.text_size = value as u32,
            NumberKind::Width(tool) => {
                self.radii[tool as usize] = value / 2;
                if self.tool == tool {
                    self.style.radius = value / 2;
                }
            }
        }
    }
    fn local_point(&self, point: (u32, u32)) -> (u32, u32) {
        let origin = self.region.map(|r| (r.x, r.y)).unwrap_or((0, 0));
        let image = self.base.as_ref().unwrap_or(&self.source);
        (
            point.0.saturating_sub(origin.0).min(image.width() - 1),
            point.1.saturating_sub(origin.1).min(image.height() - 1),
        )
    }
    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.font_dialog_open {
            return;
        }
        self.focus.focus(window, cx);
        let p = self.point(event.position);
        if matches!(self.tool, 5 | 6) {
            self.brush_position = Some(p);
        }
        if self.desktop.is_none() && self.tool == 0 && event.click_count == 2 {
            // Consume the caption double-click even when close is disabled;
            // don't let DefWindowProc turn it into a maximize action.
            cx.stop_propagation();
            if self.shared.borrow().config.pin.double_click_close {
                window.remove_window();
            }
            return;
        }
        if let Err(error) = self.commit_text(window, cx) {
            self.error(error);
            return;
        }
        if self.desktop.is_some() && self.tool == 0 {
            self.region_before = self.region;
            self.gesture = Some(if let Some(region) = self.region {
                if let Some(corner) = corner_at(region, p, self.tolerance()) {
                    Gesture::Resize(region, corner)
                } else if region.contains(p) {
                    Gesture::Move(region, p)
                } else {
                    Gesture::Select(p)
                }
            } else {
                Gesture::Select(p)
            });
        } else if self.tool != 0 && self.base.is_some() && self.region.is_none_or(|r| r.contains(p))
        {
            let p = self.local_point(p);
            if self.tool == 4 {
                self.text_position = Some(p);
                self.text.update(cx, |state, cx| {
                    state.set_value("", window, cx);
                    state.focus(window, cx);
                });
            } else {
                let selected = self.annotations.selection_bounds().map(|b| Region {
                    x: b.left,
                    y: b.top,
                    width: b.width,
                    height: b.height,
                });
                if self.tool == 8
                    && let Some(handle) = selected.and_then(|r| corner_at(r, p, self.tolerance()))
                {
                    self.annotations.begin_edit(p, handle);
                } else {
                    self.annotations.begin(self.tool, p, self.style);
                }
                self.gesture = Some(Gesture::Draw);
                if let Err(error) = self.refresh() {
                    self.error(error);
                }
            }
        }
        cx.notify();
    }
    fn tolerance(&self) -> u32 {
        ((self.source.width() as f32 / f32::from(self.canvas_bounds.get().size.width).max(1.)) * 6.)
            .ceil() as u32
    }
    fn mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.font_dialog_open {
            return;
        }
        let p = self.point(event.position);
        if matches!(self.tool, 5 | 6) {
            self.brush_position = Some(p);
            cx.notify();
        }
        if let Some(gesture) = self.gesture {
            match gesture {
                Gesture::Select(start) => {
                    self.region = normalized_selection(
                        start.0 as f32,
                        start.1 as f32,
                        p.0 as f32,
                        p.1 as f32,
                        self.source.width(),
                        self.source.height(),
                    )
                }
                Gesture::Move(region, start) => {
                    self.region = Some(Region {
                        x: (region.x as i64 + p.0 as i64 - start.0 as i64)
                            .clamp(0, (self.source.width() - region.width) as i64)
                            as u32,
                        y: (region.y as i64 + p.1 as i64 - start.1 as i64)
                            .clamp(0, (self.source.height() - region.height) as i64)
                            as u32,
                        ..region
                    });
                }
                Gesture::Resize(region, corner) => {
                    let anchor = corners(region)[(3 - corner) as usize];
                    self.region = normalized_selection(
                        anchor.0 as f32,
                        anchor.1 as f32,
                        p.0 as f32,
                        p.1 as f32,
                        self.source.width(),
                        self.source.height(),
                    );
                }
                Gesture::Draw => {
                    if let Some(base) = &self.base {
                        self.annotations.update(
                            self.local_point(p),
                            event.modifiers.shift,
                            (base.width(), base.height()),
                        );
                    }
                    if let Err(error) = self.refresh() {
                        self.error(error);
                    }
                }
            }
            if !matches!(gesture, Gesture::Draw) {
                self.preview = None;
            }
            cx.notify();
        } else if self.region.is_none()
            && let Some(targets) = &self.desktop
        {
            self.hover = targets
                .target_at(
                    self.source.bounds.left + p.0 as i32,
                    self.source.bounds.top + p.1 as i32,
                )
                .map(|b| Region {
                    x: (b.left - self.source.bounds.left) as u32,
                    y: (b.top - self.source.bounds.top) as u32,
                    width: b.width as u32,
                    height: b.height as u32,
                });
            cx.notify();
        }
        let _ = window;
    }
    fn mouse_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.font_dialog_open {
            return;
        }
        if let Some(gesture) = self.gesture.take() {
            if matches!(gesture, Gesture::Draw) {
                self.annotations.finish();
            } else {
                if self.region.is_none() {
                    self.region = self.hover;
                }
                if let Some(region) = self.region {
                    if Some(region) == self.region_before {
                        if let Err(error) = self.refresh() {
                            self.error(error);
                        }
                        cx.notify();
                        return;
                    }
                    match self
                        .source
                        .crop(region.x, region.y, region.width, region.height)
                    {
                        Ok(image) => {
                            self.base = Some(image);
                            self.annotations.clear();
                            if let Err(e) =
                                self.refresh().and_then(|_| self.ensure_toolbar(window, cx))
                            {
                                self.error(e);
                            }
                        }
                        Err(error) => self.error(error),
                    }
                }
            }
            cx.notify();
        }
    }
    fn commit_text(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<()> {
        if let Some(position) = self.text_position.take() {
            let text = self.text.read(cx).value().to_string();
            self.annotations.add_text(
                position,
                &text,
                self.style,
                self.text_size,
                self.text_font.clone(),
            );
            self.refresh()?;
            self.focus.focus(window, cx);
        }
        Ok(())
    }
    fn ensure_toolbar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<()> {
        self.toolbar_visible = true;
        if self.toolbar.is_none() {
            let editor = cx.entity();
            let snapshot = self.toolbar_snapshot();
            let sender = self.sender.clone();
            let (handle, toolbar) = gpui_kit::open_window(
                WindowOptions {
                    titlebar: None,
                    kind: WindowKind::PopUp,
                    focus: false,
                    is_resizable: false,
                    is_minimizable: false,
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(TOOLBAR_WIDTH as f32), px(TOOLBAR_MAIN_HEIGHT as f32)),
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    window.set_window_title("ShiTu · Toolbar");
                    window.on_window_should_close(cx, |_, _| false);
                    cx.new(|cx| {
                        let kind = snapshot.number_kind();
                        let (min, max) = kind.limits();
                        let number_input = cx.new(|cx| {
                            InputState::new(window, cx)
                                .default_value(snapshot.number_value().to_string())
                                .min(min as f64)
                                .max(max as f64)
                                .step(kind.step() as f64)
                        });
                        let number_subscription = cx.subscribe_in(
                            &number_input,
                            window,
                            |toolbar: &mut Toolbar, input, event: &InputEvent, window, cx| {
                                let kind = toolbar.snapshot.number_kind();
                                let value = input.read(cx).value().parse::<i32>();
                                let commit = matches!(
                                    event,
                                    InputEvent::Blur | InputEvent::PressEnter { .. }
                                );
                                if matches!(event, InputEvent::Change)
                                    && let Ok(value) = value
                                    && value == kind.normalize(value)
                                {
                                    let _ = toolbar.sender.send(Command::Number(kind, value));
                                } else if commit {
                                    let value = kind.normalize(
                                        value.unwrap_or(toolbar.snapshot.number_value()),
                                    );
                                    input.update(cx, |input, cx| {
                                        input.set_value(value.to_string(), window, cx)
                                    });
                                    let _ = toolbar.sender.send(Command::Number(kind, value));
                                }
                            },
                        );
                        let subscription = cx.observe_in(
                            &editor,
                            window,
                            |toolbar: &mut Toolbar, editor, window, cx| {
                                let snapshot = editor.read(cx).toolbar_snapshot();
                                let number_changed = toolbar.snapshot.number_kind()
                                    != snapshot.number_kind()
                                    || toolbar.snapshot.number_value() != snapshot.number_value();
                                toolbar.snapshot = snapshot;
                                if number_changed {
                                    toolbar.number_input.update(cx, |input, cx| {
                                        let kind = toolbar.snapshot.number_kind();
                                        let (min, max) = kind.limits();
                                        input.set_min(Some(min as f64), window, cx);
                                        input.set_max(Some(max as f64), window, cx);
                                        input.set_step(
                                            Some(NumberStep::from(kind.step() as f64)),
                                            window,
                                            cx,
                                        );
                                        input.set_value(
                                            toolbar.snapshot.number_value().to_string(),
                                            window,
                                            cx,
                                        )
                                    });
                                }
                                cx.notify();
                            },
                        );
                        Toolbar {
                            snapshot,
                            sender,
                            number_input,
                            focus: cx.focus_handle(),
                            tips: Rc::new(RefCell::new(None)),
                            _subscriptions: vec![subscription, number_subscription],
                        }
                    })
                },
            )?;
            let setup = handle
                .update(cx, |_, toolbar_window, cx| {
                    native::hide(toolbar_window)?;
                    native::prepare_image_window(toolbar_window, false)?;
                    native::set_owner(toolbar_window, window)?;
                    let tips = NativeTooltips::new(native::hwnd(toolbar_window)?)?;
                    toolbar.update(cx, |toolbar, cx| {
                        *toolbar.tips.borrow_mut() = Some(tips);
                        cx.notify();
                    });
                    Ok::<_, anyhow::Error>(())
                })
                .and_then(|r| r);
            if let Err(error) = setup {
                let _ = handle.update(cx, |_, w, _| w.remove_window());
                return Err(error);
            }
            self.toolbar = Some(handle);
        }
        self.position_toolbar(window, cx)?;
        if let Some(handle) = self.toolbar {
            handle.update(cx, |_, w, _| {
                native::set_always_on_top(w, self.desktop.is_some() || self.top)?;
                native::show_without_activation(w)
            })??;
        }
        self.focus.focus(window, cx);
        Ok(())
    }
    fn toolbar_snapshot(&self) -> ToolbarSnapshot {
        ToolbarSnapshot {
            tool: self.tool,
            busy: self.busy || self.font_dialog_open,
            style: self.style,
            text_size: self.text_size,
            text_font: self.text_font.clone(),
            capture: self.desktop.is_some(),
            properties_above: self.properties_above,
            can_undo: self.annotations.can_undo(),
            can_redo: self.annotations.can_redo(),
        }
    }
    fn position_toolbar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Result<()> {
        if !self.toolbar_visible {
            return Ok(());
        }
        let Some(toolbar) = self.toolbar else {
            return Ok(());
        };
        let host = native::bounds(window)?;
        let region = if self.desktop.is_some() {
            let Some(r) = self.region else {
                return Ok(());
            };
            RECT {
                left: self.source.bounds.left + r.x as i32,
                top: self.source.bounds.top + r.y as i32,
                right: self.source.bounds.left + (r.x + r.width) as i32,
                bottom: self.source.bounds.top + (r.y + r.height) as i32,
            }
        } else {
            host
        };
        let work = native::work_area_for_rect(region)?;
        let selected = rect(region);
        if !self.layout.is_initialized() {
            let cursor = native::cursor_position()?;
            self.layout.initialize(selected, cursor.0 as f64)?;
        }
        toolbar.update(cx, |_, toolbar_window, _| -> Result<()> {
            let scale = toolbar_window.scale_factor() as f64;
            let width = (TOOLBAR_WIDTH * scale).min((work.right - work.left - 20) as f64);
            let height = (TOOLBAR_MAIN_HEIGHT
                + property_height(self.tool, self.text_size as f32 / scale as f32))
                * scale;
            let placement = self.layout.place(
                selected,
                rect(work),
                Metrics {
                    width,
                    height,
                    main_height: TOOLBAR_MAIN_HEIGHT * scale,
                    expanded_height: height.max((TOOLBAR_MAIN_HEIGHT + 64.) * scale),
                    popup_padding: 0.,
                },
            )?;
            self.properties_above = placement.properties_above;
            native::place(
                toolbar_window,
                RECT {
                    left: placement.x.round() as i32,
                    top: placement.y.round() as i32,
                    right: (placement.x + width).round() as i32,
                    bottom: (placement.y + height).round() as i32,
                },
            )
        })??;
        Ok(())
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.font_dialog_open || !self.focus.is_focused(window) {
            return;
        }
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        let command = match key {
            "escape" => {
                if let Some(gesture) = self.gesture.take() {
                    if !matches!(gesture, Gesture::Draw) {
                        self.region = self.region_before;
                    }
                    self.annotations.cancel_edit();
                    if let Err(e) = self.refresh() {
                        self.error(e);
                    }
                    cx.notify();
                    return;
                }
                Command::Close
            }
            "enter" => Command::Copy,
            "c" if modifiers.control => Command::Copy,
            "s" if modifiers.control => Command::Save,
            "z" if modifiers.control && modifiers.shift => Command::Redo,
            "z" if modifiers.control => Command::Undo,
            "y" if modifiers.control => Command::Redo,
            "delete" | "backspace" => {
                self.annotations.delete_selected();
                if let Err(e) = self.refresh() {
                    self.error(e);
                }
                cx.notify();
                return;
            }
            _ => return,
        };
        if let Err(error) = self.command(command, window, cx) {
            self.error(error);
        }
        cx.stop_propagation();
        cx.notify();
    }
    fn menu(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.font_dialog_open {
            return;
        }
        // The pin canvas is a native drag area when no annotation tool is active.
        // Consume its right-click so Windows doesn't also open a caption menu.
        cx.stop_propagation();
        self.focus.focus(window, cx);
        if self.desktop.is_some() {
            if let Err(e) = self.command(Command::Close, window, cx) {
                self.error(e);
            }
            return;
        }
        NativeMenu::new()
            .menu(i18n::text("复制图像"), Box::new(CopyImage))
            .menu(i18n::text("图像另存为..."), Box::new(SaveImage))
            .menu(i18n::text("OCR 识别"), Box::new(Recognize))
            .separator()
            .menu_with_check(
                i18n::text("显示工具栏"),
                self.toolbar_visible,
                Box::new(ToggleToolbar),
            )
            .menu_with_check(i18n::text("始终置顶"), self.top, Box::new(ToggleTop))
            .menu_with_check(i18n::text("窗口阴影"), self.shadow, Box::new(ToggleShadow))
            .submenu(
                i18n::text("不透明度"),
                NativeMenu::new()
                    .menu_with_check("25%", self.opacity == 25, Box::new(Opacity25))
                    .menu_with_check("50%", self.opacity == 50, Box::new(Opacity50))
                    .menu_with_check("75%", self.opacity == 75, Box::new(Opacity75))
                    .menu_with_check("90%", self.opacity == 90, Box::new(Opacity90))
                    .menu_with_check("100%", self.opacity == 100, Box::new(Opacity100)),
            )
            .submenu(
                i18n::text("缩放（{}%）").replace("{}", &self.scale.to_string()),
                NativeMenu::new()
                    .menu_with_check("25%", self.scale == 25, Box::new(Scale25))
                    .menu_with_check("50%", self.scale == 50, Box::new(Scale50))
                    .menu_with_check("75%", self.scale == 75, Box::new(Scale75))
                    .menu_with_check("100%", self.scale == 100, Box::new(OriginalSize))
                    .menu_with_check("125%", self.scale == 125, Box::new(Scale125))
                    .menu_with_check("150%", self.scale == 150, Box::new(Scale150))
                    .menu_with_check("200%", self.scale == 200, Box::new(Scale200)),
            )
            .menu(i18n::text("恢复原始大小"), Box::new(OriginalSize))
            .menu(i18n::text("适合屏幕"), Box::new(FitScreen))
            .submenu(
                i18n::text("图像处理"),
                NativeMenu::new()
                    .menu(i18n::text("向左旋转 90°"), Box::new(RotateLeft))
                    .menu(i18n::text("向右旋转 90°"), Box::new(RotateRight))
                    .menu(i18n::text("水平翻转"), Box::new(FlipHorizontal))
                    .menu(i18n::text("垂直翻转"), Box::new(FlipVertical)),
            )
            .submenu(
                i18n::text("替换图像"),
                NativeMenu::new()
                    .menu(i18n::text("从剪贴板"), Box::new(ReplaceClipboard))
                    .menu(i18n::text("从文件..."), Box::new(ReplaceFile)),
            )
            .menu_with_disabled(
                i18n::text("在文件夹中显示"),
                self.source_path.is_none(),
                Box::new(RevealFile),
            )
            .separator()
            .menu(i18n::text("关闭钉住"), Box::new(Close))
            .show(event.position, window, cx);
    }
}

fn rect(r: RECT) -> Rect {
    Rect {
        left: r.left as f64,
        top: r.top as f64,
        right: r.right as f64,
        bottom: r.bottom as f64,
    }
}
fn fitted_size(width: u32, height: u32, available_width: u32, available_height: u32) -> (u32, u32) {
    let scale = (available_width as f64 / width as f64)
        .min(available_height as f64 / height as f64)
        .min(1.);
    (
        (width as f64 * scale).round().max(1.) as u32,
        (height as f64 * scale).round().max(1.) as u32,
    )
}
fn corners(r: Region) -> [(u32, u32); 4] {
    [
        (r.x, r.y),
        (r.x + r.width, r.y),
        (r.x, r.y + r.height),
        (r.x + r.width, r.y + r.height),
    ]
}
fn corner_at(r: Region, p: (u32, u32), tolerance: u32) -> Option<i32> {
    corners(r)
        .iter()
        .position(|c| c.0.abs_diff(p.0) <= tolerance && c.1.abs_diff(p.1) <= tolerance)
        .map(|i| i as i32)
}
fn normalized_selection(
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    width: u32,
    height: u32,
) -> Option<Region> {
    let x = x1.min(x2).clamp(0., width as f32) as u32;
    let y = y1.min(y2).clamp(0., height as f32) as u32;
    let right = x1.max(x2).clamp(0., width as f32) as u32;
    let bottom = y1.max(y2).clamp(0., height as f32) as u32;
    (right > x && bottom > y).then_some(Region {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

fn brush_bounds(
    image: Bounds<Pixels>,
    source: (u32, u32),
    point: (u32, u32),
    radius: i32,
) -> Bounds<Pixels> {
    let sx = f32::from(image.size.width) / source.0 as f32;
    let sy = f32::from(image.size.height) / source.1 as f32;
    Bounds::new(
        image.origin
            + gpui_kit::point(
                px((point.0 as f32 - radius as f32) * sx),
                px((point.1 as f32 - radius as f32) * sy),
            ),
        size(px(radius as f32 * 2. * sx), px(radius as f32 * 2. * sy)),
    )
}

fn image_display_bounds(
    bounds: Bounds<Pixels>,
    source: (u32, u32),
    scale: f32,
    capture: bool,
) -> Bounds<Pixels> {
    if capture {
        // Desktop pixels must never be stretched to fit a client area. GPUI
        // multiplies logical coordinates by the window scale when painting.
        Bounds::new(
            bounds.origin,
            size(px(source.0 as f32 / scale), px(source.1 as f32 / scale)),
        )
    } else {
        bounds
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = self.background.clone();
        let preview = self.preview.clone();
        let region = self.region.or(self.hover);
        let capture = self.desktop.is_some();
        let source_size = (self.source.width(), self.source.height());
        let selected = self.annotations.selection_bounds();
        let brush = if matches!(self.tool, 5 | 6) && !self.busy && self.base.is_some() {
            self.brush_position
                .filter(|p| self.region.is_none_or(|r| r.contains(*p)))
                .map(|p| (p, self.style.radius))
        } else {
            None
        };
        let region_origin = self.region.map(|r| (r.x, r.y)).unwrap_or((0, 0));
        let measured = self.canvas_bounds.clone();
        let canvas = canvas(
            move |bounds, window, _| {
                if capture && measured.get() != bounds {
                    logging::info(format!(
                        "capture canvas: {:?}; scale={}; physical={}x{}; source={}x{}",
                        bounds,
                        window.scale_factor(),
                        f32::from(bounds.size.width) * window.scale_factor(),
                        f32::from(bounds.size.height) * window.scale_factor(),
                        source_size.0,
                        source_size.1
                    ));
                }
                measured.set(bounds);
            },
            move |bounds, _, window, _| {
                let image_bounds =
                    image_display_bounds(bounds, source_size, window.scale_factor(), capture);
                let sx = f32::from(image_bounds.size.width) / source_size.0 as f32;
                let sy = f32::from(image_bounds.size.height) / source_size.1 as f32;
                let region_bounds = |r: Region| {
                    Bounds::new(
                        bounds.origin + point(px(r.x as f32 * sx), px(r.y as f32 * sy)),
                        size(px(r.width as f32 * sx), px(r.height as f32 * sy)),
                    )
                };
                if capture
                    && let Err(error) = window.paint_image(
                        bounds,
                        image_bounds,
                        Corners::default(),
                        background.clone(),
                        0,
                        false,
                    )
                {
                    logging::error(error.to_string());
                }
                if capture {
                    if let Some(r) = region {
                        let rb = region_bounds(r);
                        for shade in [
                            Bounds::new(
                                bounds.origin,
                                size(bounds.size.width, rb.origin.y - bounds.origin.y),
                            ),
                            Bounds::new(
                                point(bounds.origin.x, rb.bottom()),
                                size(bounds.size.width, bounds.bottom() - rb.bottom()),
                            ),
                            Bounds::new(
                                point(bounds.origin.x, rb.origin.y),
                                size(rb.origin.x - bounds.origin.x, rb.size.height),
                            ),
                            Bounds::new(
                                point(rb.right(), rb.origin.y),
                                size(bounds.right() - rb.right(), rb.size.height),
                            ),
                        ] {
                            window.paint_quad(fill(shade, rgba(0x00000066)));
                        }
                        if let Some(preview) = &preview
                            && let Err(error) = window.paint_image(
                                rb,
                                rb,
                                Corners::default(),
                                preview.clone(),
                                0,
                                false,
                            )
                        {
                            logging::error(error.to_string());
                        }
                        window.paint_quad(outline(rb, rgb(0x4a90e2), BorderStyle::Solid));
                        for p in corners(r) {
                            window.paint_quad(fill(
                                Bounds::new(
                                    bounds.origin
                                        + point(px(p.0 as f32 * sx - 3.), px(p.1 as f32 * sy - 3.)),
                                    size(px(6.), px(6.)),
                                ),
                                rgb(0xffffff),
                            ));
                        }
                    } else {
                        window.paint_quad(fill(bounds, rgba(0x00000055)));
                    }
                } else if let Some(preview) = &preview
                    && let Err(error) = window.paint_image(
                        bounds,
                        bounds,
                        Corners::default(),
                        preview.clone(),
                        0,
                        false,
                    )
                {
                    logging::error(error.to_string());
                }
                if !capture {
                    // UI-only outline: never add this border to exported pixels.
                    window.paint_quad(outline(bounds, rgb(0x54a5ff), BorderStyle::Solid));
                }
                if let Some(selection) = selected {
                    let r = Region {
                        x: region_origin.0 + selection.left,
                        y: region_origin.1 + selection.top,
                        width: selection.width,
                        height: selection.height,
                    };
                    window.paint_quad(outline(region_bounds(r), rgb(0x4a90e2), BorderStyle::Solid));
                    for p in corners(r) {
                        window.paint_quad(fill(
                            Bounds::new(
                                bounds.origin
                                    + point(px(p.0 as f32 * sx - 3.), px(p.1 as f32 * sy - 3.)),
                                size(px(6.), px(6.)),
                            ),
                            rgb(0x4a90e2),
                        ));
                    }
                }
                if let Some((p, radius)) = brush {
                    let ring = brush_bounds(image_bounds, source_size, p, radius);
                    let clip = region.map(region_bounds).unwrap_or(image_bounds);
                    window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                        window.paint_quad(
                            outline(ring, rgb(0x000000), BorderStyle::Solid)
                                .corner_radii(ring.size.width.min(ring.size.height) / 2.)
                                .border_widths(px(2.)),
                        );
                        window.paint_quad(
                            outline(ring, rgb(0xffffff), BorderStyle::Solid)
                                .corner_radii(ring.size.width.min(ring.size.height) / 2.)
                                .border_widths(px(1.)),
                        );
                    });
                }
            },
        )
        .size_full();
        let mut root = div()
            .id("image-editor")
            .size_full()
            .relative()
            .when(matches!(self.tool, 5 | 6), |root| {
                root.cursor(CursorStyle::Crosshair)
            })
            .on_hover(cx.listener(|this, hovered, _, cx| {
                if !hovered && this.brush_position.take().is_some() {
                    cx.notify();
                }
            }))
            // start_window_move() is a no-op in gpui-pre-windows 0.3.8.
            // The official TitleBar uses this hit-test area, which the Windows
            // backend maps to HTCAPTION and native window dragging instead.
            .when(!capture && self.tool == 0 && !self.busy, |root| {
                root.window_control_area(WindowControlArea::Drag)
                    .cursor(CursorStyle::OpenHand)
            })
            .track_focus(&self.focus)
            .key_context("ShiTuEditor")
            .child(
                div()
                    .size_full()
                    .opacity(self.opacity as f32 / 100.)
                    .child(canvas),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event, window, cx| this.mouse_down(event, window, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event, window, cx| this.menu(event, window, cx)),
            )
            .on_mouse_move(
                cx.listener(|this, event, window, cx| this.mouse_move(event, window, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.mouse_up(window, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.mouse_up(window, cx)),
            )
            .on_key_down(cx.listener(|this, event, window, cx| this.key(event, window, cx)))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                if this.desktop.is_none() && this.shared.borrow().config.pin.wheel_zoom {
                    let direction = if event.delta.pixel_delta(px(16.)).y > px(0.) {
                        1
                    } else {
                        -1
                    };
                    let step = this.shared.borrow().config.pin.zoom_step as i32;
                    if let Err(error) = this.set_scale(this.scale + direction * step, window, cx) {
                        this.error(error);
                    }
                    cx.notify();
                }
            }));
        macro_rules! action {
            ($ty:ty,$command:expr) => {
                root = root.on_action(cx.listener(|this, _: &$ty, window, cx| {
                    if let Err(error) = this.command($command, window, cx) {
                        this.error(error);
                    }
                    cx.notify();
                }));
            };
        }
        action!(CopyImage, Command::Copy);
        action!(SaveImage, Command::Save);
        action!(Recognize, Command::Ocr);
        action!(Close, Command::Close);
        action!(ToggleToolbar, Command::ToggleToolbar);
        action!(ToggleTop, Command::ToggleTop);
        action!(ToggleShadow, Command::ToggleShadow);
        action!(OriginalSize, Command::Scale(100));
        action!(FitScreen, Command::Fit);
        action!(RotateLeft, Command::Transform(0));
        action!(RotateRight, Command::Transform(1));
        action!(FlipHorizontal, Command::Transform(2));
        action!(FlipVertical, Command::Transform(3));
        action!(ReplaceClipboard, Command::ReplaceClipboard);
        action!(ReplaceFile, Command::ReplaceFile);
        action!(RevealFile, Command::RevealFile);
        action!(Opacity25, Command::Opacity(25));
        action!(Opacity50, Command::Opacity(50));
        action!(Opacity75, Command::Opacity(75));
        action!(Opacity90, Command::Opacity(90));
        action!(Opacity100, Command::Opacity(100));
        action!(Scale25, Command::Scale(25));
        action!(Scale50, Command::Scale(50));
        action!(Scale75, Command::Scale(75));
        action!(Scale125, Command::Scale(125));
        action!(Scale150, Command::Scale(150));
        action!(Scale200, Command::Scale(200));
        if capture && let Some(region) = self.region {
            let bounds = self.canvas_bounds.get();
            let scale = f32::from(bounds.size.width) / source_size.0 as f32;
            let label = if self.busy {
                i18n::text("正在识别文字...").to_owned()
            } else if !self.status.is_empty() {
                self.status.clone()
            } else {
                format!("{} × {}", region.width, region.height)
            };
            root = root.child(
                div()
                    .absolute()
                    .left(px((region.x as f32 * scale)
                        .min((f32::from(bounds.size.width) - 118.).max(0.))))
                    .top(px((region.y as f32 * scale - 26.).max(0.)))
                    .min_w(px(118.))
                    .h(px(22.))
                    .px_2()
                    .rounded(px(4.))
                    .bg(rgba(0x1f2329e8))
                    .text_color(rgb(0xffffff))
                    .text_xs()
                    .h_flex()
                    .justify_center()
                    .child(label),
            );
        }
        if let Some(p) = self.text_position {
            let b = self.canvas_bounds.get();
            let sx = f32::from(b.size.width) / source_size.0 as f32;
            let sy = f32::from(b.size.height) / source_size.1 as f32;
            let available_width = self
                .base
                .as_ref()
                .map_or(source_size.0, |image| image.width())
                .saturating_sub(p.0) as f32
                * sx;
            root = root.child(
                div()
                    .absolute()
                    .left(px((p.0 + region_origin.0) as f32 * sx))
                    .top(px((p.1 + region_origin.1) as f32 * sy))
                    .w(px((self.text_size as f32 * sx * 8.)
                        .max(240.)
                        .min(available_width.max(1.))))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        super::text_input(&self.text)
                            .font_family(self.text_font.family.clone())
                            .font_weight(FontWeight(self.text_font.weight as f32))
                            .when(self.text_font.italic, |input| input.italic())
                            .text_size(px(self.text_size as f32 * sy))
                            .line_height(relative(1.3))
                            .h(px(self.text_size as f32 * sy * 1.5 + 4.))
                            .px_0()
                            .py_0()
                            .text_color(rgb((self.style.rgba[0] as u32) << 16
                                | (self.style.rgba[1] as u32) << 8
                                | self.style.rgba[2] as u32)),
                    ),
            );
        }
        root
    }
}

struct ToolbarSnapshot {
    tool: i32,
    busy: bool,
    style: DrawStyle,
    text_size: u32,
    text_font: TextFont,
    capture: bool,
    properties_above: bool,
    can_undo: bool,
    can_redo: bool,
}
impl ToolbarSnapshot {
    fn number_kind(&self) -> NumberKind {
        if self.tool == 4 {
            NumberKind::Font
        } else {
            NumberKind::Width(self.tool)
        }
    }
    fn number_value(&self) -> i32 {
        if self.tool == 4 {
            self.text_size as i32
        } else {
            self.style.radius * 2
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum NumberKind {
    Font,
    Width(i32),
}

fn number_wheel_steps(event: &ScrollWheelEvent) -> i32 {
    let delta = event.delta.pixel_delta(px(16.));
    // gpui-pre-windows 0.3.8 routes Shift+WM_MOUSEWHEEL to the X axis.
    let delta = if event.modifiers.shift && delta.y == px(0.) {
        delta.x
    } else {
        delta.y
    };
    let direction = if delta > px(0.) {
        1
    } else if delta < px(0.) {
        -1
    } else {
        0
    };
    direction * if event.modifiers.shift { 5 } else { 1 }
}
impl NumberKind {
    fn limits(self) -> (i32, i32) {
        match self {
            Self::Font => (8, 96),
            Self::Width(5 | 6) => (4, 128),
            Self::Width(_) => (2, 24),
        }
    }
    fn step(self) -> i32 {
        if self == Self::Font { 1 } else { 2 }
    }
    fn normalize(self, value: i32) -> i32 {
        let (min, max) = self.limits();
        // Image-space brushes use whole-pixel radii, so diameters step by 2px.
        let value = value.clamp(min, max);
        ((value + self.step() / 2) / self.step() * self.step()).min(max)
    }
}

#[derive(Clone, PartialEq)]
enum ToolbarOption {
    Number(NumberKind, i32),
}
#[derive(Clone, PartialEq, Action)]
#[action(no_json)]
struct SetToolbarOption {
    value: ToolbarOption,
}

// Collect the real button rectangles during prepaint. The native control gets
// the complete set after layout, including clipping at narrow screen edges.
fn tooltip_button(
    button: Button,
    label: &'static str,
    regions: &Rc<RefCell<Vec<TipRegion>>>,
    dimensions: (f32, f32),
) -> impl IntoElement {
    let regions = regions.clone();
    div()
        .relative()
        .flex_shrink_0()
        .w(px(dimensions.0))
        .h(px(dimensions.1))
        .child(button.accessibility_label(label))
        .child(
            canvas(
                move |bounds, window, _| {
                    let scale = window.scale_factor();
                    let viewport = window.viewport_size();
                    let rect = RECT {
                        left: (f32::from(bounds.left()).max(0.) * scale).round() as i32,
                        top: (f32::from(bounds.top()).max(0.) * scale).round() as i32,
                        right: (f32::from(bounds.right()).min(viewport.width.into()) * scale)
                            .round() as i32,
                        bottom: (f32::from(bounds.bottom()).min(viewport.height.into()) * scale)
                            .round() as i32,
                    };
                    if rect.right > rect.left && rect.bottom > rect.top {
                        let mut regions = regions.borrow_mut();
                        let id = regions.len() + 1;
                        regions.push(TipRegion {
                            id,
                            text: label,
                            rect,
                        });
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
}

struct Toolbar {
    snapshot: ToolbarSnapshot,
    sender: mpsc::Sender<Command>,
    number_input: Entity<InputState>,
    focus: FocusHandle,
    tips: Rc<RefCell<Option<NativeTooltips>>>,
    _subscriptions: Vec<Subscription>,
}
impl Toolbar {
    // NumberInput 0.7.1 owns editing/stepping/limits, but has no wheel handler.
    // Keep wheel routing scoped to the numeric control using GPUI hit testing.
    fn wheel_control(&self, id: &'static str, content: impl IntoElement) -> impl IntoElement {
        let sender = self.sender.clone();
        let kind = self.snapshot.number_kind();
        let busy = self.snapshot.busy;
        div()
            .id(id)
            .child(content)
            .on_scroll_wheel(move |event, _, cx| {
                let steps = number_wheel_steps(event);
                if !busy && steps != 0 {
                    let _ = sender.send(Command::AdjustNumber(kind, steps));
                    cx.stop_propagation();
                }
            })
    }
    fn menu_button(
        &self,
        id: &'static str,
        label: impl Into<SharedString>,
        options: Vec<(String, bool, ToolbarOption)>,
    ) -> Button {
        let focus = self.focus.clone();
        let tips = self.tips.clone();
        Button::new(id)
            .dropdown_caret(true)
            .label(label)
            .w(px(188.))
            .h(px(32.))
            .disabled(self.snapshot.busy)
            .on_click(move |event, window, cx| {
                if let Some(tips) = tips.borrow().as_ref() {
                    tips.dismiss();
                }
                focus.focus(window, cx);
                let mut menu = NativeMenu::new();
                for (label, checked, value) in &options {
                    menu = menu.menu_with_check(
                        label.clone(),
                        *checked,
                        Box::new(SetToolbarOption {
                            value: value.clone(),
                        }),
                    );
                }
                menu.show(event.position(), window, cx);
            })
    }
    fn button(
        &self,
        id: &'static str,
        label: &'static str,
        bytes: &'static [u8],
        command: Command,
        selected: bool,
        busy: bool,
    ) -> Button {
        let sender = self.sender.clone();
        Button::new(id)
            .icon(Icon::default().data(bytes).size(px(16.)))
            .accessibility_label(label)
            .ghost()
            .selected(selected)
            .disabled(busy)
            .small()
            .w(px(28.))
            .h(px(28.))
            .rounded(px(3.))
            .on_click(move |_, _, _| {
                let _ = sender.send(command.clone());
            })
    }
}
impl Render for Toolbar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tip_regions = Rc::new(RefCell::new(Vec::new()));
        // Native window creation/placement can paint synchronously while the
        // editor is borrowed. Render a snapshot instead of reentering it.
        let editor = &self.snapshot;
        let tool = editor.tool;
        let busy = editor.busy;
        let sample_size = editor.text_size as f32 / window.scale_factor();
        let properties_height = property_height(tool, sample_size);
        // Preserve the original screenshot palette while native components own
        // button input, keyboard focus, tooltip and dropdown behavior.
        let dark = cx.theme().is_dark();
        let card = rgb(if dark { 0x252a34 } else { 0xffffff });
        let border = rgb(if dark { 0x414957 } else { 0xdde3eb });
        let accent = rgb(if dark { 0x43d4c3 } else { 0x006bc7 });
        let selected_color = rgb(if dark { 0x244b4d } else { 0xe4f0fc });
        let separator = || div().flex_shrink_0().w(px(1.)).h(px(20.)).bg(border);
        let mut buttons = div()
            .h_flex()
            .gap(px(2.))
            .h(px(36.))
            .px(px(3.))
            .flex_shrink_0();
        macro_rules! tool_button {
            ($id:literal,$label:expr,$icon:literal,$command:expr,$selected:expr) => {
                buttons = buttons.child(tooltip_button(
                    self.button(
                        $id,
                        $label,
                        include_bytes!(concat!("../../assets/icons/", $icon, ".svg")),
                        $command,
                        $selected,
                        busy || ($id == "undo" && !editor.can_undo)
                            || ($id == "redo" && !editor.can_redo),
                    )
                    .when($selected, |button| {
                        button
                            .bg(selected_color)
                            .text_color(accent)
                            .border_1()
                            .border_color(accent)
                    }),
                    $label,
                    &tip_regions,
                    (28., 28.),
                ));
            };
        }
        tool_button!(
            "undo",
            i18n::text("撤销上一步操作"),
            "undo-2",
            Command::Undo,
            false
        );
        tool_button!(
            "redo",
            i18n::text("重做上一步操作"),
            "redo-2",
            Command::Redo,
            false
        );
        buttons = buttons.child(separator());
        tool_button!(
            "pointer",
            i18n::text("选择 / 编辑标注"),
            "mouse-pointer",
            Command::Tool(8),
            tool == 8
        );
        tool_button!(
            "pen",
            i18n::text("画笔标注"),
            "pencil",
            Command::Tool(1),
            tool == 1
        );
        tool_button!(
            "rect",
            i18n::text("绘制矩形"),
            "square",
            Command::Tool(2),
            tool == 2
        );
        tool_button!(
            "ellipse",
            i18n::text("绘制圆形 / 椭圆"),
            "circle",
            Command::Tool(7),
            tool == 7
        );
        tool_button!(
            "arrow",
            i18n::text("绘制箭头"),
            "arrow-up-right",
            Command::Tool(3),
            tool == 3
        );
        tool_button!(
            "text",
            i18n::text("添加文字"),
            "letter-t",
            Command::Tool(4),
            tool == 4
        );
        tool_button!(
            "erase",
            i18n::text("擦除标注"),
            "eraser",
            Command::Tool(5),
            tool == 5
        );
        tool_button!(
            "mosaic",
            i18n::text("添加马赛克"),
            "mosaic",
            Command::Tool(6),
            tool == 6
        );
        buttons = buttons.child(separator());
        tool_button!(
            "ocr",
            i18n::text("识别图像文字"),
            "scan-text",
            Command::Ocr,
            false
        );
        if editor.capture {
            tool_button!(
                "pin",
                i18n::text("将截图钉在屏幕上"),
                "pin",
                Command::Pin,
                false
            );
        }
        tool_button!("close", i18n::text("关闭"), "x", Command::Close, false);
        tool_button!(
            "save",
            i18n::text("保存截图到文件"),
            "save",
            Command::Save,
            false
        );
        tool_button!(
            "copy",
            i18n::text("复制截图到剪贴板"),
            "copy",
            Command::Copy,
            false
        );
        let color_names = [
            i18n::text("红色"),
            i18n::text("蓝色"),
            i18n::text("绿色"),
            i18n::text("黄色"),
            i18n::text("橙色"),
            i18n::text("紫色"),
            i18n::text("粉色"),
            i18n::text("青色"),
            i18n::text("黑色"),
            i18n::text("白色"),
            i18n::text("灰色"),
            i18n::text("棕色"),
        ];
        let mut colors = div().v_flex().gap(px(2.)).w(px(154.)).flex_shrink_0();
        for row in 0..2 {
            let mut swatches = div().h_flex().gap(px(2.)).h(px(28.));
            for index in row * 6..row * 6 + 6 {
                let color = COLORS[index];
                let sender = self.sender.clone();
                let value = rgb((color[0] as u32) << 16 | (color[1] as u32) << 8 | color[2] as u32);
                swatches = swatches.child(tooltip_button(
                    Button::new(("color", index))
                        .ghost()
                        .small()
                        .w(px(24.))
                        .h(px(28.))
                        .p_0()
                        .rounded(px(3.))
                        .accessibility_label(color_names[index])
                        .selected(color == editor.style.rgba)
                        .disabled(busy)
                        .child(
                            div()
                                .size(px(16.))
                                .bg(value)
                                .border_1()
                                .border_color(cx.theme().border),
                        )
                        .on_click(move |_, _, _| {
                            let _ = sender.send(Command::Color(index));
                        }),
                    color_names[index],
                    &tip_regions,
                    (24., 28.),
                ));
            }
            colors = colors.child(swatches);
        }
        let brush = matches!(tool, 5 | 6);
        let dot = |diameter: f32| {
            div()
                .size(px(32.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(div().size(px(diameter)).rounded_full().bg(rgb(if dark {
                    0xf0f0f0
                } else {
                    0x111111
                })))
        };
        let mut controls = div().h_flex().gap(px(6.)).flex_shrink_0();
        if tool == 4 || brush {
            if brush {
                // Keep a fixed slot; the canvas ring shows the true pixel size.
                controls =
                    controls.child(dot(4. + (editor.number_value() as f32 - 4.) / 124. * 24.));
            }
            controls = controls.child(
                self.wheel_control(
                    "size-input-wheel",
                    NumberInput::new(&self.number_input)
                        .suffix("px")
                        .small()
                        .w(px(if tool == 4 { 148. } else { 188. }))
                        .h(px(32.))
                        .disabled(busy),
                ),
            );
            if tool == 4 {
                let sender = self.sender.clone();
                let fields = div()
                    .v_flex()
                    .gap(px(4.))
                    .w(px(148.))
                    .flex_shrink_0()
                    .child(controls)
                    .child(
                        Button::new("font-settings")
                            .small()
                            .h(px(28.))
                            .w_full()
                            .label(i18n::text("字体设置…"))
                            .disabled(busy)
                            .on_click(move |_, _, _| {
                                let _ = sender.send(Command::ChooseFont);
                            }),
                    );
                controls = div()
                    .h_flex()
                    .gap(px(6.))
                    .flex_shrink_0()
                    .child(fields)
                    .child(
                        div()
                            .w(px(104.))
                            .h(px((properties_height - 8.) as f32))
                            .flex_shrink_0()
                            .h_flex()
                            .justify_center()
                            .font_family(editor.text_font.family.clone())
                            .font_weight(FontWeight(editor.text_font.weight as f32))
                            .when(editor.text_font.italic, |sample| sample.italic())
                            .text_size(px(sample_size))
                            .line_height(relative(1.3))
                            .child("字"),
                    );
            }
        } else {
            let display = editor.number_value();
            let width = 150.;
            let width_row = div()
                .h_flex()
                .gap(px(6.))
                .child(
                    self.wheel_control(
                        "width-presets-wheel",
                        tooltip_button(
                            self.menu_button(
                                "stroke-width",
                                format!("{display} px"),
                                [2, 4, 6, 8, 12, 16, 24]
                                    .into_iter()
                                    .map(|diameter| {
                                        (
                                            format!("{diameter} px"),
                                            diameter == display,
                                            ToolbarOption::Number(
                                                NumberKind::Width(tool),
                                                diameter,
                                            ),
                                        )
                                    })
                                    .collect(),
                            )
                            .w(px(width)),
                            i18n::text("线宽"),
                            &tip_regions,
                            (width, 32.),
                        ),
                    ),
                )
                .child(dot(display as f32));
            let sender = self.sender.clone();
            controls = div()
                .v_flex()
                .gap(px(4.))
                .w(px(188.))
                .flex_shrink_0()
                .child(width_row)
                .child(
                    RadioGroup::horizontal("stroke-style")
                        .selected_index(Some(usize::from(editor.style.dashed)))
                        .disabled(busy)
                        .child(
                            Radio::new("solid")
                                .small()
                                .label("────")
                                .accessibility_label(i18n::text("实线")),
                        )
                        .child(
                            Radio::new("dashed")
                                .small()
                                .label("┄┄┄┄")
                                .accessibility_label(i18n::text("虚线")),
                        )
                        .on_change(move |index, _, _| {
                            let _ = sender.send(Command::Dashed(*index == 1));
                        }),
                );
        }
        let properties = div()
            .h_flex()
            .justify_center()
            .gap(px(8.))
            .px(px(3.))
            .h(px(properties_height as f32))
            .flex_shrink_0()
            .when(!brush, |row| {
                row.child(colors)
                    .child(div().w(px(1.)).h(px(50.)).bg(cx.theme().border))
            })
            .child(controls);
        let content = div().v_flex();
        let content = if editor.properties_above {
            content
                .when(properties_height > 0., |row| row.child(properties))
                .child(buttons)
        } else {
            content
                .child(buttons)
                .when(properties_height > 0., |row| row.child(properties))
        };
        let tips = self.tips.clone();
        let reset_regions = tip_regions.clone();
        div()
            .id("toolbar-scroll")
            .relative()
            .track_focus(&self.focus)
            .size_full()
            .overflow_x_scrollbar()
            .bg(card)
            .border_1()
            .border_color(border)
            .rounded(px(4.))
            .text_color(cx.theme().foreground)
            .text_xs()
            .child(
                canvas(
                    move |_, _, _| reset_regions.borrow_mut().clear(),
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(content)
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, _, _, _| {
                        if let Some(tips) = tips.borrow_mut().as_mut()
                            && let Err(error) = tips.sync(&tip_regions.borrow())
                        {
                            logging::error(format!("Update toolbar tooltips: {error:#}"));
                        }
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .on_action(cx.listener(|this, action: &SetToolbarOption, _, cx| {
                let command = match action.value {
                    ToolbarOption::Number(kind, value) => Command::Number(kind, value),
                };
                let _ = this.sender.send(command);
                cx.stop_propagation();
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Region, fitted_size, image_display_bounds, normalized_selection, render_image};
    use crate::image::CapturedImage;
    #[test]
    fn brush_outline_tracks_image_pixels_across_dpi_and_pin_zoom() {
        use super::brush_bounds;
        use gpui_kit::{Bounds, point, px, size};
        for dpi in [1., 1.25, 1.5, 2.] {
            for zoom in [0.5, 1., 2.] {
                let image = Bounds::new(
                    point(px(12.), px(8.)),
                    size(px(800. * zoom / dpi), px(600. * zoom / dpi)),
                );
                for radius in [2, 18, 64] {
                    let ring = brush_bounds(image, (800, 600), (200, 100), radius);
                    assert!(
                        (f32::from(ring.size.width) * dpi - radius as f32 * 2. * zoom).abs()
                            < 0.001
                    );
                    assert!(
                        (f32::from(ring.center().x - image.origin.x) * dpi - 200. * zoom).abs()
                            < 0.001
                    );
                }
            }
        }
    }
    #[test]
    fn number_wheel_handles_windows_shift_axis_and_limits() {
        use super::{NumberKind, number_wheel_steps};
        use gpui_kit::{ScrollDelta, ScrollWheelEvent, point};
        let mut event = ScrollWheelEvent {
            delta: ScrollDelta::Lines(point(0., 3.)),
            ..Default::default()
        };
        assert_eq!(number_wheel_steps(&event), 1);
        event.delta = ScrollDelta::Lines(point(0., -3.));
        assert_eq!(number_wheel_steps(&event), -1);
        event.delta = ScrollDelta::Lines(point(3., 0.));
        assert_eq!(number_wheel_steps(&event), 0);
        event.modifiers.shift = true;
        assert_eq!(number_wheel_steps(&event), 5);
        for kind in [
            NumberKind::Font,
            NumberKind::Width(1),
            NumberKind::Width(5),
            NumberKind::Width(6),
        ] {
            let (min, max) = kind.limits();
            assert_eq!(kind.normalize(min - kind.step()), min);
            assert_eq!(kind.normalize(max + kind.step()), max);
        }
        assert_eq!(NumberKind::Width(6).normalize(35), 36);
        assert_eq!(NumberKind::Width(5).limits(), (4, 128));
    }
    #[test]
    fn capture_preview_keeps_device_pixels_even_with_a_smaller_client_area() {
        use gpui_kit::{Bounds, point, px, size};
        for scale in [1., 1.25, 1.5, 2.] {
            let client = Bounds::new(
                point(px(0.), px(0.)),
                size(px(3824. / scale), px(1124. / scale)),
            );
            let capture = image_display_bounds(client, (3840, 1132), scale, true);
            assert!((f32::from(capture.size.width) * scale - 3840.).abs() < 0.001);
            assert!((f32::from(capture.size.height) * scale - 1132.).abs() < 0.001);
            assert_eq!(
                image_display_bounds(client, (3840, 1132), scale, false),
                client
            );
        }
    }
    #[test]
    fn selection_coordinates_support_reverse_drag_and_clamping() {
        assert_eq!(
            normalized_selection(300., 250., 100., 50., 1920, 1080),
            Some(Region {
                x: 100,
                y: 50,
                width: 200,
                height: 200
            })
        );
        assert_eq!(
            normalized_selection(-50., -25., 2000., 1200., 1920, 1080),
            Some(Region {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080
            })
        );
        assert_eq!(
            normalized_selection(100., 100., 100., 200., 1920, 1080),
            None
        );
    }
    #[test]
    fn render_upload_swizzles_red_and_blue_without_mutating_source() {
        let image = CapturedImage::from_rgba(0, 0, 1, 1, &[200, 10, 30, 255]).unwrap();
        let gpu = render_image(&image);
        assert_eq!(gpu.as_bytes(0).unwrap(), &[30, 10, 200, 255]);
        assert_eq!(image.rgba_bytes(), [200, 10, 30, 255]);
    }
    #[test]
    fn fit_preserves_extreme_aspect_ratios_and_does_not_enlarge_small_images() {
        assert_eq!(fitted_size(10_000, 100, 1900, 1000), (1900, 19));
        assert_eq!(fitted_size(100, 10_000, 1000, 1900), (19, 1900));
        assert_eq!(fitted_size(800, 600, 1900, 1000), (800, 600));
    }
}
