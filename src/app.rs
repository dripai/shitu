use std::{
    cell::RefCell,
    fmt::Write as _,
    rc::{Rc, Weak as RcWeak},
    thread,
    time::Duration,
};

use anyhow::{Result, anyhow};
use global_hotkey::GlobalHotKeyEvent;
use slint::{
    CloseRequestResponse, Color, ComponentHandle, ModelRc, PhysicalPosition, PhysicalSize,
    SharedString, Timer, VecModel,
};

use crate::{
    capture::{self, CapturedImage, DrawStyle},
    config::Config,
    hotkey::HotkeyState,
};

slint::include_modules!();

pub fn run() -> Result<(), slint::PlatformError> {
    let main = MainWindow::new()?;
    let tray = CaptureTray::new()?;
    let pins = Rc::new(RefCell::new(PinRegistry::default()));
    let state = Rc::new(RefCell::new(AppController::new(Rc::clone(&pins))));

    {
        let state = state.borrow();
        main.set_hotkey_text(state.config.hotkey.clone().unwrap_or_default().into());
        main.set_status_text(state.status.as_str().into());
    }

    bind_main_window(&main, Rc::clone(&state));
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
    {
        main.on_quit(|| {
            let _ = slint::quit_event_loop();
        });
    }
    {
        let main = main.as_weak();
        main.unwrap().on_save_settings(move |hotkey| {
            let status = {
                let mut state = state.borrow_mut();
                state.set_hotkey(hotkey.to_string());
                state.status.clone()
            };
            if let Some(main) = main.upgrade() {
                main.set_hotkey_text(
                    state
                        .borrow()
                        .config
                        .hotkey
                        .clone()
                        .unwrap_or_default()
                        .into(),
                );
                main.set_status_text(status.into());
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
        tray.on_open_settings(move || {
            if let Some(main) = main.upgrade() {
                main.set_settings_open(true);
            }
            show_main_window(&main);
        });
    }
    {
        let main = main.clone();
        tray.on_hide_main(move || {
            if let Some(main) = main.upgrade() {
                let _ = main.hide();
            }
        });
    }
    {
        let main = main.clone();
        let pins = state.borrow().pins.clone();
        tray.on_disable_pin_passthrough(move || {
            let changed = pins.borrow_mut().disable_passthrough();
            let status = if changed == 0 {
                "当前没有开启鼠标穿透的贴图".to_owned()
            } else {
                format!("已关闭 {changed} 个贴图的鼠标穿透")
            };
            set_status(&main, &mut state.borrow_mut(), status);
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
    crate::pin::activate(main.window());

    let main = main.as_weak();
    Timer::single_shot(Duration::from_millis(16), move || {
        if let Some(main) = main.upgrade() {
            main.window().request_redraw();
        }
    });
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

    overlay.set_desktop_image(desktop.slint_image());
    overlay.set_annotations(empty_annotation_model());
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
        overlay.unwrap().on_copy_selection(move || {
            let result = state
                .borrow()
                .rendered_selection()
                .and_then(|image| capture::copy_to_clipboard(&image));
            match result {
                Ok(()) => finish_capture(&overlay, &main, &state, "已复制到剪贴板".to_owned()),
                Err(error) => {
                    set_status(&main, &mut state.borrow_mut(), format!("复制失败：{error}"))
                }
            }
        });
    }
    {
        let overlay = overlay.as_weak();
        let main = main.clone();
        let state = Rc::clone(&state);
        overlay.unwrap().on_pin_selection(move || {
            let image = state.borrow().rendered_selection();
            let pins = state.borrow().pins.clone();
            match image.and_then(|image| PinRegistry::add(&pins, image)) {
                Ok(()) => finish_capture(&overlay, &main, &state, "已将截图贴到屏幕上".to_owned()),
                Err(error) => {
                    set_status(&main, &mut state.borrow_mut(), format!("贴图失败：{error}"))
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
    fn new(pins: Rc<RefCell<PinRegistry>>) -> Self {
        let config = Config::load();
        let hotkey = HotkeyState::new(config.hotkey.as_deref());
        Self {
            config,
            hotkey,
            status: "就绪。右键托盘图标可打开菜单。".to_owned(),
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

    fn set_hotkey(&mut self, value: String) {
        let value = value.trim().to_owned();
        self.config.hotkey = (!value.is_empty()).then_some(value);
        self.hotkey.set_binding(self.config.hotkey.as_deref());
        self.status = if let Some(error) = self.hotkey.error() {
            format!("快捷键设置失败：{error}")
        } else if let Err(error) = self.config.save() {
            format!("设置保存失败：{error}")
        } else if self.config.hotkey.is_some() {
            "快捷键已保存".to_owned()
        } else {
            "全局快捷键已关闭".to_owned()
        };
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
struct AnnotationHistory {
    commands: Vec<AnnotationCommand>,
    redo: Vec<AnnotationCommand>,
    active: bool,
}

impl AnnotationHistory {
    fn clear(&mut self) {
        self.commands.clear();
        self.redo.clear();
        self.active = false;
    }

    fn begin(&mut self, tool: i32, point: (u32, u32), style: DrawStyle) {
        self.finish();
        let command = match tool {
            1 => AnnotationCommand::Pen {
                points: vec![point],
                style,
            },
            2 => AnnotationCommand::Rectangle {
                start: point,
                end: point,
                style,
            },
            3 => AnnotationCommand::Arrow {
                start: point,
                end: point,
                style,
            },
            _ => return,
        };
        self.redo.clear();
        self.commands.push(command);
        self.active = true;
    }

    fn update(&mut self, point: (u32, u32)) {
        if !self.active {
            return;
        }
        match self.commands.last_mut() {
            Some(AnnotationCommand::Pen { points, .. }) => {
                if points.last().copied() != Some(point) {
                    points.push(point);
                }
            }
            Some(AnnotationCommand::Rectangle { end, .. })
            | Some(AnnotationCommand::Arrow { end, .. }) => {
                *end = point;
            }
            None => {}
        }
    }

    fn finish(&mut self) {
        self.active = false;
    }

    fn undo(&mut self) {
        self.finish();
        if let Some(command) = self.commands.pop() {
            self.redo.push(command);
        }
    }

    fn redo(&mut self) {
        self.finish();
        if let Some(command) = self.redo.pop() {
            self.commands.push(command);
        }
    }

    fn views(&self) -> Vec<AnnotationView> {
        self.commands.iter().map(AnnotationCommand::view).collect()
    }

    fn render(&self, base: &CapturedImage) -> CapturedImage {
        let mut image = base.clone();
        for command in &self.commands {
            command.render(&mut image);
        }
        image
    }
}

#[derive(Clone)]
enum AnnotationCommand {
    Pen {
        points: Vec<(u32, u32)>,
        style: DrawStyle,
    },
    Rectangle {
        start: (u32, u32),
        end: (u32, u32),
        style: DrawStyle,
    },
    Arrow {
        start: (u32, u32),
        end: (u32, u32),
        style: DrawStyle,
    },
}

impl AnnotationCommand {
    fn view(&self) -> AnnotationView {
        let (commands, style) = match self {
            Self::Pen { points, style } => (pen_path(points), *style),
            Self::Rectangle { start, end, style } => (rectangle_path(*start, *end), *style),
            Self::Arrow { start, end, style } => (arrow_path(*start, *end), *style),
        };
        AnnotationView {
            commands: commands.into(),
            stroke_color: Color::from_argb_u8(
                style.rgba[3],
                style.rgba[0],
                style.rgba[1],
                style.rgba[2],
            ),
            stroke_width: (style.radius * 2) as f32,
        }
    }

    fn render(&self, image: &mut CapturedImage) {
        match self {
            Self::Pen { points, style } => {
                if points.len() == 1 {
                    image.draw_line(points[0], points[0], *style);
                } else {
                    for pair in points.windows(2) {
                        image.draw_line(pair[0], pair[1], *style);
                    }
                }
            }
            Self::Rectangle { start, end, style } => {
                image.draw_rectangle(*start, *end, *style);
            }
            Self::Arrow { start, end, style } => {
                image.draw_arrow(*start, *end, *style);
            }
        }
    }
}

fn pen_path(points: &[(u32, u32)]) -> String {
    let Some(first) = points.first() else {
        return String::new();
    };
    let mut path = format!("M {} {}", first.0, first.1);
    for point in &points[1..] {
        let _ = write!(path, " L {} {}", point.0, point.1);
    }
    if points.len() == 1 {
        let _ = write!(path, " L {} {}", first.0, first.1);
    }
    path
}

fn rectangle_path(start: (u32, u32), end: (u32, u32)) -> String {
    let left = start.0.min(end.0);
    let right = start.0.max(end.0);
    let top = start.1.min(end.1);
    let bottom = start.1.max(end.1);
    format!("M {left} {top} L {right} {top} L {right} {bottom} L {left} {bottom} Z")
}

fn arrow_path(start: (u32, u32), end: (u32, u32)) -> String {
    let mut path = format!("M {} {} L {} {}", start.0, start.1, end.0, end.1);
    if let Some((left, right)) = capture::arrow_head(start, end) {
        let _ = write!(
            path,
            " M {} {} L {} {} M {} {} L {} {}",
            end.0, end.1, left.0, left.1, end.0, end.1, right.0, right.1
        );
    }
    path
}

#[derive(Default)]
struct PinRegistry {
    next_id: u64,
    windows: Vec<PinnedWindow>,
}

impl PinRegistry {
    fn add(registry: &Rc<RefCell<Self>>, image: CapturedImage) -> Result<()> {
        let pin = PinWindow::new()?;
        pin.set_screenshot(image.slint_image());
        pin.window()
            .set_position(PhysicalPosition::new(image.bounds.left, image.bounds.top));
        pin.window().set_size(PhysicalSize::new(
            image.bounds.width as u32,
            image.bounds.height as u32,
        ));

        let id = {
            let mut registry = registry.borrow_mut();
            let id = registry.next_id;
            registry.next_id += 1;
            id
        };
        let window_state = Rc::new(RefCell::new(PinnedWindowState {
            opacity: 255,
            click_through: false,
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
                    crate::pin::drag(pin.window());
                }
            });
        }
        {
            let pin = pin.as_weak();
            pin.unwrap().on_scale_pin(move |direction| {
                if let Some(pin) = pin.upgrade() {
                    let current = pin.window().size();
                    let factor = if direction > 0 { 1.15 } else { 1.0 / 1.15 };
                    let width = (current.width as f32 * factor).clamp(80.0, 4000.0) as u32;
                    let height = (current.height as f32 * factor).clamp(60.0, 3000.0) as u32;
                    pin.window().set_size(PhysicalSize::new(width, height));
                }
            });
        }
        {
            let pin = pin.as_weak();
            let window_state = Rc::clone(&window_state);
            pin.unwrap().on_cycle_opacity(move || {
                if let Some(pin) = pin.upgrade() {
                    let mut state = window_state.borrow_mut();
                    state.opacity = match state.opacity {
                        255 => 217,
                        217 => 179,
                        _ => 255,
                    };
                    pin.set_alpha_percent(((state.opacity as u16 * 100) / 255) as i32);
                    crate::pin::apply(pin.window(), state.opacity, state.click_through);
                }
            });
        }
        {
            let pin = pin.as_weak();
            let window_state = Rc::clone(&window_state);
            pin.unwrap().on_toggle_click_through(move || {
                if let Some(pin) = pin.upgrade() {
                    let mut state = window_state.borrow_mut();
                    state.click_through = !state.click_through;
                    pin.set_click_through(state.click_through);
                    crate::pin::apply(pin.window(), state.opacity, state.click_through);
                }
            });
        }

        pin.show()?;
        crate::pin::apply(pin.window(), 255, false);
        registry.borrow_mut().windows.push(PinnedWindow {
            id,
            ui: pin,
            state: window_state,
        });
        Ok(())
    }

    fn disable_passthrough(&mut self) -> usize {
        let mut changed = 0;
        for pin in &self.windows {
            let mut state = pin.state.borrow_mut();
            if !state.click_through {
                continue;
            }
            state.click_through = false;
            pin.ui.set_click_through(false);
            crate::pin::apply(pin.ui.window(), state.opacity, false);
            changed += 1;
        }
        changed
    }
}

struct PinnedWindow {
    id: u64,
    ui: PinWindow,
    state: Rc<RefCell<PinnedWindowState>>,
}

struct PinnedWindowState {
    opacity: u8,
    click_through: bool,
}

#[cfg(test)]
mod tests {
    use super::{
        AnnotationCommand, AnnotationHistory, DrawStyle, arrow_path, normalized_selection,
        rectangle_path,
    };

    #[test]
    fn annotation_history_undo_and_redo_preserve_commands() {
        let mut history = AnnotationHistory::default();
        history.begin(
            2,
            (10, 20),
            DrawStyle {
                rgba: [255, 0, 0, 255],
                radius: 2,
            },
        );
        history.update((30, 40));
        history.finish();
        assert_eq!(history.commands.len(), 1);

        history.undo();
        assert!(history.commands.is_empty());
        history.redo();
        assert_eq!(history.commands.len(), 1);
    }

    #[test]
    fn shape_paths_use_image_coordinates() {
        assert_eq!(
            rectangle_path((30, 40), (10, 20)),
            "M 10 20 L 30 20 L 30 40 L 10 40 Z"
        );
        assert!(arrow_path((10, 10), (30, 20)).starts_with("M 10 10 L 30 20"));
        let _ = AnnotationCommand::Pen {
            points: vec![(1, 1), (2, 2)],
            style: DrawStyle {
                rgba: [0, 0, 0, 255],
                radius: 1,
            },
        };
    }

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
