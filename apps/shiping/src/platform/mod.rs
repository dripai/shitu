#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub(crate) use windows::{
    ComRuntime, audio, begin_window_drag, capture, encoder, local_timestamp, native_window_handle,
    replace_file, shell, show_native_choice_menu, target,
};
