use std::path::Path;

use anyhow::Result;

use crate::{
    config::OutputFormat,
    domain::{
        AudioSourceKind, Bounds, MonitorCandidates, RecordingTarget, WindowCandidates, WindowId,
    },
};

pub(crate) trait TargetSelection: Sync {
    fn monitors(&self, owner: Option<&slint::Window>) -> Result<MonitorCandidates>;
    fn windows(&self, desktop: Bounds) -> Result<WindowCandidates>;
    fn primary_screen_bounds(&self) -> Result<Bounds>;
    fn virtual_desktop_bounds(&self) -> Result<Bounds>;
    fn current_bounds(&self, target: RecordingTarget) -> Result<Bounds>;
}

/// 采集和编码对象在录制线程内创建、使用和销毁，因此接口不要求 `Send`。
/// 这保留了 COM、ScreenCaptureKit 等线程亲和型后端的实现空间。
pub(crate) trait VideoCapture {
    fn capture(
        &mut self,
        source: Bounds,
        show_cursor: bool,
        highlight_clicks: bool,
    ) -> Result<&[u8]>;
}

pub(crate) trait AudioCapture {
    fn system_available(&self) -> bool;
    fn microphone_available(&self) -> bool;
    fn error(&self, kind: AudioSourceKind) -> Option<&str>;
    fn has_any_source(&self) -> bool;
    fn pump(&mut self) -> Result<()>;
    fn discard(&mut self);
    fn mix(&mut self, frames: usize, system_enabled: bool, microphone_enabled: bool) -> Vec<i16>;
}

pub(crate) trait MediaWriter {
    fn write_video(&mut self, frame_index: u64, bgra: &[u8]) -> Result<()>;
    fn write_audio(&mut self, start_frame: u64, pcm: &[i16]) -> Result<()>;
    fn finalize(self: Box<Self>) -> Result<()>;
}

pub(crate) trait RecordingThreadRuntime {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RecordingCapabilities {
    pub(crate) system_audio: bool,
    pub(crate) microphone: bool,
    pub(crate) highlight_clicks: bool,
}

pub(crate) trait RecordingBackend: Sync {
    fn capabilities(&self) -> RecordingCapabilities;
    fn initialize_thread(&self) -> Result<Box<dyn RecordingThreadRuntime>>;
    fn create_video_capture(
        &self,
        target: RecordingTarget,
        width: u32,
        height: u32,
        show_cursor: bool,
    ) -> Result<Box<dyn VideoCapture>>;
    fn create_audio_capture(
        &self,
        target: RecordingTarget,
        system_enabled: bool,
        microphone_enabled: bool,
    ) -> Box<dyn AudioCapture>;
    fn create_writer(
        &self,
        format: OutputFormat,
        path: &Path,
        width: u32,
        height: u32,
        frames_per_second: u32,
        include_audio: bool,
    ) -> Result<Box<dyn MediaWriter>>;
    fn audio_sample_rate(&self) -> u32;
}

pub(crate) trait DesktopIntegration: Sync {
    fn replace_file(&self, source: &Path, target: &Path) -> Result<()>;
    fn local_timestamp(&self) -> String;
    fn open_path(&self, path: &Path) -> Result<()>;
    fn native_window_id(&self, window: &slint::Window) -> Option<WindowId>;
    fn activate_window(&self, window: &slint::Window);
}
