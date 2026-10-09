mod annotation;
mod editor;
mod toolbar_layout;
mod tray;

use crate::{
    capture,
    config::{AppearanceMode, Config, ImageFormat, LanguageMode, OcrEngineKind},
    hotkey::{HotkeyState, validate_binding},
    i18n,
    image::CapturedImage,
    logging,
    platform::{
        ocr::{self, AiOcrState, OcrFailure},
        windows::{shell, window as native},
    },
    settings,
};
use anyhow::{Result, anyhow};
use global_hotkey::{GlobalHotKeyEvent, HotKeyState as KeyState};
use gpui_kit::component::{
    button::*,
    checkbox::Checkbox,
    input::{Input, InputState, Textarea, TextareaState},
    menu::{DropdownMenu, PopupMenuItem},
    scroll::ScrollableElement,
    tab::{Tab, TabBar},
    *,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::{atomic::Ordering, mpsc},
    time::{Duration, Instant},
};

pub(super) enum Message {
    Status(String),
    CaptureClosed,
    Ocr(Result<String, OcrFailure>),
    Pin(CapturedImage, Option<PathBuf>),
    Ai(Result<AiOcrState, OcrFailure>, bool),
}

pub(super) struct Shared {
    config: Config,
    sender: mpsc::Sender<Message>,
}
type SharedApp = Rc<RefCell<Shared>>;

pub fn run(start_minimized: bool) -> Result<()> {
    logging::initialize(Config::log_directory(), "gridstart.log");
    logging::info("UI: GPUI Kit 0.7.1");
    i18n::prepare(LanguageMode::System);
    let config = Config::load()?;
    i18n::prepare(config.language);
    // GPUI initializes OLE/STA on its UI thread; system OCR requires an MTA.
    let system_ocr = std::thread::spawn(ocr::system_availability)
        .join()
        .map_err(|_| anyhow!("System OCR availability worker panicked"))?;
    let (sender, receiver) = mpsc::channel();
    let shared = Rc::new(RefCell::new(Shared {
        config: config.clone(),
        sender,
    }));
    let startup_error = Rc::new(RefCell::new(None));
    let launch_error = startup_error.clone();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            gpui_kit::component::set_locale(i18n::current_language().bundle_code());
            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(i18n::text("拾图").into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(640.), px(560.)),
                    cx,
                ))),
                window_min_size: Some(size(px(540.), px(460.))),
                ..Default::default()
            };
            let result = gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| Panel::new(shared, receiver, system_ocr, window, cx))
            });
            match result {
                Ok((handle, panel)) => {
                    if start_minimized
                        && panel.read(cx).tray.is_some()
                        && let Err(error) = handle
                            .update(cx, |_, window, _| native::hide(window))
                            .and_then(|r| r)
                    {
                        logging::error(error.to_string());
                    }
                }
                Err(error) => {
                    logging::error(format!("Open control panel: {error:#}"));
                    *launch_error.borrow_mut() = Some(error);
                    cx.quit();
                }
            }
        });
    match startup_error.borrow_mut().take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

struct Panel {
    shared: SharedApp,
    receiver: mpsc::Receiver<Message>,
    draft: Config,
    fields: HashMap<&'static str, Entity<InputState>>,
    hotkey: HotkeyState,
    tray: Option<tray::Tray>,
    tab: usize,
    status: String,
    ai: AiOcrState,
    system_ocr: Result<(), OcrFailure>,
    capture_due: Option<Instant>,
    capturing: bool,
    restore_main: bool,
    _subscriptions: Vec<Subscription>,
}

pub(super) fn input(
    value: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    let value = value.into();
    cx.new(|cx| {
        let mut state = InputState::new(window, cx);
        state.set_value(value, window, cx);
        state
    })
}

// GPUI's built-in editing labels do not cover all ten ShiTu languages.
// Reuse NativeMenu and official editing actions; only supply our catalog text.
pub(super) fn text_menu(
    menu: component::native_menu::NativeMenu,
    _: &mut Window,
    _: &mut App,
) -> component::native_menu::NativeMenu {
    use component::input::{Copy, Cut, Paste, SelectAll};
    // Called while InputState is mutably borrowed by the engine. Never read
    // that entity here; editing actions enforce selection/editability themselves.
    menu.menu(i18n::text("Cut"), Box::new(Cut))
        .menu(i18n::text("Copy"), Box::new(Copy))
        .menu(i18n::text("Paste"), Box::new(Paste))
        .separator()
        .menu(i18n::text("Select All"), Box::new(SelectAll))
}

pub(super) fn text_input(state: &Entity<InputState>) -> Input {
    Input::new(state).context_menu(text_menu)
}

impl Panel {
    fn new(
        shared: SharedApp,
        receiver: mpsc::Receiver<Message>,
        system_ocr: Result<(), OcrFailure>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let draft = shared.borrow().config.clone();
        let hotkey = HotkeyState::new(draft.hotkey.as_deref());
        let mut status = i18n::text("就绪。右键托盘图标可打开菜单。").to_owned();
        if let Some(error) = hotkey.error() {
            status = error.message().to_owned();
        }
        let tray = match tray::Tray::new() {
            Ok(tray) => Some(tray),
            Err(error) => {
                status = format!("Tray: {error:#}");
                None
            }
        };
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |panel, cx| {
                if panel.tray.is_none() {
                    cx.quit();
                    return true;
                }
                if let Err(error) = native::hide(window) {
                    panel.status = error.to_string();
                    cx.notify();
                }
                false
            })
            .unwrap_or(true)
        });
        let theme_subscription =
            cx.observe_window_appearance(window, |panel, window, cx| panel.apply_theme(window, cx));
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                if cx
                    .update(|window, cx| this.update(cx, |panel, cx| panel.poll(window, cx)))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let mut panel = Self {
            shared,
            receiver,
            draft,
            fields: HashMap::new(),
            hotkey,
            tray,
            tab: 0,
            status,
            ai: AiOcrState::Checking,
            system_ocr,
            capture_due: None,
            capturing: false,
            restore_main: false,
            _subscriptions: vec![theme_subscription],
        };
        panel.populate(window, cx);
        panel.apply_theme(window, cx);
        panel.probe_ai(false);
        panel
    }

    fn populate(&mut self, window: &mut Window, cx: &mut App) {
        for (key, value) in [
            (
                "directory",
                self.draft
                    .capture
                    .save_directory
                    .to_string_lossy()
                    .into_owned(),
            ),
            ("filename", self.draft.capture.filename_template.clone()),
            ("quality", self.draft.capture.jpeg_quality.to_string()),
            ("confidence", self.draft.ocr.minimum_confidence.to_string()),
            ("opacity", self.draft.pin.default_opacity.to_string()),
            ("zoom", self.draft.pin.zoom_step.to_string()),
            ("hotkey", self.draft.hotkey.clone().unwrap_or_default()),
        ] {
            if let Some(field) = self.fields.get(key) {
                field.update(cx, |state, cx| state.set_value(value, window, cx));
            } else {
                self.fields.insert(key, input(value, window, cx));
            }
        }
    }
    fn value(&self, key: &str, cx: &App) -> String {
        self.fields[key].read(cx).value().to_string()
    }
    fn candidate(&self, cx: &App) -> Result<Config> {
        let mut config = self.draft.clone();
        config.capture.save_directory = self.value("directory", cx).into();
        config.capture.filename_template = self.value("filename", cx);
        config.capture.jpeg_quality = self.value("quality", cx).parse()?;
        config.ocr.minimum_confidence = self.value("confidence", cx).parse()?;
        config.pin.default_opacity = self.value("opacity", cx).parse()?;
        config.pin.zoom_step = self.value("zoom", cx).parse()?;
        let hotkey = self.value("hotkey", cx).trim().to_owned();
        config.hotkey = (!hotkey.is_empty()).then_some(hotkey);
        config.validate()?;
        if let Some(binding) = &config.hotkey {
            validate_binding(binding).map_err(|e| anyhow!(e.message()))?;
        }
        Ok(config)
    }
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.candidate(cx).and_then(|candidate| {
            settings::apply_transaction(
                &self.shared.borrow().config,
                &candidate,
                &mut self.hotkey,
            )?;
            self.shared.borrow_mut().config = candidate.clone();
            self.draft = candidate;
            Ok(())
        });
        if result.is_ok() {
            self.populate(window, cx);
        }
        self.report(result, i18n::text("设置已保存"));
        self.apply_language(window, cx);
        cx.notify();
    }
    fn report(&mut self, result: Result<()>, success: &str) {
        self.status = match result {
            Ok(()) => success.to_owned(),
            Err(e) => {
                logging::error(format!("{e:#}"));
                format!("{}: {e:#}", i18n::text("操作失败"))
            }
        };
    }
    fn apply_theme(&self, window: &mut Window, cx: &mut App) {
        let mode = match self.draft.appearance {
            AppearanceMode::System => window.appearance().into(),
            AppearanceMode::Light => ThemeMode::Light,
            AppearanceMode::Dark => ThemeMode::Dark,
        };
        Theme::change(mode, Some(window), cx);
    }
    fn apply_language(&self, window: &mut Window, cx: &mut App) {
        i18n::prepare(self.draft.language);
        gpui_kit::component::set_locale(i18n::current_language().bundle_code());
        window.set_window_title(i18n::text("拾图"));
        if let Some(tray) = &self.tray {
            tray.refresh_language();
        }
        self.apply_theme(window, cx);
        cx.refresh_windows();
    }
    fn probe_ai(&mut self, prepare: bool) {
        self.ai = if prepare {
            AiOcrState::Preparing
        } else {
            AiOcrState::Checking
        };
        let sender = self.shared.borrow().sender.clone();
        std::thread::spawn(move || {
            let result = if prepare {
                ocr::prepare_ai()
            } else {
                ocr::ai_availability()
            };
            let _ = sender.send(Message::Ai(result, prepare));
        });
    }
    fn start_capture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.capturing {
            return;
        }
        match native::visible(window).and_then(|visible| {
            if visible {
                native::hide(window)?;
            }
            Ok(visible)
        }) {
            Ok(visible) => {
                logging::info("capture scheduled");
                self.restore_main = visible;
                self.capturing = true;
                self.capture_due = Some(Instant::now() + Duration::from_millis(160));
            }
            Err(error) => self.report(Err(error), ""),
        }
        cx.notify();
    }
    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            let active = self.hotkey.active_id_handle().load(Ordering::Relaxed);
            if should_trigger_hotkey_event(&event, active) {
                self.start_capture(window, cx);
            }
        }
        while let Some(action) = self.tray.as_ref().and_then(tray::Tray::next) {
            match action {
                tray::Action::Capture => self.start_capture(window, cx),
                tray::Action::Show => {
                    self.report(native::show(window), i18n::text("就绪"));
                    cx.notify();
                }
                tray::Action::Hide => {
                    self.report(native::hide(window), i18n::text("就绪"));
                    cx.notify();
                }
                tray::Action::Quit => cx.quit(),
            }
        }
        if self.capture_due.is_some_and(|due| due <= Instant::now()) {
            logging::info("capture opening");
            self.capture_due = None;
            if let Err(error) = editor::open_capture(self.shared.clone(), cx) {
                self.report(Err(error), "");
                self.capture_finished(window);
                cx.notify();
            }
        }
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                Message::Status(status) => self.status = status,
                Message::CaptureClosed => self.capture_finished(window),
                Message::Pin(image, path) => {
                    self.report(
                        editor::open_pin(image, path, self.shared.clone(), cx),
                        i18n::text("钉住"),
                    );
                }
                Message::Ocr(result) => {
                    let success = result.is_ok();
                    let text = match result {
                        Ok(text) if text.trim().is_empty() => i18n::text("未识别到文字").to_owned(),
                        Ok(text) => text,
                        Err(error) => error.message(),
                    };
                    let status = if success {
                        i18n::text("OCR 识别完成").to_owned()
                    } else {
                        text.clone()
                    };
                    self.report(open_result(text, cx), &status);
                }
                Message::Ai(result, requested) => {
                    self.ai = match result {
                        Ok(state) => state,
                        Err(OcrFailure::AiUnavailable(state)) => state,
                        Err(error) => AiOcrState::Failed(error.message()),
                    };
                    if requested || (self.system_ocr.is_err() && !self.ai.is_ready()) {
                        self.status = self.ai.message();
                    }
                }
            }
            cx.notify();
        }
    }
    fn capture_finished(&mut self, window: &mut Window) {
        self.capturing = false;
        if self.restore_main
            && let Err(error) = native::show_without_activation(window)
        {
            self.report(Err(error), "");
        }
        self.restore_main = false;
    }
    fn restore_defaults(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Preserve even invalid, unfinished input on all other pages.
        let retained: Vec<_> = self
            .fields
            .keys()
            .copied()
            .filter(|key| field_page(key) != self.tab)
            .map(|key| (key, self.value(key, cx)))
            .collect();
        let defaults = Config::default();
        match self.tab {
            0 => {
                self.draft.appearance = defaults.appearance;
                self.draft.language = defaults.language;
                self.draft.launch_at_startup = defaults.launch_at_startup;
                self.draft.start_minimized = defaults.start_minimized;
            }
            1 => self.draft.capture = defaults.capture,
            2 => self.draft.ocr = defaults.ocr,
            3 => self.draft.pin = defaults.pin,
            4 => self.draft.hotkey = defaults.hotkey,
            _ => return,
        }
        self.populate(window, cx);
        for (key, value) in retained {
            self.fields[key].update(cx, |state, cx| state.set_value(value, window, cx));
        }
        self.apply_language(window, cx);
        self.status = i18n::text("已恢复当前页默认值，点击保存后生效").to_owned();
        cx.notify();
    }
    fn field(&self, label: &'static str, key: &'static str) -> AnyElement {
        row(label, text_input(&self.fields[key]).into_any_element()).into_any_element()
    }
    fn checkbox(
        &self,
        id: &'static str,
        label: &'static str,
        checked: bool,
        set: fn(&mut Config, bool),
        cx: &Context<Self>,
    ) -> AnyElement {
        Checkbox::new(id)
            .label(label)
            .checked(checked)
            .on_click(cx.listener(move |this, checked, _, cx| {
                set(&mut this.draft, *checked);
                cx.notify();
            }))
            .into_any_element()
    }
    fn choice(
        &self,
        id: &'static str,
        label: &'static str,
        labels: Vec<&'static str>,
        selected: usize,
        set: fn(&mut Config, usize),
        cx: &Context<Self>,
    ) -> AnyElement {
        let weak = cx.weak_entity();
        let button =
            Button::new(id)
                .label(labels[selected])
                .dropdown_menu(move |mut menu, _, _| {
                    for (index, label) in labels.iter().copied().enumerate() {
                        let weak = weak.clone();
                        menu = menu.item(
                            PopupMenuItem::new(label)
                                .checked(index == selected)
                                .on_click(move |_, window, cx| {
                                    let _ = weak.update(cx, |panel, cx| {
                                        set(&mut panel.draft, index);
                                        panel.apply_language(window, cx);
                                        cx.notify();
                                    });
                                }),
                        );
                    }
                    menu
                });
        row(label, button.into_any_element()).into_any_element()
    }
}

fn field_page(key: &str) -> usize {
    match key {
        "directory" | "filename" | "quality" => 1,
        "confidence" => 2,
        "opacity" | "zoom" => 3,
        "hotkey" => 4,
        _ => unreachable!("Unknown settings field"),
    }
}

fn row(label: &'static str, control: AnyElement) -> Div {
    div()
        .h_flex()
        .gap_4()
        .items_center()
        .child(div().w(px(136.)).flex_shrink_0().child(label))
        .child(div().flex_1().min_w_0().child(control))
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let labels = [
            i18n::text("常规"),
            i18n::text("截图"),
            i18n::text("OCR 识别"),
            i18n::text("钉住"),
            i18n::text("快捷键"),
            i18n::text("关于"),
        ];
        let tabs = TabBar::new("settings-tabs")
            .selected_index(self.tab)
            .children(labels.into_iter().map(|label| Tab::new().label(label)))
            .on_click(cx.listener(|this, index, _, cx| {
                this.tab = *index;
                cx.notify();
            }));
        let mut page = div().v_flex().gap_4().p_5();
        match self.tab {
            0 => {
                page = page
                    .child(self.choice(
                        "appearance",
                        i18n::text("外观模式"),
                        vec![
                            i18n::text("跟随系统"),
                            i18n::text("浅色"),
                            i18n::text("深色"),
                        ],
                        self.draft.appearance as usize,
                        |c, i| {
                            c.appearance = [
                                AppearanceMode::System,
                                AppearanceMode::Light,
                                AppearanceMode::Dark,
                            ][i]
                        },
                        cx,
                    ))
                    .child(self.choice(
                        "language",
                        i18n::text("界面语言"),
                        vec![
                            i18n::text("跟随系统"),
                            "简体中文",
                            "English",
                            "日本語",
                            "한국어",
                            "Français",
                            "Deutsch",
                            "Español",
                            "Português",
                            "Русский",
                            "हिन्दी",
                        ],
                        self.draft.language as usize,
                        |c, i| c.language = LanguageMode::ALL[i],
                        cx,
                    ))
                    .child(self.checkbox(
                        "startup",
                        i18n::text("开机启动"),
                        self.draft.launch_at_startup,
                        |c, v| c.launch_at_startup = v,
                        cx,
                    ))
                    .child(self.checkbox(
                        "minimized",
                        i18n::text("启动最小化"),
                        self.draft.start_minimized,
                        |c, v| c.start_minimized = v,
                        cx,
                    ))
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(Button::new("logs").label(i18n::text("打开日志")).on_click(
                                cx.listener(|this, _, _, cx| {
                                    let path = Config::log_directory();
                                    let result = std::fs::create_dir_all(&path)
                                        .map_err(anyhow::Error::from)
                                        .and_then(|_| shell::open_path(&path));
                                    this.report(result, i18n::text("已打开日志文件夹"));
                                    cx.notify();
                                }),
                            ))
                            .child(
                                Button::new("config")
                                    .label(i18n::text("打开配置"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let result = if Config::path().exists() {
                                            Ok(())
                                        } else {
                                            this.shared.borrow().config.save()
                                        }
                                        .and_then(|_| shell::open_path(&Config::directory()));
                                        this.report(result, i18n::text("已打开配置文件夹"));
                                        cx.notify();
                                    })),
                            ),
                    );
            }
            1 => {
                page = page
                    .child(self.choice(
                        "format",
                        i18n::text("图像格式"),
                        vec!["PNG", "JPEG"],
                        self.draft.capture.format as usize,
                        |c, i| c.capture.format = [ImageFormat::Png, ImageFormat::Jpeg][i],
                        cx,
                    ))
                    .child(self.field(i18n::text("质量"), "quality"))
                    .child(self.field(i18n::text("保存目录"), "directory"))
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(
                                Button::new("choose-dir")
                                    .label(i18n::text("选择..."))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if let Some(path) = rfd::FileDialog::new()
                                            .set_parent(window)
                                            .set_directory(this.value("directory", cx))
                                            .pick_folder()
                                        {
                                            this.fields["directory"].update(cx, |state, cx| {
                                                state.set_value(
                                                    path.to_string_lossy().into_owned(),
                                                    window,
                                                    cx,
                                                )
                                            });
                                        }
                                    })),
                            )
                            .child(
                                Button::new("open-dir")
                                    .label(i18n::text("打开目录"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let path = PathBuf::from(this.value("directory", cx));
                                        let result = std::fs::create_dir_all(&path)
                                            .map_err(anyhow::Error::from)
                                            .and_then(|_| shell::open_path(&path));
                                        this.report(result, i18n::text("已打开保存目录"));
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(self.field(i18n::text("文件名模板"), "filename"))
                    .child(self.checkbox(
                        "auto-save",
                        i18n::text("自动保存（执行复制或钉住时同时保存）"),
                        self.draft.capture.auto_save,
                        |c, v| c.capture.auto_save = v,
                        cx,
                    ))
                    .child(self.checkbox(
                        "save-notice",
                        i18n::text("显示保存结果"),
                        self.draft.capture.save_notification,
                        |c, v| c.capture.save_notification = v,
                        cx,
                    ));
            }
            2 => {
                page = page
                    .child(self.choice(
                        "ocr-engine",
                        i18n::text("默认引擎"),
                        vec![
                            i18n::text("兼容系统 OCR"),
                            i18n::text("Windows AI OCR（增强）"),
                        ],
                        self.draft.ocr.engine as usize,
                        |c, i| c.ocr.engine = [OcrEngineKind::System, OcrEngineKind::WindowsAi][i],
                        cx,
                    ))
                    .child(match &self.system_ocr {
                        Ok(()) => i18n::text("可用（Windows 系统 OCR）").to_owned(),
                        Err(e) => e.message(),
                    })
                    .child(self.ai.message())
                    .child(
                        Button::new("install-ai")
                            .label(i18n::text("安装模型"))
                            .disabled(!self.ai.can_install())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.probe_ai(true);
                                cx.notify();
                            })),
                    )
                    .child(self.field(i18n::text("最低置信度"), "confidence"))
                    .child(
                        div()
                            .text_sm()
                            .child(i18n::text("仅过滤 Windows AI OCR 返回的低置信度词语。")),
                    );
            }
            3 => {
                page = page
                    .child(self.field(i18n::text("默认不透明度"), "opacity"))
                    .child(self.checkbox(
                        "shadow",
                        i18n::text("窗口阴影"),
                        self.draft.pin.shadow,
                        |c, v| c.pin.shadow = v,
                        cx,
                    ))
                    .child(self.checkbox(
                        "top",
                        i18n::text("始终置顶"),
                        self.draft.pin.always_on_top,
                        |c, v| c.pin.always_on_top = v,
                        cx,
                    ))
                    .child(self.checkbox(
                        "wheel",
                        i18n::text("启用滚轮缩放"),
                        self.draft.pin.wheel_zoom,
                        |c, v| c.pin.wheel_zoom = v,
                        cx,
                    ))
                    .child(self.field(i18n::text("缩放步长"), "zoom"))
                    .child(self.checkbox(
                        "double-close",
                        i18n::text("双击关闭"),
                        self.draft.pin.double_click_close,
                        |c, v| c.pin.double_click_close = v,
                        cx,
                    ));
            }
            4 => {
                page = page
                    .child(self.field(i18n::text("区域截图"), "hotkey"))
                    .child(i18n::text("默认 Ctrl+Alt+C；留空则关闭"))
                    .child(
                        Button::new("clear-hotkey")
                            .label(i18n::text("清除"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let result = this
                                    .hotkey
                                    .set_binding(None)
                                    .map_err(|e| anyhow!(e.message()));
                                if result.is_ok() {
                                    this.fields["hotkey"]
                                        .update(cx, |state, cx| state.set_value("", window, cx));
                                }
                                this.report(result, i18n::text("快捷键已注销，点击保存后永久生效"));
                                cx.notify();
                            })),
                    );
            }
            _ => {
                let icon = std::sync::Arc::new(gpui_kit::Image::from_bytes(
                    gpui_kit::ImageFormat::Png,
                    include_bytes!("../assets/app.png").to_vec(),
                ));
                page = page
                    .child(
                        div()
                            .h_flex()
                            .gap_3()
                            .child(img(icon).size(px(44.)).flex_shrink_0())
                            .child(
                                div()
                                    .v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_xl()
                                            .font_weight(FontWeight::BOLD)
                                            .child(i18n::text("拾图")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .pl_4()
                                    .text_sm()
                                    .child(i18n::text("一款轻量、便捷的截图、标注与钉图工具。")),
                            ),
                    )
                    .child(div().h(px(1.)).bg(cx.theme().border))
                    .child(div().text_color(cx.theme().muted_foreground).child(format!(
                        "Windows {} · GPUI Kit 0.7.1 · {} · {}",
                        std::env::consts::ARCH,
                        if cfg!(debug_assertions) {
                            "Debug"
                        } else {
                            "Release"
                        },
                        env!("SHITU_BUILD_DATE")
                    )))
                    .child(
                        div()
                            .h_flex()
                            .child(div().w(px(80.)).child(i18n::text("作者：")))
                            .child("logic"),
                    )
                    .child(
                        div()
                            .h_flex()
                            .child(div().w(px(80.)).child("Twitter:"))
                            .child(
                                Button::new("about-twitter")
                                    .link()
                                    .label("@wz220321")
                                    .on_click(|_, _, cx| cx.open_url("https://x.com/wz220321")),
                            ),
                    )
                    .child(
                        div()
                            .h_flex()
                            .child(div().w(px(80.)).child("GitHub:"))
                            .child(
                                Button::new("about-github")
                                    .link()
                                    .label("dripai/shitu")
                                    .on_click(|_, _, cx| {
                                        cx.open_url("https://github.com/dripai/shitu")
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(i18n::text("使用 Rust 与 GPUI Kit 构建 · © 2026")),
                    );
            }
        }
        div()
            .v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm()
            .child(tabs)
            .child(
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(page),
            )
            .child(
                div()
                    .h_flex()
                    .p_3()
                    .gap_2()
                    .child(
                        Button::new("capture")
                            .primary()
                            .label(i18n::text("开始截图"))
                            .disabled(self.capturing)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.start_capture(window, cx)),
                            ),
                    )
                    .child(div().flex_1())
                    .when(self.tab != 5, |row| {
                        row.child(
                            Button::new("restore")
                                .label(i18n::text("恢复默认"))
                                .disabled(self.tab == 5)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.restore_defaults(window, cx)
                                })),
                        )
                        .child(
                            Button::new("save")
                                .label(i18n::text("保存"))
                                .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                        )
                    }),
            )
            .child(div().px_3().pb_3().child(self.status.clone()))
    }
}

struct OcrResult {
    text: Entity<TextareaState>,
    status: String,
}
fn open_result(text: String, cx: &mut App) -> Result<()> {
    gpui_kit::open_window(
        WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some(i18n::text("OCR 识别结果").into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(600.), px(440.)),
                cx,
            ))),
            ..Default::default()
        },
        cx,
        |window, cx| {
            let text = cx.new(|cx| {
                let mut state = TextareaState::new(window, cx);
                state.set_value(text, window, cx);
                state
            });
            cx.new(|_| OcrResult {
                text,
                status: String::new(),
            })
        },
    )?;
    Ok(())
}
impl Render for OcrResult {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .v_flex()
            .p_4()
            .gap_3()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                Textarea::new(&self.text).context_menu(text_menu).h((window
                    .viewport_size()
                    .height
                    - px(110.))
                .max(px(80.))),
            )
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        Button::new("copy")
                            .primary()
                            .label(i18n::text("复制全部"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.status = match capture::copy_text_to_clipboard(
                                    this.text.read(cx).value().as_ref(),
                                ) {
                                    Ok(()) => i18n::text("已复制全部文字").to_owned(),
                                    Err(error) => error.to_string(),
                                };
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("close")
                            .label(i18n::text("关闭"))
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            )
            .child(self.status.clone())
    }
}

fn should_trigger_hotkey_event(event: &GlobalHotKeyEvent, active: u32) -> bool {
    active != 0 && event.id == active && event.state == KeyState::Pressed
}

#[cfg(test)]
mod tests {
    use super::{GlobalHotKeyEvent, KeyState, should_trigger_hotkey_event};
    #[test]
    fn hotkey_events_require_pressed_state_and_current_id() {
        let event = GlobalHotKeyEvent {
            id: 42,
            state: KeyState::Pressed,
        };
        assert!(should_trigger_hotkey_event(&event, 42));
        assert!(!should_trigger_hotkey_event(&event, 0));
        assert!(!should_trigger_hotkey_event(&event, 7));
        assert!(!should_trigger_hotkey_event(
            &GlobalHotKeyEvent {
                id: 42,
                state: KeyState::Released
            },
            42
        ));
    }
}
