use std::{
    collections::VecDeque,
    fs,
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use screencapturekit::{
    cm::CMSampleBufferExt,
    screenshot_manager::{CGImageExt, SCScreenshotManager},
    shareable_content::{SCDisplay, SCShareableContent},
    stream::{
        SCStream,
        configuration::{SCStreamConfiguration, pixel_format::PixelFormat},
        content_filter::SCContentFilter,
        output_type::SCStreamOutputType,
    },
};
use shi_foundation::i18n;

use crate::{
    config::OutputFormat,
    domain::{
        AudioSourceKind, Bounds, MonitorCandidate, MonitorCandidates, RecordingTarget,
        WindowCandidate, WindowCandidates, WindowId,
    },
    output::GifWriter,
    ports::{
        AudioCapture, DesktopIntegration, MediaWriter, RecordingBackend, RecordingCapabilities,
        RecordingThreadRuntime, TargetSelection, VideoCapture,
    },
};

use super::ffmpeg::FfmpegWriter;

pub(super) static DESKTOP_INTEGRATION: MacOsDesktopIntegration = MacOsDesktopIntegration;
pub(super) static RECORDING_BACKEND: MacOsRecordingBackend = MacOsRecordingBackend;
pub(super) static TARGET_SELECTION: MacOsTargetSelection = MacOsTargetSelection;

pub(super) struct MacOsDesktopIntegration;
pub(super) struct MacOsRecordingBackend;
pub(super) struct MacOsTargetSelection;

struct MacOsRuntime;

impl RecordingThreadRuntime for MacOsRuntime {}

impl TargetSelection for MacOsTargetSelection {
    fn monitors(&self, _owner: Option<&slint::Window>) -> Result<MonitorCandidates> {
        let content = shareable_content()?;
        let monitors = content
            .displays()
            .iter()
            .map(|display| {
                let bounds = display_bounds(display);
                MonitorCandidate {
                    bounds,
                    primary: bounds.contains(0, 0),
                }
            })
            .collect();
        Ok(MonitorCandidates::new(monitors))
    }

    fn windows(&self, _desktop: Bounds) -> Result<WindowCandidates> {
        let content = shareable_content()?;
        let windows = content
            .windows()
            .into_iter()
            .filter(|window| window.is_on_screen() && window.window_layer() == 0)
            .filter_map(|window| {
                let title = window.title()?.trim().to_owned();
                if title.is_empty() {
                    return None;
                }
                Some(WindowCandidate {
                    id: WindowId::from_platform_value(window.window_id() as u64),
                    bounds: cg_bounds(window.frame()),
                    title,
                })
            })
            .collect();
        Ok(WindowCandidates::new(windows))
    }

    fn primary_screen_bounds(&self) -> Result<Bounds> {
        self.monitors(None)?
            .get(0)
            .map(|monitor| monitor.bounds)
            .ok_or_else(|| anyhow!(i18n::text("未找到显示器", "No display was found")))
    }

    fn virtual_desktop_bounds(&self) -> Result<Bounds> {
        union_bounds(
            self.monitors(None)?
                .into_values()
                .map(|monitor| monitor.bounds),
        )
    }

    fn current_bounds(&self, target: RecordingTarget) -> Result<Bounds> {
        match target {
            RecordingTarget::Screen(bounds) | RecordingTarget::Region(bounds) => bounds.validate(),
            RecordingTarget::Window { id, .. } => shareable_content()?
                .windows()
                .into_iter()
                .find(|window| window.window_id() as u64 == id.platform_value())
                .map(|window| cg_bounds(window.frame()))
                .ok_or_else(|| {
                    anyhow!(i18n::text(
                        "所选窗口已关闭或不可共享",
                        "The selected window is closed or no longer shareable"
                    ))
                })?
                .validate(),
        }
    }
}

impl RecordingBackend for MacOsRecordingBackend {
    fn capabilities(&self) -> RecordingCapabilities {
        RecordingCapabilities {
            system_audio: true,
            microphone: true,
            highlight_clicks: false,
        }
    }

    fn initialize_thread(&self) -> Result<Box<dyn RecordingThreadRuntime>> {
        Ok(Box::new(MacOsRuntime))
    }

    fn create_video_capture(
        &self,
        target: RecordingTarget,
        width: u32,
        height: u32,
        _show_cursor: bool,
    ) -> Result<Box<dyn VideoCapture>> {
        Ok(Box::new(ScreenCaptureKitGrabber::new(
            target, width, height,
        )?))
    }

    fn create_audio_capture(
        &self,
        target: RecordingTarget,
        system_enabled: bool,
        microphone_enabled: bool,
    ) -> Box<dyn AudioCapture> {
        match ScreenCaptureKitAudio::new(target, system_enabled, microphone_enabled) {
            Ok(capture) => Box::new(capture),
            Err(error) => Box::new(UnavailableMacOsAudio::new(error.to_string())),
        }
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
            OutputFormat::Mp4 => Ok(Box::new(FfmpegWriter::create(
                path,
                width,
                height,
                frames_per_second,
                include_audio,
            )?)),
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
        }
    }

    fn audio_sample_rate(&self) -> u32 {
        48_000
    }
}

struct ScreenCaptureKitGrabber {
    filter: SCContentFilter,
    configuration: SCStreamConfiguration,
    frame: Vec<u8>,
}

impl ScreenCaptureKitGrabber {
    fn new(target: RecordingTarget, width: u32, height: u32) -> Result<Self> {
        let filter = content_filter(target)?;
        let mut configuration = SCStreamConfiguration::new()
            .with_width(width)
            .with_height(height)
            .with_pixel_format(PixelFormat::BGRA);
        if let RecordingTarget::Region(bounds) = target {
            let display = shareable_content()?
                .displays()
                .into_iter()
                .find(|display| display_bounds(display).contains(bounds.left, bounds.top))
                .ok_or_else(|| {
                    anyhow!(i18n::text(
                        "未找到区域所在的显示器",
                        "The display containing the selected region was not found"
                    ))
                })?;
            let display = display_bounds(&display);
            configuration.set_source_rect(screencapturekit::cg::CGRect::new(
                bounds.left.saturating_sub(display.left) as f64,
                bounds.top.saturating_sub(display.top) as f64,
                bounds.width as f64,
                bounds.height as f64,
            ));
        }
        Ok(Self {
            filter,
            configuration,
            frame: vec![0; width as usize * height as usize * 4],
        })
    }
}

impl VideoCapture for ScreenCaptureKitGrabber {
    fn capture(
        &mut self,
        _source: Bounds,
        show_cursor: bool,
        _highlight_clicks: bool,
    ) -> Result<&[u8]> {
        self.configuration.set_shows_cursor(show_cursor);
        let image = SCScreenshotManager::capture_image(&self.filter, &self.configuration).context(
            i18n::text(
                "ScreenCaptureKit 获取视频帧失败",
                "ScreenCaptureKit failed to capture a video frame",
            ),
        )?;
        image.bgra_data_into(&mut self.frame).context(i18n::text(
            "读取 ScreenCaptureKit BGRA 像素失败",
            "Failed to read ScreenCaptureKit BGRA pixels",
        ))?;
        Ok(&self.frame)
    }
}

struct ScreenCaptureKitAudio {
    stream: SCStream,
    system: Arc<Mutex<VecDeque<[f32; 2]>>>,
    microphone: Arc<Mutex<VecDeque<[f32; 2]>>>,
    system_available: bool,
    microphone_available: bool,
}

impl ScreenCaptureKitAudio {
    fn new(
        target: RecordingTarget,
        system_enabled: bool,
        microphone_enabled: bool,
    ) -> Result<Self> {
        let filter = content_filter(target)?;
        let configuration = SCStreamConfiguration::new()
            .with_captures_audio(system_enabled)
            .with_captures_microphone(microphone_enabled)
            .with_excludes_current_process_audio(true)
            .with_sample_rate(48_000)
            .with_channel_count(2);
        let system = Arc::new(Mutex::new(VecDeque::new()));
        let microphone = Arc::new(Mutex::new(VecDeque::new()));
        let mut stream = SCStream::new(&filter, &configuration);

        let system_output = Arc::clone(&system);
        let system_handler = system_enabled.then(|| {
            stream.add_output_handler(
                move |sample, _| append_audio_sample(&system_output, &sample),
                SCStreamOutputType::Audio,
            )
        });
        let microphone_output = Arc::clone(&microphone);
        let microphone_handler = microphone_enabled.then(|| {
            stream.add_output_handler(
                move |sample, _| append_audio_sample(&microphone_output, &sample),
                SCStreamOutputType::Microphone,
            )
        });
        if system_handler.is_some_and(|handler| handler.is_none())
            || microphone_handler.is_some_and(|handler| handler.is_none())
        {
            return Err(anyhow!(i18n::text(
                "ScreenCaptureKit 拒绝注册音频输出",
                "ScreenCaptureKit rejected an audio output handler"
            )));
        }
        stream.start_capture().context(i18n::text(
            "启动 ScreenCaptureKit 音频流失败",
            "Failed to start the ScreenCaptureKit audio stream",
        ))?;
        Ok(Self {
            stream,
            system,
            microphone,
            system_available: system_enabled,
            microphone_available: microphone_enabled,
        })
    }
}

impl Drop for ScreenCaptureKitAudio {
    fn drop(&mut self) {
        let _ = self.stream.stop_capture();
    }
}

impl AudioCapture for ScreenCaptureKitAudio {
    fn system_available(&self) -> bool {
        self.system_available
    }

    fn microphone_available(&self) -> bool {
        self.microphone_available
    }

    fn error(&self, _kind: AudioSourceKind) -> Option<&str> {
        None
    }

    fn has_any_source(&self) -> bool {
        self.system_available || self.microphone_available
    }

    fn pump(&mut self) -> Result<()> {
        Ok(())
    }

    fn discard(&mut self) {
        self.system
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        self.microphone
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    fn mix(&mut self, frames: usize, system_enabled: bool, microphone_enabled: bool) -> Vec<i16> {
        let mut system = self
            .system
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut microphone = self
            .microphone
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut output = Vec::with_capacity(frames * 2);
        for _ in 0..frames {
            let system_frame = system.pop_front().unwrap_or([0.0; 2]);
            let microphone_frame = microphone.pop_front().unwrap_or([0.0; 2]);
            for channel in 0..2 {
                let mut value = 0.0_f32;
                if system_enabled {
                    value += system_frame[channel];
                }
                if microphone_enabled {
                    value += microphone_frame[channel];
                }
                output.push((value.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16);
            }
        }
        output
    }
}

struct UnavailableMacOsAudio {
    error: String,
}

impl UnavailableMacOsAudio {
    fn new(error: String) -> Self {
        Self { error }
    }
}

impl AudioCapture for UnavailableMacOsAudio {
    fn system_available(&self) -> bool {
        false
    }

    fn microphone_available(&self) -> bool {
        false
    }

    fn error(&self, _kind: AudioSourceKind) -> Option<&str> {
        Some(&self.error)
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

impl DesktopIntegration for MacOsDesktopIntegration {
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
        unix_timestamp()
    }

    fn open_path(&self, path: &Path) -> Result<()> {
        let status = Command::new("open")
            .arg(path)
            .status()
            .context(i18n::text("启动 open 失败", "Failed to launch open"))?;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow!(
                "{}: {status}",
                i18n::text("打开路径失败", "Failed to open the path")
            ))
        }
    }

    fn native_window_id(&self, _window: &slint::Window) -> Option<WindowId> {
        None
    }

    fn activate_window(&self, window: &slint::Window) {
        let _ = window.show();
        window.request_redraw();
    }
}

fn shareable_content() -> Result<SCShareableContent> {
    SCShareableContent::create()
        .with_on_screen_windows_only(true)
        .with_exclude_desktop_windows(true)
        .get()
        .context(i18n::text(
            "读取 ScreenCaptureKit 可共享内容失败；请检查屏幕录制权限",
            "Failed to read ScreenCaptureKit shareable content; check Screen Recording permission",
        ))
}

fn content_filter(target: RecordingTarget) -> Result<SCContentFilter> {
    let content = shareable_content()?;
    match target {
        RecordingTarget::Window { id, .. } => content
            .windows()
            .into_iter()
            .find(|window| window.window_id() as u64 == id.platform_value())
            .map(|window| SCContentFilter::create().with_window(&window).build())
            .ok_or_else(|| {
                anyhow!(i18n::text(
                    "所选窗口已关闭或不可共享",
                    "The selected window is closed or no longer shareable"
                ))
            }),
        RecordingTarget::Screen(bounds) | RecordingTarget::Region(bounds) => content
            .displays()
            .into_iter()
            .find(|display| display_bounds(display).contains(bounds.left, bounds.top))
            .map(|display| {
                SCContentFilter::create()
                    .with_display(&display)
                    .with_excluding_windows(&[])
                    .build()
            })
            .ok_or_else(|| {
                anyhow!(i18n::text(
                    "未找到目标显示器",
                    "Target display was not found"
                ))
            }),
    }
}

fn append_audio_sample(
    queue: &Arc<Mutex<VecDeque<[f32; 2]>>>,
    sample: &screencapturekit::cm::CMSampleBuffer,
) {
    let Some(format) = sample.format_description() else {
        return;
    };
    if !format.audio_is_float()
        || format.audio_is_big_endian()
        || format.audio_bits_per_channel() != Some(32)
    {
        return;
    }
    let Some(list) = sample.audio_buffer_list() else {
        return;
    };
    let mut frames = Vec::new();
    if list.num_buffers() == 1 {
        if let Some(buffer) = list.get(0) {
            let channels = format.audio_channel_count().unwrap_or(1).max(1) as usize;
            let values = buffer
                .data()
                .chunks_exact(4)
                .map(|bytes| f32::from_ne_bytes(bytes.try_into().unwrap()))
                .collect::<Vec<_>>();
            for frame in values.chunks(channels) {
                let left = frame.first().copied().unwrap_or_default();
                let right = frame.get(1).copied().unwrap_or(left);
                frames.push([left, right]);
            }
        }
    } else if let (Some(left), Some(right)) = (list.get(0), list.get(1)) {
        let left = left.data().chunks_exact(4);
        let right = right.data().chunks_exact(4);
        for (left, right) in left.zip(right) {
            frames.push([
                f32::from_ne_bytes(left.try_into().unwrap()),
                f32::from_ne_bytes(right.try_into().unwrap()),
            ]);
        }
    }
    queue
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .extend(frames);
}

fn display_bounds(display: &SCDisplay) -> Bounds {
    let frame = display.frame();
    Bounds {
        left: frame.origin.x.round() as i32,
        top: frame.origin.y.round() as i32,
        width: display.width() as i32,
        height: display.height() as i32,
    }
}

fn cg_bounds(rect: screencapturekit::cg::CGRect) -> Bounds {
    Bounds {
        left: rect.origin.x.round() as i32,
        top: rect.origin.y.round() as i32,
        width: rect.size.width.round() as i32,
        height: rect.size.height.round() as i32,
    }
}

fn union_bounds(values: impl Iterator<Item = Bounds>) -> Result<Bounds> {
    let mut values = values.peekable();
    let first = values
        .next()
        .ok_or_else(|| anyhow!(i18n::text("未找到显示器", "No display was found")))?;
    let (left, top, right, bottom) = values.fold(
        (
            first.left,
            first.top,
            first.left.saturating_add(first.width),
            first.top.saturating_add(first.height),
        ),
        |(left, top, right, bottom), bounds| {
            (
                left.min(bounds.left),
                top.min(bounds.top),
                right.max(bounds.left.saturating_add(bounds.width)),
                bottom.max(bounds.top.saturating_add(bounds.height)),
            )
        },
    );
    Bounds {
        left,
        top,
        width: right.saturating_sub(left),
        height: bottom.saturating_sub(top),
    }
    .validate()
}

fn unix_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{Bounds, union_bounds};

    #[test]
    fn virtual_desktop_union_keeps_negative_display_coordinates() {
        let bounds = union_bounds(
            [
                Bounds {
                    left: -1920,
                    top: 0,
                    width: 1920,
                    height: 1080,
                },
                Bounds {
                    left: 0,
                    top: 0,
                    width: 2560,
                    height: 1440,
                },
            ]
            .into_iter(),
        )
        .unwrap();
        assert_eq!(
            bounds,
            Bounds {
                left: -1920,
                top: 0,
                width: 4480,
                height: 1440,
            }
        );
    }
}
