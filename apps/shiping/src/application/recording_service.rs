use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use shi_foundation::i18n;

use crate::{
    config::OutputFormat,
    domain::{AudioSourceKind as SourceKind, RecordingTarget, output_size},
    output,
    platform::{recording_backend, target_selection},
    ports::{RecordingBackend, TargetSelection},
};

const AUDIO_CHUNK_FRAMES: u64 = 1024;

trait RecordingClock {
    fn now(&self) -> Duration;
    fn sleep(&self, duration: Duration);
}

struct SystemClock {
    epoch: Instant,
}

impl SystemClock {
    fn new() -> Self {
        Self {
            epoch: Instant::now(),
        }
    }
}

impl RecordingClock for SystemClock {
    fn now(&self) -> Duration {
        self.epoch.elapsed()
    }

    fn sleep(&self, duration: Duration) {
        thread::sleep(duration);
    }
}

#[derive(Clone, Copy)]
struct RecordingServices<'a> {
    backend: &'a dyn RecordingBackend,
    targets: &'a dyn TargetSelection,
    clock: &'a dyn RecordingClock,
}

#[derive(Clone)]
pub struct RecordingOptions {
    pub target: RecordingTarget,
    pub quality_preset: u8,
    pub frames_per_second: u32,
    pub output_format: OutputFormat,
    pub system_audio: bool,
    pub microphone: bool,
    pub show_cursor: bool,
    pub highlight_clicks: bool,
    pub save_directory: PathBuf,
}

pub enum Command {
    TogglePause,
    Stop,
    SystemAudio(bool),
    Microphone(bool),
    ShowCursor(bool),
    HighlightClicks(bool),
}

#[derive(Debug)]
pub enum Event {
    Started {
        output_path: PathBuf,
        system_available: bool,
        microphone_available: bool,
        warnings: Vec<String>,
    },
    Progress(Duration),
    Paused(bool),
    AudioRejected(SourceKind, String),
    Completed {
        output_path: PathBuf,
        duration: Duration,
    },
    Failed(String),
}

pub struct RecorderHandle {
    commands: Sender<Command>,
    events: Receiver<Event>,
    thread: Option<JoinHandle<()>>,
}

impl RecorderHandle {
    pub fn start(options: RecordingOptions) -> Result<Self> {
        let (command_sender, command_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("shiping-recorder".to_owned())
            .spawn(move || recording_thread(options, command_receiver, event_sender))
            .context(i18n::text(
                "创建录制线程失败",
                "Failed to create the recording thread",
            ))?;
        Ok(Self {
            commands: command_sender,
            events: event_receiver,
            thread: Some(thread),
        })
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn drain_events(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

impl Drop for RecorderHandle {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn recording_thread(options: RecordingOptions, commands: Receiver<Command>, events: Sender<Event>) {
    if let Err(error) = run_recording(options, commands, &events) {
        let _ = events.send(Event::Failed(format!("{error:#}")));
    }
}

fn run_recording(
    options: RecordingOptions,
    commands: Receiver<Command>,
    events: &Sender<Event>,
) -> Result<()> {
    let clock = SystemClock::new();
    run_recording_with_services(
        RecordingServices {
            backend: recording_backend(),
            targets: target_selection(),
            clock: &clock,
        },
        options,
        commands,
        events,
    )
}

fn run_recording_with_services(
    services: RecordingServices<'_>,
    options: RecordingOptions,
    commands: Receiver<Command>,
    events: &Sender<Event>,
) -> Result<()> {
    let _runtime = services.backend.initialize_thread()?;
    let initial_bounds = services.targets.current_bounds(options.target)?;
    let (width, height) = output_size(initial_bounds, options.quality_preset);
    let paths = output::prepare(&options.save_directory, options.output_format)?;
    let result = run_with_output(
        services,
        &options,
        &paths,
        (width, height),
        commands,
        events,
    );
    if result.is_err() {
        output::discard_partial(&paths);
    }
    result
}

fn run_with_output(
    services: RecordingServices<'_>,
    options: &RecordingOptions,
    paths: &output::OutputPaths,
    output_size: (u32, u32),
    commands: Receiver<Command>,
    events: &Sender<Event>,
) -> Result<()> {
    let (width, height) = output_size;
    let mut audio = options.output_format.supports_audio().then(|| {
        services.backend.create_audio_capture(
            options.target,
            options.system_audio,
            options.microphone,
        )
    });
    let mut warnings = Vec::new();
    if let Some(audio) = audio.as_ref() {
        if options.system_audio && !audio.system_available() {
            return Err(anyhow!(
                "{}: {}",
                i18n::text(
                    "系统声音已启用，但采集设备不可用",
                    "System audio is enabled, but the capture device is unavailable"
                ),
                audio
                    .error(SourceKind::System)
                    .unwrap_or_else(|| i18n::text("未知原因", "Unknown reason"))
            ));
        }
        if options.microphone && !audio.microphone_available() {
            return Err(anyhow!(
                "{}: {}",
                i18n::text(
                    "麦克风已启用，但采集设备不可用",
                    "The microphone is enabled, but the capture device is unavailable"
                ),
                audio
                    .error(SourceKind::Microphone)
                    .unwrap_or_else(|| i18n::text("未知原因", "Unknown reason"))
            ));
        }
        if !audio.system_available() {
            warnings.push(format!(
                "{}: {}",
                i18n::text("系统声音不可用", "System audio is unavailable"),
                audio
                    .error(SourceKind::System)
                    .unwrap_or_else(|| i18n::text("未知原因", "Unknown reason"))
            ));
        }
        if !audio.microphone_available() {
            warnings.push(format!(
                "{}: {}",
                i18n::text("麦克风不可用", "The microphone is unavailable"),
                audio
                    .error(SourceKind::Microphone)
                    .unwrap_or_else(|| i18n::text("未知原因", "Unknown reason"))
            ));
        }
    }

    let system_available = audio.as_ref().is_some_and(|audio| audio.system_available());
    let microphone_available = audio
        .as_ref()
        .is_some_and(|audio| audio.microphone_available());
    let include_audio = audio.as_ref().is_some_and(|audio| audio.has_any_source());
    let mut writer = services.backend.create_writer(
        options.output_format,
        &paths.partial,
        width,
        height,
        options.frames_per_second,
        include_audio,
    )?;
    let mut grabber = services.backend.create_video_capture(
        options.target,
        width,
        height,
        options.show_cursor,
    )?;
    let audio_sample_rate = services.backend.audio_sample_rate();
    events
        .send(Event::Started {
            output_path: paths.final_path.clone(),
            system_available,
            microphone_available,
            warnings,
        })
        .ok();

    let mut active_duration = Duration::ZERO;
    let mut active_segment_started = Some(services.clock.now());
    let mut next_video_index = 0_u64;
    let mut audio_frame_index = 0_u64;
    let mut system_audio = options.system_audio;
    let mut microphone = options.microphone;
    let mut show_cursor = options.show_cursor;
    let mut highlight_clicks = options.highlight_clicks;
    let mut paused = false;
    let mut stopping = false;
    let mut last_progress = services.clock.now();

    while !stopping {
        loop {
            match commands.try_recv() {
                Ok(Command::TogglePause) => {
                    paused = !paused;
                    if paused {
                        if let Some(started) = active_segment_started.take() {
                            active_duration += services.clock.now().saturating_sub(started);
                        }
                    } else {
                        active_segment_started = Some(services.clock.now());
                    }
                    if let Some(audio) = audio.as_mut() {
                        audio.discard();
                    }
                    events.send(Event::Paused(paused)).ok();
                }
                Ok(Command::Stop) => {
                    stopping = true;
                    break;
                }
                Ok(Command::SystemAudio(enabled)) => {
                    let Some(audio) = audio.as_ref() else {
                        if enabled {
                            events
                                .send(Event::AudioRejected(
                                    SourceKind::System,
                                    i18n::text(
                                        "GIF 格式不支持系统声音",
                                        "GIF output does not support system audio",
                                    )
                                    .to_owned(),
                                ))
                                .ok();
                        }
                        continue;
                    };
                    if enabled && !audio.system_available() {
                        events
                            .send(Event::AudioRejected(
                                SourceKind::System,
                                audio
                                    .error(SourceKind::System)
                                    .unwrap_or_else(|| {
                                        i18n::text(
                                            "系统声音设备不可用",
                                            "The system audio device is unavailable",
                                        )
                                    })
                                    .to_owned(),
                            ))
                            .ok();
                    } else {
                        system_audio = enabled;
                    }
                }
                Ok(Command::Microphone(enabled)) => {
                    let Some(audio) = audio.as_ref() else {
                        if enabled {
                            events
                                .send(Event::AudioRejected(
                                    SourceKind::Microphone,
                                    i18n::text(
                                        "GIF 格式不支持麦克风",
                                        "GIF output does not support microphone audio",
                                    )
                                    .to_owned(),
                                ))
                                .ok();
                        }
                        continue;
                    };
                    if enabled && !audio.microphone_available() {
                        events
                            .send(Event::AudioRejected(
                                SourceKind::Microphone,
                                audio
                                    .error(SourceKind::Microphone)
                                    .unwrap_or_else(|| {
                                        i18n::text(
                                            "麦克风设备不可用",
                                            "The microphone device is unavailable",
                                        )
                                    })
                                    .to_owned(),
                            ))
                            .ok();
                    } else {
                        microphone = enabled;
                    }
                }
                Ok(Command::ShowCursor(enabled)) => show_cursor = enabled,
                Ok(Command::HighlightClicks(enabled)) => highlight_clicks = enabled,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    stopping = true;
                    break;
                }
            }
        }
        if stopping {
            break;
        }

        if let Some(audio) = audio.as_mut() {
            audio.pump()?;
        }
        if paused {
            if let Some(audio) = audio.as_mut() {
                audio.discard();
            }
            services.clock.sleep(Duration::from_millis(8));
            continue;
        }

        let elapsed = active_duration
            + active_segment_started
                .map(|started| services.clock.now().saturating_sub(started))
                .unwrap_or_default();
        let expected_video_index =
            (elapsed.as_secs_f64() * options.frames_per_second as f64).floor() as u64;
        if next_video_index <= expected_video_index {
            if expected_video_index > next_video_index + 2 {
                next_video_index = expected_video_index;
            }
            let bounds = services.targets.current_bounds(options.target)?;
            let pixels = grabber.capture(bounds, show_cursor, highlight_clicks)?;
            writer.write_video(next_video_index, pixels)?;
            next_video_index += 1;
        }

        if let Some(audio) = audio.as_mut() {
            let expected_audio_frames =
                (elapsed.as_secs_f64() * audio_sample_rate as f64).floor() as u64;
            while audio_frame_index + AUDIO_CHUNK_FRAMES <= expected_audio_frames {
                let pcm = audio.mix(AUDIO_CHUNK_FRAMES as usize, system_audio, microphone);
                writer.write_audio(audio_frame_index, &pcm)?;
                audio_frame_index += AUDIO_CHUNK_FRAMES;
            }
        }

        let now = services.clock.now();
        if now.saturating_sub(last_progress) >= Duration::from_millis(200) {
            events.send(Event::Progress(elapsed)).ok();
            last_progress = now;
        }
        services.clock.sleep(Duration::from_millis(2));
    }

    if let Some(started) = active_segment_started.take() {
        active_duration += services.clock.now().saturating_sub(started);
    }
    if let Some(mut audio) = audio.take() {
        let expected_audio_frames =
            (active_duration.as_secs_f64() * audio_sample_rate as f64).floor() as u64;
        if audio_frame_index < expected_audio_frames {
            let remaining = (expected_audio_frames - audio_frame_index) as usize;
            let pcm = audio.mix(remaining, system_audio, microphone);
            writer.write_audio(audio_frame_index, &pcm)?;
        }
    }
    writer.finalize()?;
    output::commit(paths)?;
    events
        .send(Event::Completed {
            output_path: paths.final_path.clone(),
            duration: active_duration,
        })
        .ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        fs,
        path::Path,
        sync::{Arc, Mutex, mpsc},
        thread,
        time::{Duration, Instant},
    };

    use anyhow::{Result, anyhow};

    use super::{
        Command, Event, RecorderHandle, RecordingClock, RecordingOptions, RecordingServices,
        run_recording_with_services,
    };
    use crate::{
        config::OutputFormat,
        domain::{AudioSourceKind, Bounds, MonitorCandidates, RecordingTarget, WindowCandidates},
        platform::target_selection,
        ports::{
            AudioCapture, MediaWriter, RecordingBackend, RecordingCapabilities,
            RecordingThreadRuntime, TargetSelection, VideoCapture,
        },
    };

    #[derive(Default)]
    struct FakeTrace {
        video_indices: Vec<u64>,
        capture_options: Vec<(bool, bool)>,
        audio_writes: Vec<(u64, usize, i16)>,
        audio_discards: usize,
        finalized: bool,
    }

    struct FakeBackend {
        trace: Arc<Mutex<FakeTrace>>,
        fail_video_write: bool,
    }

    struct FakeTargetSelection;
    struct FakeRuntime;
    struct FakeVideoCapture {
        pixels: Vec<u8>,
        trace: Arc<Mutex<FakeTrace>>,
    }
    struct FakeAudioCapture {
        trace: Arc<Mutex<FakeTrace>>,
    }
    struct FakeWriter {
        trace: Arc<Mutex<FakeTrace>>,
        fail_video_write: bool,
    }
    struct ScriptedClock {
        now: Cell<Duration>,
        commands: mpsc::Sender<Command>,
        schedule: RefCell<VecDeque<(Duration, Command)>>,
    }

    impl RecordingThreadRuntime for FakeRuntime {}

    impl RecordingClock for ScriptedClock {
        fn now(&self) -> Duration {
            self.now.get()
        }

        fn sleep(&self, duration: Duration) {
            let now = self.now.get() + duration;
            self.now.set(now);
            let mut schedule = self.schedule.borrow_mut();
            while schedule
                .front()
                .is_some_and(|(scheduled, _)| *scheduled <= now)
            {
                let (_, command) = schedule.pop_front().expect("scheduled command exists");
                self.commands
                    .send(command)
                    .expect("command receiver is open");
            }
        }
    }

    impl TargetSelection for FakeTargetSelection {
        fn monitors(&self, _owner: Option<&slint::Window>) -> Result<MonitorCandidates> {
            Ok(MonitorCandidates::new(Vec::new()))
        }

        fn windows(&self, _desktop: Bounds) -> Result<WindowCandidates> {
            Ok(WindowCandidates::new(Vec::new()))
        }

        fn primary_screen_bounds(&self) -> Result<Bounds> {
            Ok(fake_bounds())
        }

        fn virtual_desktop_bounds(&self) -> Result<Bounds> {
            Ok(fake_bounds())
        }

        fn current_bounds(&self, target: RecordingTarget) -> Result<Bounds> {
            Ok(target.initial_bounds())
        }
    }

    impl VideoCapture for FakeVideoCapture {
        fn capture(
            &mut self,
            _source: Bounds,
            show_cursor: bool,
            highlight_clicks: bool,
        ) -> Result<&[u8]> {
            self.trace
                .lock()
                .unwrap()
                .capture_options
                .push((show_cursor, highlight_clicks));
            Ok(&self.pixels)
        }
    }

    impl AudioCapture for FakeAudioCapture {
        fn system_available(&self) -> bool {
            true
        }

        fn microphone_available(&self) -> bool {
            true
        }

        fn error(&self, _kind: AudioSourceKind) -> Option<&str> {
            None
        }

        fn has_any_source(&self) -> bool {
            true
        }

        fn pump(&mut self) -> Result<()> {
            Ok(())
        }

        fn discard(&mut self) {
            self.trace.lock().unwrap().audio_discards += 1;
        }

        fn mix(
            &mut self,
            frames: usize,
            system_enabled: bool,
            microphone_enabled: bool,
        ) -> Vec<i16> {
            let marker = i16::from(system_enabled) + i16::from(microphone_enabled) * 2;
            vec![marker; frames * 2]
        }
    }

    impl MediaWriter for FakeWriter {
        fn write_video(&mut self, frame_index: u64, _bgra: &[u8]) -> Result<()> {
            if self.fail_video_write {
                return Err(anyhow!("fake video write failure"));
            }
            self.trace.lock().unwrap().video_indices.push(frame_index);
            Ok(())
        }

        fn write_audio(&mut self, start_frame: u64, pcm: &[i16]) -> Result<()> {
            self.trace.lock().unwrap().audio_writes.push((
                start_frame,
                pcm.len() / 2,
                pcm.first().copied().unwrap_or_default(),
            ));
            Ok(())
        }

        fn finalize(self: Box<Self>) -> Result<()> {
            self.trace.lock().unwrap().finalized = true;
            Ok(())
        }
    }

    impl RecordingBackend for FakeBackend {
        fn capabilities(&self) -> RecordingCapabilities {
            RecordingCapabilities {
                system_audio: true,
                microphone: true,
                highlight_clicks: true,
            }
        }

        fn initialize_thread(&self) -> Result<Box<dyn RecordingThreadRuntime>> {
            Ok(Box::new(FakeRuntime))
        }

        fn create_video_capture(
            &self,
            _target: RecordingTarget,
            width: u32,
            height: u32,
            _show_cursor: bool,
        ) -> Result<Box<dyn VideoCapture>> {
            Ok(Box::new(FakeVideoCapture {
                pixels: vec![0; width as usize * height as usize * 4],
                trace: Arc::clone(&self.trace),
            }))
        }

        fn create_audio_capture(
            &self,
            _target: RecordingTarget,
            _system_enabled: bool,
            _microphone_enabled: bool,
        ) -> Box<dyn AudioCapture> {
            Box::new(FakeAudioCapture {
                trace: Arc::clone(&self.trace),
            })
        }

        fn create_writer(
            &self,
            _format: OutputFormat,
            path: &Path,
            _width: u32,
            _height: u32,
            _frames_per_second: u32,
            _include_audio: bool,
        ) -> Result<Box<dyn MediaWriter>> {
            fs::write(path, b"fake recording")?;
            Ok(Box::new(FakeWriter {
                trace: Arc::clone(&self.trace),
                fail_video_write: self.fail_video_write,
            }))
        }

        fn audio_sample_rate(&self) -> u32 {
            48_000
        }
    }

    #[test]
    fn fake_backend_covers_pause_resume_stop_and_media_timeline() {
        let directory =
            std::env::temp_dir().join(format!("shiping-timeline-test-{}", std::process::id()));
        let trace = Arc::new(Mutex::new(FakeTrace::default()));
        let backend = FakeBackend {
            trace: Arc::clone(&trace),
            fail_video_write: false,
        };
        let (command_sender, command_receiver) = mpsc::channel();
        let clock = ScriptedClock {
            now: Cell::new(Duration::ZERO),
            commands: command_sender,
            schedule: RefCell::new(VecDeque::from([
                (Duration::from_millis(50), Command::SystemAudio(false)),
                (Duration::from_millis(50), Command::ShowCursor(true)),
                (Duration::from_millis(50), Command::HighlightClicks(true)),
                (Duration::from_millis(120), Command::TogglePause),
                (Duration::from_millis(320), Command::TogglePause),
                (Duration::from_millis(500), Command::Stop),
            ])),
        };
        let (event_sender, events) = mpsc::channel();

        run_recording_with_services(
            RecordingServices {
                backend: &backend,
                targets: &FakeTargetSelection,
                clock: &clock,
            },
            fake_options(directory.clone()),
            command_receiver,
            &event_sender,
        )
        .unwrap();

        let events: Vec<_> = events.try_iter().collect();
        assert!(matches!(events.first(), Some(Event::Started { .. })));
        assert_eq!(
            events
                .iter()
                .filter_map(|event| match event {
                    Event::Paused(paused) => Some(*paused),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [true, false]
        );
        assert!(matches!(
            events.last(),
            Some(Event::Completed { duration, .. }) if *duration == Duration::from_millis(300)
        ));

        let trace = trace.lock().unwrap();
        assert_eq!(trace.video_indices, [0, 1, 2]);
        assert_eq!(
            trace.capture_options,
            [(false, false), (true, true), (true, true)]
        );
        assert!(trace.audio_discards > 0);
        assert!(trace.finalized);
        assert_contiguous_audio_timeline(&trace.audio_writes, 14_400);
        assert!(trace.audio_writes.iter().any(|(_, _, marker)| *marker == 3));
        assert!(trace.audio_writes.iter().any(|(_, _, marker)| *marker == 2));
        drop(trace);

        let files = fs::read_dir(&directory)
            .unwrap()
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(fs::read(files[0].path()).unwrap(), b"fake recording");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn fake_backend_failure_removes_partial_output() {
        let directory =
            std::env::temp_dir().join(format!("shiping-rollback-test-{}", std::process::id()));
        let trace = Arc::new(Mutex::new(FakeTrace::default()));
        let backend = FakeBackend {
            trace: Arc::clone(&trace),
            fail_video_write: true,
        };
        let (command_sender, command_receiver) = mpsc::channel();
        let clock = ScriptedClock {
            now: Cell::new(Duration::ZERO),
            commands: command_sender,
            schedule: RefCell::new(VecDeque::new()),
        };
        let (event_sender, _events) = mpsc::channel();

        let error = run_recording_with_services(
            RecordingServices {
                backend: &backend,
                targets: &FakeTargetSelection,
                clock: &clock,
            },
            fake_options(directory.clone()),
            command_receiver,
            &event_sender,
        )
        .unwrap_err();

        assert!(error.to_string().contains("fake video write failure"));
        assert!(!trace.lock().unwrap().finalized);
        assert!(
            fs::read_dir(&directory).unwrap().next().is_none(),
            "failed recording left a partial or final file"
        );
        fs::remove_dir(directory).unwrap();
    }

    fn fake_options(save_directory: std::path::PathBuf) -> RecordingOptions {
        RecordingOptions {
            target: RecordingTarget::Screen(fake_bounds()),
            quality_preset: 3,
            frames_per_second: 10,
            output_format: OutputFormat::Mp4,
            system_audio: true,
            microphone: true,
            show_cursor: false,
            highlight_clicks: false,
            save_directory,
        }
    }

    fn fake_bounds() -> Bounds {
        Bounds {
            left: 0,
            top: 0,
            width: 16,
            height: 16,
        }
    }

    fn assert_contiguous_audio_timeline(writes: &[(u64, usize, i16)], expected_frames: u64) {
        let mut next_frame = 0_u64;
        for (start_frame, frames, _) in writes {
            assert_eq!(*start_frame, next_frame);
            next_frame += *frames as u64;
        }
        assert_eq!(next_frame, expected_frames);
    }

    #[test]
    #[ignore = "需要 Windows 桌面、Media Foundation 编码器和实际屏幕采集"]
    fn records_a_short_mp4() {
        let directory =
            std::env::temp_dir().join(format!("shiping-recorder-smoke-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let recorder = RecorderHandle::start(RecordingOptions {
            target: RecordingTarget::Screen(target_selection().primary_screen_bounds().unwrap()),
            quality_preset: 1,
            frames_per_second: 30,
            output_format: crate::config::OutputFormat::Mp4,
            system_audio: false,
            microphone: false,
            show_cursor: false,
            highlight_clicks: false,
            save_directory: directory.clone(),
        })
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut started = false;
        let mut completed = None;
        while Instant::now() < deadline {
            for event in recorder.drain_events() {
                match event {
                    Event::Started { .. } => started = true,
                    Event::Failed(message) => panic!("smoke recording failed: {message}"),
                    Event::Completed {
                        output_path,
                        duration,
                    } => completed = Some((output_path, duration)),
                    _ => {}
                }
            }
            if started && completed.is_none() {
                thread::sleep(Duration::from_millis(900));
                recorder.send(Command::Stop);
                started = false;
            }
            if completed.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let (output, duration) = completed.expect("recording did not complete before timeout");
        assert!(
            (Duration::from_millis(700)..=Duration::from_secs(2)).contains(&duration),
            "unexpected recording duration: {duration:?}"
        );
        let metadata = fs::metadata(&output).unwrap();
        assert!(metadata.len() > 1_024, "MP4 file is unexpectedly small");
        fs::remove_file(output).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
