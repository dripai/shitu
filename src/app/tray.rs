//! GPUI Kit 0.7.1 PopupMenu handles rendering, keyboard navigation and dismissal.
//! NativeMenu uses Windows selection-fade; AnchoredPopup is unsupported by
//! gpui-pre-windows 0.3.8. This host only adds desktop placement and deactivation.
//! https://github.com/longbridge/gpui-kit/blob/87d10ae5e1299d1be18670a37570c5402f952e34/crates/component/src/menu/popup_menu.rs
use super::{Message, SharedApp};
use crate::{i18n, logging, platform::windows::window as native};
use anyhow::Result;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::RECT;

#[derive(Clone, Copy)]
pub enum Action {
    Capture,
    Show,
    Hide,
    Quit,
    Menu { x: i32, y: i32 },
}

pub struct Tray {
    icon: TrayIcon,
    menu: Option<AnyWindowHandle>,
}

impl Tray {
    pub fn new() -> Result<Self> {
        let image = image::load_from_memory(include_bytes!("../../assets/app.png"))?
            .resize(32, 32, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        let icon = TrayIconBuilder::new()
            .with_tooltip(i18n::text("拾图"))
            .with_icon(Icon::from_rgba(image.into_raw(), 32, 32)?)
            .build()?;
        Ok(Self { icon, menu: None })
    }

    pub fn refresh_language(&self) {
        // Menu labels are read from the active catalog each time it opens.
        if let Err(error) = self.icon.set_tooltip(Some(i18n::text("拾图"))) {
            logging::error(format!("Tray tooltip: {error}"));
        }
    }

    pub fn next(&self) -> Option<Action> {
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if event.id() != self.icon.id() {
                continue;
            }
            match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } => return Some(Action::Show),
                TrayIconEvent::Click {
                    button: MouseButton::Right,
                    button_state: MouseButtonState::Up,
                    position,
                    ..
                } => {
                    return Some(Action::Menu {
                        x: position.x.round() as i32,
                        y: position.y.round() as i32,
                    });
                }
                _ => {}
            }
        }
        None
    }

    pub fn open_menu(&mut self, x: i32, y: i32, shared: SharedApp, cx: &mut App) -> Result<()> {
        let sender = shared.borrow().sender.clone();
        if let Some(previous) = self.menu.take() {
            // A closed window leaves a stale handle, which is safe to discard.
            previous
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
        let options = WindowOptions {
            titlebar: None,
            kind: WindowKind::PopUp,
            focus: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(HOST_WIDTH), px(HOST_HEIGHT)),
                cx,
            ))),
            ..Default::default()
        };
        let (handle, _) = gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| TrayMenu::new(x, y, shared, window, cx))
        })?;
        let setup = handle
            .update(cx, |_, window, cx| -> Result<()> {
                native::hide(window)?;
                native::prepare_popup_window(window, false)?;
                native::set_always_on_top(window, true)?;
                let root = window
                    .root::<base::Root>()
                    .flatten()
                    .ok_or_else(|| anyhow::anyhow!("Tray menu requires GPUI Base Root"))?;
                root.update(cx, |root, cx| {
                    root.style().refine(&StyleRefinement::default().bg(rgba(0)));
                    cx.notify();
                });
                let area = native::work_area_for_rect(anchor_rect(x, y))?;
                native::place(window, menu_rect(x, y, window.scale_factor(), area))?;
                Ok(())
            })
            .and_then(|result| result);
        if let Err(error) = setup {
            handle
                .update(cx, |_, window, _| window.remove_window())
                .ok();
            return Err(error);
        }
        self.menu = Some(handle.into());
        logging::info("application tray menu opening");
        cx.defer(move |cx| {
            if let Err(error) = handle
                .update(cx, |_, window, _| {
                    native::show_without_activation(window)?;
                    window.activate_window();
                    Ok::<_, anyhow::Error>(())
                })
                .and_then(|result| result)
            {
                logging::error(format!("Show tray menu: {error:#}"));
                handle
                    .update(cx, |_, window, _| window.remove_window())
                    .ok();
                let _ = sender.send(Message::Status(format!(
                    "{}: {error:#}",
                    i18n::text("操作失败")
                )));
            }
        });
        Ok(())
    }
}

const PADDING: f32 = 8.;
const MENU_WIDTH: f32 = 160.;
const HOST_WIDTH: f32 = MENU_WIDTH + PADDING * 2.;
// PopupMenu's four 26px rows, three 2px gaps and 8px inner padding.
const HOST_HEIGHT: f32 = 4. * 26. + 3. * 2. + 8. + PADDING * 2.;

fn anchor_rect(x: i32, y: i32) -> RECT {
    RECT {
        left: x,
        top: y,
        right: x + 1,
        bottom: y + 1,
    }
}

fn menu_rect(x: i32, y: i32, scale: f32, area: RECT) -> RECT {
    let width = ((HOST_WIDTH * scale).ceil() as i32).min(area.right - area.left);
    let height = ((HOST_HEIGHT * scale).ceil() as i32).min(area.bottom - area.top);
    let left = if x + width <= area.right {
        x
    } else {
        x - width
    }
    .clamp(area.left, area.right - width);
    let top = if y + height <= area.bottom {
        y
    } else {
        y - height
    }
    .clamp(area.top, area.bottom - height);
    RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

struct TrayMenu {
    menu: Entity<PopupMenu>,
    shared: SharedApp,
    closing: bool,
    activated: bool,
    _subscriptions: Vec<Subscription>,
}

impl TrayMenu {
    fn new(x: i32, y: i32, shared: SharedApp, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let owner = cx.weak_entity();
        let menu = PopupMenu::build(window, cx, |mut menu, _, _| {
            for (label, action) in [
                (i18n::text("开始截图"), Action::Capture),
                (i18n::text("显示控制面板"), Action::Show),
                (i18n::text("隐藏控制面板"), Action::Hide),
                (i18n::text("退出"), Action::Quit),
            ] {
                let owner = owner.clone();
                menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    owner
                        .update(cx, |this, cx| this.close(Some(action), window, cx))
                        .ok();
                }));
            }
            menu.min_w(px(MENU_WIDTH)).max_w(px(MENU_WIDTH))
        });
        menu.focus_handle(cx).focus(window, cx);
        let dismiss = cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, window, cx| {
            this.close(None, window, cx);
        });
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.activated = true;
            } else if this.activated {
                this.close(None, window, cx);
            }
        });
        let bounds = cx.observe_window_bounds(window, move |this, window, _| {
            if this.closing {
                return;
            }
            let result = native::work_area_for_rect(anchor_rect(x, y)).and_then(|area| {
                let desired = menu_rect(x, y, window.scale_factor(), area);
                if native::bounds(window)? != desired {
                    native::place(window, desired)?;
                }
                Ok(())
            });
            if let Err(error) = result {
                logging::error(format!("Position tray menu: {error:#}"));
            }
        });
        Self {
            menu,
            shared,
            closing: false,
            activated: false,
            _subscriptions: vec![dismiss, activation, bounds],
        }
    }

    fn close(&mut self, action: Option<Action>, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing {
            return;
        }
        self.closing = true;
        let sender = self.shared.borrow().sender.clone();
        // Finish PopupMenu's click/dismiss callback before removing its host.
        window.defer(cx, move |window, cx| {
            let result = native::hide(window).and_then(|_| {
                if matches!(action, Some(Action::Capture)) {
                    native::flush_composition()
                } else {
                    Ok(())
                }
            });
            window.remove_window();
            match result {
                Ok(()) => {
                    if let Some(action) = action {
                        logging::info("application tray menu hidden before command");
                        cx.defer(move |_| {
                            let _ = sender.send(Message::Tray(action));
                        });
                    }
                }
                Err(error) => {
                    logging::error(format!("Close tray menu: {error:#}"));
                    let _ = sender.send(Message::Status(format!(
                        "{}: {error:#}",
                        i18n::text("操作失败")
                    )));
                }
            }
        });
    }
}

impl Render for TrayMenu {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .p(px(PADDING))
            .w(px(HOST_WIDTH))
            .child(self.menu.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{HOST_HEIGHT, HOST_WIDTH, menu_rect};
    use windows::Win32::Foundation::RECT;

    #[test]
    fn menu_fits_work_area_at_tray_edges_and_mixed_scales() {
        for area in [
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1040,
            },
            RECT {
                left: -1920,
                top: -200,
                right: 0,
                bottom: 880,
            },
        ] {
            for scale in [1., 1.25, 1.5, 2.] {
                for (x, y) in [(area.left, area.top), (area.right - 1, area.bottom - 1)] {
                    let rect = menu_rect(x, y, scale, area);
                    assert!(rect.left >= area.left && rect.top >= area.top);
                    assert!(rect.right <= area.right && rect.bottom <= area.bottom);
                    assert_eq!(rect.right - rect.left, (HOST_WIDTH * scale).ceil() as i32);
                    assert_eq!(rect.bottom - rect.top, (HOST_HEIGHT * scale).ceil() as i32);
                }
            }
        }
    }
}
