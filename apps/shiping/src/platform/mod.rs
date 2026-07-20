mod windowing;

pub(crate) use windowing::{begin_window_drag, configure_visual_overlay};

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub(crate) use windows::{
    ComRuntime, audio, capture, encoder, local_timestamp, native_window_handle, replace_file,
    shell, target,
};
