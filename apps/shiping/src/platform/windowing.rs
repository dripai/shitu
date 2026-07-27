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

/// Makes a visual-only overlay ignore cursor input while keeping it above other windows.
/// Winit owns the platform-specific hit-test implementation; Windows-only taskbar and
/// undecorated-shadow flags stay isolated in this adapter.
pub(crate) async fn configure_visual_overlay(window: &slint::Window) -> Result<()> {
    let window = window.winit_window().await.context(i18n::text(
        "无法获取选区边框的 Winit 窗口",
        "Could not obtain the Winit window for the target indicator",
    ))?;
    window.set_cursor_hittest(false).context(i18n::text(
        "无法让选区边框穿透鼠标",
        "Could not make the target indicator ignore pointer input",
    ))?;

    #[cfg(target_os = "windows")]
    {
        use slint::winit_030::winit::platform::windows::WindowExtWindows;

        window.set_skip_taskbar(true);
        window.set_undecorated_shadow(false);
    }
    Ok(())
}
