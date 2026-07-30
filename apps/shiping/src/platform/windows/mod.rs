use std::{os::windows::ffi::OsStrExt, path::Path};

use anyhow::{Context, Result, anyhow};
use shi_foundation::i18n;

use crate::{
    config::OutputFormat,
    domain::WindowId,
    output::GifWriter,
    ports::{
        AudioCapture, DesktopIntegration, MediaWriter, RecordingBackend, RecordingCapabilities,
        RecordingThreadRuntime, TargetSelection, VideoCapture,
    },
};

mod audio;
mod capture;
mod encoder;
mod shell;
mod target;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::{
    Win32::{
        Foundation::HWND,
        Storage::FileSystem::{
            MOVE_FILE_FLAGS, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize},
        System::SystemInformation::GetLocalTime,
        UI::WindowsAndMessaging::SetForegroundWindow,
    },
    core::PCWSTR,
};

pub(super) static DESKTOP_INTEGRATION: WindowsDesktopIntegration = WindowsDesktopIntegration;
pub(super) static RECORDING_BACKEND: WindowsRecordingBackend = WindowsRecordingBackend;

pub(super) fn target_selection() -> &'static dyn TargetSelection {
    &target::WINDOWS_TARGET_SELECTION
}

pub(super) struct WindowsDesktopIntegration;
pub(super) struct WindowsRecordingBackend;

struct ComRuntime;

impl ComRuntime {
    pub(crate) fn initialize() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .context(i18n::text("初始化 COM 失败", "Failed to initialize COM"))?;
        Ok(Self)
    }
}

impl Drop for ComRuntime {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

impl RecordingThreadRuntime for ComRuntime {}

impl RecordingBackend for WindowsRecordingBackend {
    fn capabilities(&self) -> RecordingCapabilities {
        RecordingCapabilities {
            system_audio: true,
            microphone: true,
            highlight_clicks: true,
        }
    }

    fn initialize_thread(&self) -> Result<Box<dyn RecordingThreadRuntime>> {
        Ok(Box::new(ComRuntime::initialize()?))
    }

    fn create_video_capture(
        &self,
        _target: crate::domain::RecordingTarget,
        width: u32,
        height: u32,
        _show_cursor: bool,
    ) -> Result<Box<dyn VideoCapture>> {
        Ok(Box::new(capture::FrameGrabber::new(width, height)?))
    }

    fn create_audio_capture(
        &self,
        _target: crate::domain::RecordingTarget,
        _system_enabled: bool,
        _microphone_enabled: bool,
    ) -> Box<dyn AudioCapture> {
        Box::new(audio::AudioSources::initialize())
    }

    fn create_writer(
        &self,
        format: OutputFormat,
        path: &Path,
        width: u32,
        height: u32,
        frames_per_second: u32,
        include_audio: bool,
    ) -> Result<Box<dyn MediaWriter>> {
        match format {
            OutputFormat::Mp4 => Ok(Box::new(encoder::MediaFoundationWriter::create(
                path,
                width,
                height,
                frames_per_second,
                include_audio,
            )?)),
            OutputFormat::Gif => {
                if include_audio {
                    return Err(anyhow!(i18n::text(
                        "GIF 格式不支持音频",
                        "GIF output does not support audio"
                    )));
                }
                Ok(Box::new(GifWriter::create(
                    path,
                    width,
                    height,
                    frames_per_second,
                )?))
            }
        }
    }

    fn audio_sample_rate(&self) -> u32 {
        encoder::AUDIO_SAMPLE_RATE
    }
}

impl DesktopIntegration for WindowsDesktopIntegration {
    fn replace_file(&self, source: &Path, target: &Path) -> Result<()> {
        replace_file(source, target)
    }

    fn local_timestamp(&self) -> String {
        local_timestamp()
    }

    fn open_path(&self, path: &Path) -> Result<()> {
        shell::open_path(path)
    }

    fn native_window_id(&self, window: &slint::Window) -> Option<WindowId> {
        native_window_handle(window).map(|handle| target::window_id(HWND(handle as *mut _)))
    }

    fn activate_window(&self, window: &slint::Window) {
        activate_window(window);
    }
}

fn replace_file(source: &Path, target: &Path) -> Result<()> {
    let source_wide: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let target_wide: Vec<u16> = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let flags = MOVE_FILE_FLAGS(MOVEFILE_REPLACE_EXISTING.0 | MOVEFILE_WRITE_THROUGH.0);
    unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(target_wide.as_ptr()),
            flags,
        )
    }
    .with_context(|| {
        format!(
            "{}: {}",
            i18n::text("替换配置文件失败", "Failed to replace the settings file"),
            target.display()
        )
    })
}

fn local_timestamp() -> String {
    let value = unsafe { GetLocalTime() };
    format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}",
        value.wYear, value.wMonth, value.wDay, value.wHour, value.wMinute, value.wSecond
    )
}

fn native_window_handle(window: &slint::Window) -> Option<isize> {
    let handle = window.window_handle();
    let handle = handle.window_handle().ok()?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return None;
    };
    Some(handle.hwnd.get())
}

fn activate_window(window: &slint::Window) {
    if let Some(hwnd) = native_window_handle(window) {
        unsafe {
            let _ = SetForegroundWindow(HWND(hwnd as *mut _));
        }
    }
}
