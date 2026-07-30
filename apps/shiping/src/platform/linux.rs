use std::{
    fs,
    io::Read,
    os::fd::OwnedFd,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Mutex, OnceLock,
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use ashpd::desktop::{
    PersistMode,
    screencast::{
        CursorMode, Screencast, SelectSourcesOptions, SourceType, Stream as PortalStream,
    },
};
use pipewire as pw;
use pw::{properties::properties, spa};
use shi_foundation::i18n;
use slint::winit_030::WinitWindowAccessor;

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

pub(super) static DESKTOP_INTEGRATION: LinuxDesktopIntegration = LinuxDesktopIntegration;
pub(super) static RECORDING_BACKEND: LinuxRecordingBackend = LinuxRecordingBackend;
pub(super) static TARGET_SELECTION: LinuxTargetSelection = LinuxTargetSelection;

static MONITORS: OnceLock<Mutex<Vec<MonitorCandidate>>> = OnceLock::new();

pub(super) struct LinuxDesktopIntegration;
pub(super) struct LinuxRecordingBackend;
pub(super) struct LinuxTargetSelection;

struct LinuxRuntime;

impl RecordingThreadRuntime for LinuxRuntime {}

impl TargetSelection for LinuxTargetSelection {
    fn monitors(&self, owner: Option<&slint::Window>) -> Result<MonitorCandidates> {
        if let Some(owner) = owner {
            let candidates = owner
                .with_winit_window(|window| {
                    let primary = window.primary_monitor();
                    window
                        .available_monitors()
                        .map(|monitor| {
                            let position = monitor.position();
                            let size = monitor.size();
                            MonitorCandidate {
                                bounds: Bounds {
                                    left: position.x,
                                    top: position.y,
                                    width: size.width as i32,
                                    height: size.height as i32,
                                },
                                primary: primary.as_ref() == Some(&monitor),
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .ok_or_else(|| {
                    anyhow!(i18n::text(
                        "Slint Winit 窗口尚未创建，无法读取显示器",
                        "The Slint Winit window is not ready, so displays cannot be enumerated"
                    ))
                })?;
            if candidates.is_empty() {
                return Err(anyhow!(i18n::text("未找到显示器", "No display was found")));
            }
            *monitor_cache()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = candidates;
        }
        let values = monitor_cache()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if values.is_empty() {
            return Err(anyhow!(i18n::text(
                "显示器信息尚未初始化",
                "Display information has not been initialized"
            )));
        }
        Ok(MonitorCandidates::new(values))
    }

    fn windows(&self, desktop: Bounds) -> Result<WindowCandidates> {
        Ok(WindowCandidates::new(vec![WindowCandidate {
            id: WindowId::from_platform_value(0),
            bounds: desktop,
            title: i18n::text(
                "使用系统选择器选择窗口",
                "Choose a window in the system picker",
            )
            .to_owned(),
        }]))
    }

    fn primary_screen_bounds(&self) -> Result<Bounds> {
        let values = monitor_cache()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        values
            .iter()
            .find(|monitor| monitor.primary)
            .or_else(|| values.first())
            .map(|monitor| monitor.bounds)
            .ok_or_else(|| {
                anyhow!(i18n::text(
                    "显示器信息尚未初始化",
                    "Display information has not been initialized"
                ))
            })
    }

    fn virtual_desktop_bounds(&self) -> Result<Bounds> {
        union_bounds(
            monitor_cache()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .map(|monitor| monitor.bounds),
        )
    }

    fn current_bounds(&self, target: RecordingTarget) -> Result<Bounds> {
        target.initial_bounds().validate()
    }
}

impl RecordingBackend for LinuxRecordingBackend {
    fn capabilities(&self) -> RecordingCapabilities {
        RecordingCapabilities {
            system_audio: false,
            microphone: true,
            highlight_clicks: false,
        }
    }

    fn initialize_thread(&self) -> Result<Box<dyn RecordingThreadRuntime>> {
        Ok(Box::new(LinuxRuntime))
    }

    fn create_video_capture(
        &self,
        target: RecordingTarget,
        width: u32,
        height: u32,
        show_cursor: bool,
    ) -> Result<Box<dyn VideoCapture>> {
        Ok(Box::new(PortalPipeWireGrabber::new(
            target,
            width,
            height,
            show_cursor,
        )?))
    }

    fn create_audio_capture(
        &self,
        _target: RecordingTarget,
        _system_enabled: bool,
        microphone_enabled: bool,
    ) -> Box<dyn AudioCapture> {
        Box::new(PipeWireAudio::new(microphone_enabled))
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

struct PortalPipeWireGrabber {
    receiver: Receiver<FrameMessage>,
    stop: pw::channel::Sender<()>,
    worker: Option<JoinHandle<()>>,
    frame: Vec<u8>,
    target: RecordingTarget,
    portal_position: Option<(i32, i32)>,
    portal_size: Option<(i32, i32)>,
    width: u32,
    height: u32,
    latest: Option<RawFrame>,
    initial_show_cursor: bool,
}

impl PortalPipeWireGrabber {
    fn new(target: RecordingTarget, width: u32, height: u32, show_cursor: bool) -> Result<Self> {
        let (portal_stream, fd) = open_portal(target, show_cursor)?;
        let portal_position = portal_stream.position();
        let portal_size = portal_stream.size();
        let node_id = portal_stream.pipe_wire_node_id();
        let (frame_sender, frame_receiver) = mpsc::sync_channel(2);
        let (stop, stop_receiver) = pw::channel::channel();
        let worker = thread::Builder::new()
            .name("shiping-pipewire-video".to_owned())
            .spawn(move || pipewire_video_thread(node_id, fd, frame_sender, stop_receiver))
            .context(i18n::text(
                "创建 PipeWire 视频线程失败",
                "Failed to create the PipeWire video thread",
            ))?;
        Ok(Self {
            receiver: frame_receiver,
            stop,
            worker: Some(worker),
            frame: vec![0; width as usize * height as usize * 4],
            target,
            portal_position,
            portal_size,
            width,
            height,
            latest: None,
            initial_show_cursor: show_cursor,
        })
    }
}

impl Drop for PortalPipeWireGrabber {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl VideoCapture for PortalPipeWireGrabber {
    fn capture(
        &mut self,
        _source: Bounds,
        show_cursor: bool,
        _highlight_clicks: bool,
    ) -> Result<&[u8]> {
        if show_cursor != self.initial_show_cursor {
            return Err(anyhow!(i18n::text(
                "Linux Portal 会话开始后不能更改光标捕获选项",
                "The cursor capture option cannot be changed after a Linux Portal session starts"
            )));
        }
        match self.receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(FrameMessage::Frame(frame)) => self.latest = Some(frame),
            Ok(FrameMessage::Failed(error)) => return Err(anyhow!(error)),
            Err(mpsc::RecvTimeoutError::Timeout) if self.latest.is_none() => {
                return Err(anyhow!(i18n::text(
                    "等待 PipeWire 视频帧超时",
                    "Timed out waiting for a PipeWire video frame"
                )));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(anyhow!(i18n::text(
                    "PipeWire 视频流已断开",
                    "The PipeWire video stream disconnected"
                )));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                FrameMessage::Frame(frame) => self.latest = Some(frame),
                FrameMessage::Failed(error) => return Err(anyhow!(error)),
            }
        }
        let latest = self
            .latest
            .as_ref()
            .ok_or_else(|| anyhow!(i18n::text("尚未收到视频帧", "No video frame was received")))?;
        let crop = portal_crop(
            self.target,
            self.portal_position,
            self.portal_size,
            latest.width,
            latest.height,
        );
        convert_and_scale(latest, crop, self.width, self.height, &mut self.frame)?;
        Ok(&self.frame)
    }
}

#[derive(Clone)]
struct RawFrame {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    stride: usize,
    format: spa::param::video::VideoFormat,
}

enum FrameMessage {
    Frame(RawFrame),
    Failed(String),
}

fn open_portal(target: RecordingTarget, show_cursor: bool) -> Result<(PortalStream, OwnedFd)> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context(i18n::text(
            "创建 Portal 异步运行时失败",
            "Failed to create the Portal async runtime",
        ))?;
    runtime.block_on(async move {
        let proxy = Screencast::new().await?;
        let session = proxy.create_session(Default::default()).await?;
        let source = match target {
            RecordingTarget::Window { .. } => SourceType::Window,
            RecordingTarget::Screen(_) | RecordingTarget::Region(_) => SourceType::Monitor,
        };
        proxy
            .select_sources(
                &session,
                SelectSourcesOptions::default()
                    .set_cursor_mode(if show_cursor {
                        CursorMode::Embedded
                    } else {
                        CursorMode::Hidden
                    })
                    .set_sources(source)
                    .set_multiple(false)
                    .set_restore_token(None)
                    .set_persist_mode(PersistMode::DoNot),
            )
            .await?;
        let response = proxy
            .start(&session, None, Default::default())
            .await?
            .response()?;
        let stream = response.streams().first().cloned().ok_or_else(|| {
            anyhow!(i18n::text(
                "Portal 未返回视频流",
                "Portal returned no stream"
            ))
        })?;
        let fd = proxy
            .open_pipe_wire_remote(&session, Default::default())
            .await?;
        Ok::<_, anyhow::Error>((stream, fd))
    })
}

fn pipewire_video_thread(
    node_id: u32,
    fd: OwnedFd,
    sender: SyncSender<FrameMessage>,
    stop_receiver: pw::channel::Receiver<()>,
) {
    if let Err(error) = run_pipewire_video(node_id, fd, &sender, stop_receiver) {
        let _ = sender.send(FrameMessage::Failed(format!("{error:#}")));
    }
}

fn run_pipewire_video(
    node_id: u32,
    fd: OwnedFd,
    sender: &SyncSender<FrameMessage>,
    stop_receiver: pw::channel::Receiver<()>,
) -> Result<()> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_fd(fd, None)?;
    let stream = pw::stream::StreamBox::new(
        &core,
        "ShiPing ScreenCast",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )?;
    let stop_listener = stop_receiver.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        move |_| mainloop.quit()
    });
    let data = PipeWireVideoState {
        format: Default::default(),
    };
    let frame_sender = sender.clone();
    let listener = stream
        .add_local_listener_with_user_data(data)
        .param_changed(|_, state, id, param| {
            let Some(param) = param else {
                return;
            };
            if id == spa::param::ParamType::Format.as_raw() {
                let _ = state.format.parse(param);
            }
        })
        .process(move |stream, state| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let offset = data.chunk().offset() as usize;
            let size = data.chunk().size() as usize;
            let stride = data.chunk().stride().unsigned_abs() as usize;
            let Some(mapped) = data.data() else {
                return;
            };
            let end = offset.saturating_add(size).min(mapped.len());
            if end <= offset {
                return;
            }
            let raw = RawFrame {
                bytes: mapped[offset..end].to_vec(),
                width: state.format.size().width,
                height: state.format.size().height,
                stride,
                format: state.format.format(),
            };
            let _ = frame_sender.try_send(FrameMessage::Frame(raw));
        })
        .register()?;

    let object = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::RGBx
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            spa::utils::Rectangle {
                width: 16,
                height: 16
            },
            spa::utils::Rectangle {
                width: 8192,
                height: 8192
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 60, denom: 1 },
            spa::utils::Fraction { num: 1, denom: 1 },
            spa::utils::Fraction { num: 120, denom: 1 }
        ),
    );
    let values = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )?
    .0
    .into_inner();
    let mut params = [spa::pod::Pod::from_bytes(&values).ok_or_else(|| {
        anyhow!(i18n::text(
            "构造 PipeWire 视频格式参数失败",
            "Failed to construct PipeWire video format parameters"
        ))
    })?];
    stream.connect(
        spa::utils::Direction::Input,
        Some(node_id),
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
        &mut params,
    )?;
    mainloop.run();
    drop((listener, stop_listener, stream, core, context));
    Ok(())
}

struct PipeWireVideoState {
    format: spa::param::video::VideoInfoRaw,
}

struct PipeWireAudio {
    microphone: Option<Child>,
    receiver: Option<Receiver<Vec<u8>>>,
    worker: Option<JoinHandle<()>>,
    samples: Vec<i16>,
    error: Option<String>,
}

impl PipeWireAudio {
    fn new(microphone_enabled: bool) -> Self {
        if !microphone_enabled {
            return Self {
                microphone: None,
                receiver: None,
                worker: None,
                samples: Vec::new(),
                error: None,
            };
        }
        match spawn_microphone_capture() {
            Ok((child, receiver, worker)) => Self {
                microphone: Some(child),
                receiver: Some(receiver),
                worker: Some(worker),
                samples: Vec::new(),
                error: None,
            },
            Err(error) => Self {
                microphone: None,
                receiver: None,
                worker: None,
                samples: Vec::new(),
                error: Some(error.to_string()),
            },
        }
    }
}

impl Drop for PipeWireAudio {
    fn drop(&mut self) {
        if let Some(child) = self.microphone.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl AudioCapture for PipeWireAudio {
    fn system_available(&self) -> bool {
        false
    }

    fn microphone_available(&self) -> bool {
        self.microphone.is_some()
    }

    fn error(&self, kind: AudioSourceKind) -> Option<&str> {
        match kind {
            AudioSourceKind::System => Some(i18n::text(
                "Linux 系统声音采集未启用：PipeWire 没有可移植的默认监听源",
                "Linux system-audio capture is not enabled because PipeWire has no portable default monitor source",
            )),
            AudioSourceKind::Microphone => self.error.as_deref(),
        }
    }

    fn has_any_source(&self) -> bool {
        self.microphone_available()
    }

    fn pump(&mut self) -> Result<()> {
        let Some(receiver) = self.receiver.as_ref() else {
            return Ok(());
        };
        loop {
            match receiver.try_recv() {
                Ok(bytes) => {
                    self.samples.extend(
                        bytes
                            .chunks_exact(2)
                            .map(|sample| i16::from_le_bytes([sample[0], sample[1]])),
                    );
                }
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => {
                    return Err(anyhow!(i18n::text(
                        "pw-record 麦克风流已断开",
                        "The pw-record microphone stream disconnected"
                    )));
                }
            }
        }
    }

    fn discard(&mut self) {
        self.samples.clear();
    }

    fn mix(&mut self, frames: usize, _system_enabled: bool, microphone_enabled: bool) -> Vec<i16> {
        let sample_count = frames * 2;
        let take = sample_count.min(self.samples.len());
        let mut output = if microphone_enabled {
            self.samples.drain(..take).collect::<Vec<_>>()
        } else {
            self.samples.drain(..take).for_each(drop);
            Vec::new()
        };
        output.resize(sample_count, 0);
        output
    }
}

fn spawn_microphone_capture() -> Result<(Child, Receiver<Vec<u8>>, JoinHandle<()>)> {
    let mut child = Command::new("pw-record")
        .args([
            "--format=s16",
            "--rate=48000",
            "--channels=2",
            "--channel-map=stereo",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context(i18n::text(
            "无法启动 pw-record；请安装 PipeWire 工具",
            "Failed to start pw-record; install the PipeWire tools",
        ))?;
    let mut stdout = child.stdout.take().ok_or_else(|| {
        anyhow!(i18n::text(
            "pw-record 未提供标准输出",
            "pw-record did not provide stdout"
        ))
    })?;
    let (sender, receiver) = mpsc::sync_channel(8);
    let worker = thread::Builder::new()
        .name("shiping-pipewire-microphone".to_owned())
        .spawn(move || {
            loop {
                let mut bytes = vec![0; 8192];
                match stdout.read(&mut bytes) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        bytes.truncate(read);
                        if sender.send(bytes).is_err() {
                            break;
                        }
                    }
                }
            }
        })
        .context(i18n::text(
            "创建 PipeWire 麦克风线程失败",
            "Failed to create the PipeWire microphone thread",
        ))?;
    Ok((child, receiver, worker))
}

impl DesktopIntegration for LinuxDesktopIntegration {
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
        let status = Command::new("xdg-open")
            .arg(path)
            .status()
            .context(i18n::text(
                "启动 xdg-open 失败",
                "Failed to launch xdg-open",
            ))?;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow!(
                "{}: {status}",
                i18n::text("打开路径失败", "Failed to open the path")
            ))
        }
    }

    fn native_window_id(&self, window: &slint::Window) -> Option<WindowId> {
        window.with_winit_window(|window| {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            match window.window_handle().ok()?.as_raw() {
                RawWindowHandle::Wayland(handle) => Some(WindowId::from_platform_value(
                    handle.surface.as_ptr() as usize as u64,
                )),
                RawWindowHandle::Xlib(handle) => Some(WindowId::from_platform_value(handle.window)),
                RawWindowHandle::Xcb(handle) => {
                    Some(WindowId::from_platform_value(handle.window.get() as u64))
                }
                _ => None,
            }
        })?
    }

    fn activate_window(&self, window: &slint::Window) {
        let _ = window.show();
        window.request_redraw();
        window.with_winit_window(|window| window.focus_window());
    }
}

fn monitor_cache() -> &'static Mutex<Vec<MonitorCandidate>> {
    MONITORS.get_or_init(|| Mutex::new(Vec::new()))
}

fn union_bounds<'a>(values: impl Iterator<Item = Bounds> + 'a) -> Result<Bounds> {
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

fn portal_crop(
    target: RecordingTarget,
    position: Option<(i32, i32)>,
    compositor_size: Option<(i32, i32)>,
    frame_width: u32,
    frame_height: u32,
) -> Crop {
    let RecordingTarget::Region(bounds) = target else {
        return Crop::full(frame_width, frame_height);
    };
    let (Some((left, top)), Some((width, height))) = (position, compositor_size) else {
        return Crop::full(frame_width, frame_height);
    };
    if width <= 0 || height <= 0 {
        return Crop::full(frame_width, frame_height);
    }
    let x = ((bounds.left - left).max(0) as i64 * frame_width as i64 / width as i64) as u32;
    let y = ((bounds.top - top).max(0) as i64 * frame_height as i64 / height as i64) as u32;
    let crop_width = (bounds.width.max(1) as i64 * frame_width as i64 / width as i64) as u32;
    let crop_height = (bounds.height.max(1) as i64 * frame_height as i64 / height as i64) as u32;
    Crop {
        x: x.min(frame_width.saturating_sub(1)),
        y: y.min(frame_height.saturating_sub(1)),
        width: crop_width.max(1).min(frame_width.saturating_sub(x)),
        height: crop_height.max(1).min(frame_height.saturating_sub(y)),
    }
}

#[derive(Clone, Copy)]
struct Crop {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl Crop {
    fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }
}

fn convert_and_scale(
    source: &RawFrame,
    crop: Crop,
    output_width: u32,
    output_height: u32,
    output: &mut [u8],
) -> Result<()> {
    let expected = output_width as usize * output_height as usize * 4;
    if output.len() != expected || source.width == 0 || source.height == 0 {
        return Err(anyhow!(i18n::text(
            "视频帧尺寸无效",
            "The video frame dimensions are invalid"
        )));
    }
    let stride = source.stride.max(source.width as usize * 4);
    let minimum = stride
        .saturating_mul(source.height.saturating_sub(1) as usize)
        .saturating_add(source.width as usize * 4);
    if source.bytes.len() < minimum {
        return Err(anyhow!(i18n::text(
            "PipeWire 视频帧数据不足",
            "The PipeWire video frame data is too short"
        )));
    }
    for output_y in 0..output_height {
        let source_y = crop.y + output_y * crop.height / output_height;
        for output_x in 0..output_width {
            let source_x = crop.x + output_x * crop.width / output_width;
            let source_offset = source_y as usize * stride + source_x as usize * 4;
            if source_offset + 4 > source.bytes.len() {
                return Err(anyhow!(i18n::text(
                    "PipeWire 视频帧行跨度无效",
                    "The PipeWire video frame stride is invalid"
                )));
            }
            let target_offset = (output_y as usize * output_width as usize + output_x as usize) * 4;
            let pixel = &source.bytes[source_offset..source_offset + 4];
            if source.format == spa::param::video::VideoFormat::BGRA
                || source.format == spa::param::video::VideoFormat::BGRx
            {
                output[target_offset..target_offset + 3].copy_from_slice(&pixel[..3]);
            } else if source.format == spa::param::video::VideoFormat::RGBA
                || source.format == spa::param::video::VideoFormat::RGBx
            {
                output[target_offset] = pixel[2];
                output[target_offset + 1] = pixel[1];
                output[target_offset + 2] = pixel[0];
            } else {
                return Err(anyhow!(i18n::text(
                    "PipeWire 返回了未协商的像素格式",
                    "PipeWire returned an unnegotiated pixel format"
                )));
            }
            output[target_offset + 3] = 255;
        }
    }
    Ok(())
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
    use pipewire::spa;

    use crate::domain::{Bounds, RecordingTarget};

    use super::{Crop, RawFrame, convert_and_scale, portal_crop};

    #[test]
    fn rgba_frames_are_converted_to_bgra_and_scaled() {
        let source = RawFrame {
            bytes: vec![10, 20, 30, 40],
            width: 1,
            height: 1,
            stride: 4,
            format: spa::param::video::VideoFormat::RGBA,
        };
        let mut output = vec![0; 2 * 2 * 4];
        convert_and_scale(&source, Crop::full(1, 1), 2, 2, &mut output).unwrap();
        for pixel in output.chunks_exact(4) {
            assert_eq!(pixel, [30, 20, 10, 255]);
        }
    }

    #[test]
    fn region_coordinates_are_mapped_into_the_portal_stream() {
        let crop = portal_crop(
            RecordingTarget::Region(Bounds {
                left: 100,
                top: 50,
                width: 400,
                height: 200,
            }),
            Some((0, 0)),
            Some((1000, 500)),
            2000,
            1000,
        );
        assert_eq!(
            (crop.x, crop.y, crop.width, crop.height),
            (200, 100, 800, 400)
        );
    }

    #[test]
    fn invalid_pipewire_stride_is_rejected() {
        let source = RawFrame {
            bytes: vec![0; 4],
            width: 2,
            height: 2,
            stride: 8,
            format: spa::param::video::VideoFormat::BGRA,
        };
        let mut output = vec![0; 2 * 2 * 4];
        assert!(convert_and_scale(&source, Crop::full(2, 2), 2, 2, &mut output).is_err());
    }
}
