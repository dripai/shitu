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
    let backend = recording_backend();
    let targets = target_selection();
    let _runtime = backend.initialize_thread()?;
    let initial_bounds = targets.current_bounds(options.target)?;
    let (width, height) = output_size(initial_bounds, options.quality_preset);
    let paths = output::prepare(&options.save_directory, options.output_format)?;
    let result = run_with_output(
        backend,
        targets,
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
    backend: &dyn RecordingBackend,
    targets: &dyn TargetSelection,
    options: &RecordingOptions,
    paths: &output::OutputPaths,
    output_size: (u32, u32),
    commands: Receiver<Command>,
    events: &Sender<Event>,
) -> Result<()> {
    let (width, height) = output_size;
    let mut audio = options
        .output_format
        .supports_audio()
        .then(|| backend.create_audio_capture());
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
    let mut writer = backend.create_writer(
        options.output_format,
        &paths.partial,
        width,
        height,
        options.frames_per_second,
        include_audio,
    )?;
    let mut grabber = backend.create_video_capture(width, height)?;
    let audio_sample_rate = backend.audio_sample_rate();
    events
        .send(Event::Started {
            output_path: paths.final_path.clone(),
            system_available,
            microphone_available,
            warnings,
        })
        .ok();

    let mut active_duration = Duration::ZERO;
    let mut active_segment_started = Some(Instant::now());
    let mut next_video_index = 0_u64;
    let mut audio_frame_index = 0_u64;
    let mut system_audio = options.system_audio;
    let mut microphone = options.microphone;
    let mut show_cursor = options.show_cursor;
    let mut highlight_clicks = options.highlight_clicks;
    let mut paused = false;
    let mut stopping = false;
    let mut last_progress = Instant::now();

    while !stopping {
        loop {
            match commands.try_recv() {
                Ok(Command::TogglePause) => {
                    paused = !paused;
                    if paused {
                        if let Some(started) = active_segment_started.take() {
                            active_duration += started.elapsed();
                        }
                    } else {
                        active_segment_started = Some(Instant::now());
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
            thread::sleep(Duration::from_millis(8));
            continue;
        }

        let elapsed = active_duration
            + active_segment_started
                .map(|started| started.elapsed())
                .unwrap_or_default();
        let expected_video_index =
            (elapsed.as_secs_f64() * options.frames_per_second as f64).floor() as u64;
        if next_video_index <= expected_video_index {
            if expected_video_index > next_video_index + 2 {
                next_video_index = expected_video_index;
            }
            let bounds = targets.current_bounds(options.target)?;
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

        if last_progress.elapsed() >= Duration::from_millis(200) {
            events.send(Event::Progress(elapsed)).ok();
            last_progress = Instant::now();
        }
        thread::sleep(Duration::from_millis(2));
    }

    if let Some(started) = active_segment_started.take() {
        active_duration += started.elapsed();
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
        fs,
        path::Path,
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };

    use anyhow::Result;

    use super::{Command, Event, RecorderHandle, RecordingOptions, run_with_output};
    use crate::{
        config::OutputFormat,
        domain::{AudioSourceKind, Bounds, MonitorCandidates, RecordingTarget, WindowCandidates},
        output,
        platform::target_selection,
        ports::{
            AudioCapture, MediaWriter, RecordingBackend, RecordingThreadRuntime, TargetSelection,
            VideoCapture,
        },
    };

    struct FakeBackend;
    struct FakeTargetSelection;
    struct FakeRuntime;
    struct FakeVideoCapture {
        pixels: Vec<u8>,
    }
    struct FakeAudioCapture;
    struct FakeWriter;

    impl RecordingThreadRuntime for FakeRuntime {}

    impl TargetSelection for FakeTargetSelection {
        fn monitors(&self) -> Result<MonitorCandidates> {
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
            _show_cursor: bool,
            _highlight_clicks: bool,
        ) -> Result<&[u8]> {
            Ok(&self.pixels)
        }
    }

    impl AudioCapture for FakeAudioCapture {
        fn system_available(&self) -> bool {
            false
        }

        fn microphone_available(&self) -> bool {
            false
        }

        fn error(&self, _kind: AudioSourceKind) -> Option<&str> {
            Some("fake audio is unavailable")
        }

        fn has_any_source(&self) -> bool {
            false
        }

        fn pump(&mut self) -> Result<()> {
            Ok(())
        }

        fn discard(&mut self) {}

        fn mix(
            &mut self,
            frames: usize,
            _system_enabled: bool,
            _microphone_enabled: bool,
        ) -> Vec<i16> {
            vec![0; frames * 2]
        }
    }

    impl MediaWriter for FakeWriter {
        fn write_video(&mut self, _frame_index: u64, _bgra: &[u8]) -> Result<()> {
            Ok(())
        }

        fn write_audio(&mut self, _start_frame: u64, _pcm: &[i16]) -> Result<()> {
            Ok(())
        }

        fn finalize(self: Box<Self>) -> Result<()> {
            Ok(())
        }
    }

    impl RecordingBackend for FakeBackend {
        fn initialize_thread(&self) -> Result<Box<dyn RecordingThreadRuntime>> {
            Ok(Box::new(FakeRuntime))
        }

        fn create_video_capture(&self, width: u32, height: u32) -> Result<Box<dyn VideoCapture>> {
            Ok(Box::new(FakeVideoCapture {
                pixels: vec![0; width as usize * height as usize * 4],
            }))
        }

        fn create_audio_capture(&self) -> Box<dyn AudioCapture> {
            Box::new(FakeAudioCapture)
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
            Ok(Box::new(FakeWriter))
        }

        fn audio_sample_rate(&self) -> u32 {
            48_000
        }
    }

    #[test]
    fn recording_core_can_run_with_a_fake_backend() {
        let directory =
            std::env::temp_dir().join(format!("shiping-core-test-{}", std::process::id()));
        let paths = output::prepare(&directory, OutputFormat::Gif).unwrap();
        let options = RecordingOptions {
            target: RecordingTarget::Screen(Bounds {
                left: 0,
                top: 0,
                width: 16,
                height: 16,
            }),
            quality_preset: 3,
            frames_per_second: 10,
            output_format: OutputFormat::Gif,
            system_audio: false,
            microphone: false,
            show_cursor: false,
            highlight_clicks: false,
            save_directory: directory.clone(),
        };
        let (commands, command_receiver) = mpsc::channel();
        let (event_sender, events) = mpsc::channel();
        commands.send(Command::Stop).unwrap();

        run_with_output(
            &FakeBackend,
            &FakeTargetSelection,
            &options,
            &paths,
            (16, 16),
            command_receiver,
            &event_sender,
        )
        .unwrap();

        let events: Vec<_> = events.try_iter().collect();
        assert!(matches!(events.first(), Some(Event::Started { .. })));
        assert!(matches!(events.last(), Some(Event::Completed { .. })));
        assert_eq!(fs::read(&paths.final_path).unwrap(), b"fake recording");
        fs::remove_dir_all(directory).unwrap();
    }

    fn fake_bounds() -> Bounds {
        Bounds {
            left: 0,
            top: 0,
            width: 16,
            height: 16,
        }
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
