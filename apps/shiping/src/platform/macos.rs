use std::{
    collections::VecDeque,
    fs,
    path::Path,
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use screencapturekit::{
    cm::CMSampleBufferExt,
    screenshot_manager::{CGImageExt, SCScreenshotManager},
    shareable_content::SCShareableContent,
    stream::{
        SCStream,
        configuration::{SCStreamConfiguration, pixel_format::PixelFormat},
        content_filter::SCContentFilter,
        output_type::SCStreamOutputType,
    },
};
use shi_foundation::i18n;
use slint::winit_030::{WinitWindowAccessor, winit::platform::macos::MonitorHandleExtMacOS};

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

use super::macos_writer::MacOsMp4Writer;

pub(super) static DESKTOP_INTEGRATION: MacOsDesktopIntegration = MacOsDesktopIntegration;
pub(super) static RECORDING_BACKEND: MacOsRecordingBackend = MacOsRecordingBackend;
pub(super) static TARGET_SELECTION: MacOsTargetSelection = MacOsTargetSelection;

static DISPLAYS: OnceLock<Mutex<Vec<DisplayMapping>>> = OnceLock::new();

pub(super) struct MacOsDesktopIntegration;
pub(super) struct MacOsRecordingBackend;
pub(super) struct MacOsTargetSelection;

#[derive(Clone, Copy, Debug)]
struct PointBounds {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

#[derive(Clone, Copy, Debug)]
struct DisplayMapping {
    display_id: u32,
    points: PointBounds,
    pixels: Bounds,
    primary: bool,
}

struct MacOsRuntime;

impl RecordingThreadRuntime for MacOsRuntime {}

impl TargetSelection for MacOsTargetSelection {
    fn monitors(&self, owner: Option<&slint::Window>) -> Result<MonitorCandidates> {
        if let Some(owner) = owner {
            refresh_display_mappings(owner)?;
        }
        Ok(MonitorCandidates::new(
            display_mappings()?
                .into_iter()
                .map(|display| MonitorCandidate {
                    bounds: display.pixels,
                    primary: display.primary,
                })
                .collect(),
        ))
    }

    fn windows(&self, _desktop: Bounds) -> Result<WindowCandidates> {
        let content = shareable_content()?;
        let displays = display_mappings()?;
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
                    bounds: point_rect_to_physical(point_bounds(window.frame()), &displays).ok()?,
                    title,
                })
            })
            .collect();
        Ok(WindowCandidates::new(windows))
    }

    fn primary_screen_bounds(&self) -> Result<Bounds> {
        display_mappings()?
            .into_iter()
            .find(|display| display.primary)
            .map(|display| display.pixels)
            .ok_or_else(|| anyhow!(i18n::text("未找到显示器", "No display was found")))
    }

    fn virtual_desktop_bounds(&self) -> Result<Bounds> {
        union_bounds(
            display_mappings()?
                .into_iter()
                .map(|display| display.pixels),
        )
    }

    fn current_bounds(&self, target: RecordingTarget) -> Result<Bounds> {
        match target {
            RecordingTarget::Screen(bounds) | RecordingTarget::Region(bounds) => bounds.validate(),
            RecordingTarget::Window { id, .. } => {
                let displays = display_mappings()?;
                shareable_content()?
                    .windows()
                    .into_iter()
                    .find(|window| window.window_id() as u64 == id.platform_value())
                    .map(|window| point_rect_to_physical(point_bounds(window.frame()), &displays))
                    .ok_or_else(|| {
                        anyhow!(i18n::text(
                            "所选窗口已关闭或不可共享",
                            "The selected window is closed or no longer shareable"
                        ))
                    })??
                    .validate()
            }
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
            OutputFormat::Mp4 => Ok(Box::new(MacOsMp4Writer::create(
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
            let display = display_for_physical_bounds(bounds)?;
            configuration.set_source_rect(source_rect_in_points(bounds, display)?);
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
        RecordingTarget::Screen(bounds) | RecordingTarget::Region(bounds) => {
            let mapping = display_for_physical_bounds(bounds)?;
            content
                .displays()
                .into_iter()
                .find(|display| mapping.display_id == display.display_id())
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
                })
        }
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

fn display_cache() -> &'static Mutex<Vec<DisplayMapping>> {
    DISPLAYS.get_or_init(|| Mutex::new(Vec::new()))
}

fn display_mappings() -> Result<Vec<DisplayMapping>> {
    let displays = display_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if displays.is_empty() {
        Err(anyhow!(i18n::text(
            "显示器信息尚未初始化",
            "Display information has not been initialized"
        )))
    } else {
        Ok(displays)
    }
}

fn refresh_display_mappings(owner: &slint::Window) -> Result<()> {
    let physical_displays = owner
        .with_winit_window(|window| {
            let primary = window.primary_monitor();
            window
                .available_monitors()
                .map(|monitor| {
                    let position = monitor.position();
                    let size = monitor.size();
                    (
                        monitor.native_id(),
                        Bounds {
                            left: position.x,
                            top: position.y,
                            width: size.width as i32,
                            height: size.height as i32,
                        },
                        primary.as_ref() == Some(&monitor),
                    )
                })
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| {
            anyhow!(i18n::text(
                "Slint Winit 窗口尚未创建，无法读取显示器",
                "The Slint Winit window is not ready, so displays cannot be enumerated"
            ))
        })?;
    let content = shareable_content()?;
    let shareable_displays = content.displays();
    let mappings = physical_displays
        .into_iter()
        .map(|(display_id, pixels, primary)| {
            let display = shareable_displays
                .iter()
                .find(|display| display.display_id() == display_id)
                .ok_or_else(|| {
                    anyhow!(
                        "{}: {display_id}",
                        i18n::text(
                            "ScreenCaptureKit 未返回 Winit 显示器",
                            "ScreenCaptureKit did not return the Winit display"
                        )
                    )
                })?;
            Ok(DisplayMapping {
                display_id,
                points: point_bounds(display.frame()),
                pixels,
                primary,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if mappings.is_empty() {
        return Err(anyhow!(i18n::text("未找到显示器", "No display was found")));
    }
    *display_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = mappings;
    Ok(())
}

fn point_bounds(rect: screencapturekit::cg::CGRect) -> PointBounds {
    PointBounds {
        left: rect.origin.x,
        top: rect.origin.y,
        width: rect.size.width,
        height: rect.size.height,
    }
}

fn point_rect_to_physical(rect: PointBounds, displays: &[DisplayMapping]) -> Result<Bounds> {
    let display = displays
        .iter()
        .max_by(|left, right| {
            point_intersection_area(rect, left.points)
                .total_cmp(&point_intersection_area(rect, right.points))
        })
        .filter(|display| point_intersection_area(rect, display.points) > 0.0)
        .ok_or_else(|| {
            anyhow!(i18n::text(
                "窗口不在任何已知显示器中",
                "The window is not on a known display"
            ))
        })?;
    let scale_x = display.pixels.width as f64 / display.points.width;
    let scale_y = display.pixels.height as f64 / display.points.height;
    Bounds {
        left: display.pixels.left + ((rect.left - display.points.left) * scale_x).round() as i32,
        top: display.pixels.top + ((rect.top - display.points.top) * scale_y).round() as i32,
        width: (rect.width * scale_x).round() as i32,
        height: (rect.height * scale_y).round() as i32,
    }
    .validate()
}

fn point_intersection_area(left: PointBounds, right: PointBounds) -> f64 {
    let width = (left.left + left.width).min(right.left + right.width) - left.left.max(right.left);
    let height = (left.top + left.height).min(right.top + right.height) - left.top.max(right.top);
    width.max(0.0) * height.max(0.0)
}

fn display_for_physical_bounds(bounds: Bounds) -> Result<DisplayMapping> {
    display_mappings()?
        .into_iter()
        .find(|display| contains_bounds(display.pixels, bounds))
        .ok_or_else(|| {
            anyhow!(i18n::text(
                "所选区域必须完全位于一个显示器内",
                "The selected region must be entirely within one display"
            ))
        })
}

fn contains_bounds(container: Bounds, value: Bounds) -> bool {
    value.left >= container.left
        && value.top >= container.top
        && value.left.saturating_add(value.width) <= container.left.saturating_add(container.width)
        && value.top.saturating_add(value.height) <= container.top.saturating_add(container.height)
}

fn source_rect_in_points(
    bounds: Bounds,
    display: DisplayMapping,
) -> Result<screencapturekit::cg::CGRect> {
    let scale_x = display.pixels.width as f64 / display.points.width;
    let scale_y = display.pixels.height as f64 / display.points.height;
    if scale_x <= 0.0 || scale_y <= 0.0 {
        return Err(anyhow!(i18n::text(
            "显示器缩放比例无效",
            "The display scale factor is invalid"
        )));
    }
    Ok(screencapturekit::cg::CGRect::new(
        bounds.left.saturating_sub(display.pixels.left) as f64 / scale_x,
        bounds.top.saturating_sub(display.pixels.top) as f64 / scale_y,
        bounds.width as f64 / scale_x,
        bounds.height as f64 / scale_y,
    ))
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
    use super::{
        Bounds, DisplayMapping, PointBounds, point_rect_to_physical, source_rect_in_points,
        union_bounds,
    };

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

    #[test]
    fn retina_window_points_are_mapped_to_physical_pixels() {
        let display = DisplayMapping {
            display_id: 1,
            points: PointBounds {
                left: 0.0,
                top: 0.0,
                width: 1440.0,
                height: 900.0,
            },
            pixels: Bounds {
                left: 0,
                top: 0,
                width: 2880,
                height: 1800,
            },
            primary: true,
        };
        let actual = point_rect_to_physical(
            PointBounds {
                left: 100.0,
                top: 80.0,
                width: 920.0,
                height: 436.0,
            },
            &[display],
        )
        .unwrap();
        assert_eq!(
            actual,
            Bounds {
                left: 200,
                top: 160,
                width: 1840,
                height: 872,
            }
        );
    }

    #[test]
    fn physical_region_is_converted_back_to_display_points() {
        let display = DisplayMapping {
            display_id: 1,
            points: PointBounds {
                left: 0.0,
                top: 0.0,
                width: 1440.0,
                height: 900.0,
            },
            pixels: Bounds {
                left: 0,
                top: 0,
                width: 2880,
                height: 1800,
            },
            primary: true,
        };
        let actual = source_rect_in_points(
            Bounds {
                left: 200,
                top: 160,
                width: 1840,
                height: 872,
            },
            display,
        )
        .unwrap();
        assert_eq!(actual.origin.x, 100.0);
        assert_eq!(actual.origin.y, 80.0);
        assert_eq!(actual.size.width, 920.0);
        assert_eq!(actual.size.height, 436.0);
    }
}
