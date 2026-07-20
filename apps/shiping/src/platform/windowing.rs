use anyhow::{Context, Result, anyhow};
use shi_foundation::i18n;
use slint::winit_030::WinitWindowAccessor;

/// Starts the window manager's native move operation through Slint's Winit backend.
pub(crate) fn begin_window_drag(window: &slint::Window) -> Result<()> {
    window
        .with_winit_window(|window| window.drag_window())
        .ok_or_else(|| {
            anyhow!(i18n::text(
                "当前 Slint 窗口没有可用的 Winit 后端",
                "The current Slint window has no available Winit backend"
            ))
        })?
        .context(i18n::text(
            "无法开始拖动窗口",
            "Could not start dragging the window",
        ))
}
