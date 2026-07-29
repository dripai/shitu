mod windowing;

pub(crate) use windowing::{begin_window_drag, configure_visual_overlay};

#[cfg(target_os = "windows")]
mod windows;

#[cfg(not(target_os = "windows"))]
mod unsupported;

use crate::ports::{DesktopIntegration, RecordingBackend, TargetSelection};

pub(crate) fn desktop_integration() -> &'static dyn DesktopIntegration {
    #[cfg(target_os = "windows")]
    {
        &windows::DESKTOP_INTEGRATION
    }
    #[cfg(not(target_os = "windows"))]
    {
        &unsupported::DESKTOP_INTEGRATION
    }
}

pub(crate) fn recording_backend() -> &'static dyn RecordingBackend {
    #[cfg(target_os = "windows")]
    {
        &windows::RECORDING_BACKEND
    }
    #[cfg(not(target_os = "windows"))]
    {
        &unsupported::RECORDING_BACKEND
    }
}

pub(crate) fn target_selection() -> &'static dyn TargetSelection {
    #[cfg(target_os = "windows")]
    {
        windows::target_selection()
    }
    #[cfg(not(target_os = "windows"))]
    {
        &unsupported::TARGET_SELECTION
    }
}
