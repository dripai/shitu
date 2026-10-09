use crate::i18n;
use anyhow::Result;
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem},
};

pub enum Action {
    Capture,
    Show,
    Hide,
    Quit,
}
pub struct Tray {
    _icon: TrayIcon,
    capture: MenuItem,
    show: MenuItem,
    hide: MenuItem,
    quit: MenuItem,
}
impl Tray {
    pub fn new() -> Result<Self> {
        let menu = Menu::new();
        let capture = MenuItem::new(i18n::text("开始截图"), true, None);
        let show = MenuItem::new(i18n::text("显示控制面板"), true, None);
        let hide = MenuItem::new(i18n::text("隐藏控制面板"), true, None);
        let quit = MenuItem::new(i18n::text("退出"), true, None);
        menu.append_items(&[&capture, &show, &hide, &quit])?;
        let image = image::load_from_memory(include_bytes!("../../assets/app.png"))?
            .resize(32, 32, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        let icon = TrayIconBuilder::new()
            .with_tooltip(i18n::text("拾图"))
            .with_icon(Icon::from_rgba(image.into_raw(), 32, 32)?)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()?;
        Ok(Self {
            _icon: icon,
            capture,
            show,
            hide,
            quit,
        })
    }
    pub fn refresh_language(&self) {
        self.capture.set_text(i18n::text("开始截图"));
        self.show.set_text(i18n::text("显示控制面板"));
        self.hide.set_text(i18n::text("隐藏控制面板"));
        self.quit.set_text(i18n::text("退出"));
    }
    pub fn next(&self) -> Option<Action> {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.capture.id() {
                return Some(Action::Capture);
            }
            if event.id == self.show.id() {
                return Some(Action::Show);
            }
            if event.id == self.hide.id() {
                return Some(Action::Hide);
            }
            if event.id == self.quit.id() {
                return Some(Action::Quit);
            }
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                return Some(Action::Show);
            }
        }
        None
    }
}
