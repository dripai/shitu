use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use shi_foundation::i18n;

use crate::{
    config::OutputFormat,
    domain::{
        AudioSourceKind, Bounds, MonitorCandidates, RecordingTarget, WindowCandidates, WindowId,
    },
    output::GifWriter,
    ports::{
        AudioCapture, DesktopIntegration, MediaWriter, RecordingBackend, RecordingCapabilities,
        RecordingThreadRuntime, TargetSelection, VideoCapture,
    },
};

pub(super) static DESKTOP_INTEGRATION: UnsupportedDesktopIntegration =
    UnsupportedDesktopIntegration;
pub(super) static RECORDING_BACKEND: UnsupportedRecordingBackend = UnsupportedRecordingBackend;
pub(super) static TARGET_SELECTION: UnsupportedTargetSelection = UnsupportedTargetSelection;

pub(super) struct UnsupportedDesktopIntegration;
pub(super) struct UnsupportedRecordingBackend;
pub(super) struct UnsupportedTargetSelection;

struct UnsupportedRuntime;
struct UnavailableAudioCapture;

impl RecordingThreadRuntime for UnsupportedRuntime {}

impl TargetSelection for UnsupportedTargetSelection {
    fn monitors(&self, _owner: Option<&slint::Window>) -> Result<MonitorCandidates> {
        Err(not_implemented("display selection"))
    }

    fn windows(&self, _desktop: Bounds) -> Result<WindowCandidates> {
        Err(not_implemented("window selection"))
    }

    fn primary_screen_bounds(&self) -> Result<Bounds> {
        Err(not_implemented("screen bounds"))
    }

    fn virtual_desktop_bounds(&self) -> Result<Bounds> {
        Err(not_implemented("virtual desktop bounds"))
    }

    fn current_bounds(&self, target: RecordingTarget) -> Result<Bounds> {
        match target {
            RecordingTarget::Screen(bounds) | RecordingTarget::Region(bounds) => bounds.validate(),
            RecordingTarget::Window { .. } => Err(not_implemented("window tracking")),
        }
    }
}

impl RecordingBackend for UnsupportedRecordingBackend {
    fn capabilities(&self) -> RecordingCapabilities {
        RecordingCapabilities {
            system_audio: false,
            microphone: false,
            highlight_clicks: false,
        }
    }

    fn initialize_thread(&self) -> Result<Box<dyn RecordingThreadRuntime>> {
        Ok(Box::new(UnsupportedRuntime))
    }

    fn create_video_capture(
        &self,
        _target: RecordingTarget,
        _width: u32,
        _height: u32,
        _show_cursor: bool,
    ) -> Result<Box<dyn VideoCapture>> {
        Err(not_implemented("video capture"))
    }

    fn create_audio_capture(
        &self,
        _target: RecordingTarget,
        _system_enabled: bool,
        _microphone_enabled: bool,
    ) -> Box<dyn AudioCapture> {
        Box::new(UnavailableAudioCapture)
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
            OutputFormat::Gif if !include_audio => Ok(Box::new(GifWriter::create(
                path,
                width,
                height,
                frames_per_second,
            )?)),
            OutputFormat::Gif => Err(anyhow!(i18n::text(
                "GIF 格式不支持音频",
                "GIF output does not support audio"
            ))),
            OutputFormat::Mp4 => Err(not_implemented("MP4 encoding")),
        }
    }

    fn audio_sample_rate(&self) -> u32 {
        48_000
    }
}

impl AudioCapture for UnavailableAudioCapture {
    fn system_available(&self) -> bool {
        false
    }

    fn microphone_available(&self) -> bool {
        false
    }

    fn error(&self, _kind: AudioSourceKind) -> Option<&str> {
        Some(i18n::text(
            "当前平台尚未实现音频采集",
            "Audio capture is not implemented on this platform",
        ))
    }

    fn has_any_source(&self) -> bool {
        false
    }

    fn pump(&mut self) -> Result<()> {
        Ok(())
    }

    fn discard(&mut self) {}

    fn mix(&mut self, frames: usize, _system_enabled: bool, _microphone_enabled: bool) -> Vec<i16> {
        vec![0; frames * 2]
    }
}

impl DesktopIntegration for UnsupportedDesktopIntegration {
    fn replace_file(&self, source: &Path, target: &Path) -> Result<()> {
        fs::rename(source, target).with_context(|| {
            format!(
                "{}: {} -> {}",
                i18n::text("替换配置文件失败", "Failed to replace the settings file"),
                source.display(),
                target.display()
            )
        })
    }

    fn local_timestamp(&self) -> String {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string()
    }

    fn open_path(&self, _path: &Path) -> Result<()> {
        Err(not_implemented("opening paths"))
    }

    fn native_window_id(&self, _window: &slint::Window) -> Option<WindowId> {
        None
    }

    fn activate_window(&self, _window: &slint::Window) {}
}

fn not_implemented(capability: &str) -> anyhow::Error {
    anyhow!(
        "{}: {capability}",
        i18n::text(
            "当前平台后端尚未实现此能力",
            "The current platform backend does not implement this capability"
        )
    )
}
