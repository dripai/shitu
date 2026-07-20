use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use shi_foundation::i18n;

use crate::{
    output,
    platform::{
        ComRuntime,
        audio::{AudioSources, SourceKind},
        capture::{FrameGrabber, output_size},
        encoder::{AUDIO_SAMPLE_RATE, MediaFoundationRuntime, Mp4Writer},
        target::RecordingTarget,
    },
};

const AUDIO_CHUNK_FRAMES: u64 = 1024;

#[derive(Clone)]
pub struct RecordingOptions {
    pub target: RecordingTarget,
    pub quality_preset: u8,
    pub frames_per_second: u32,
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
    let _com = ComRuntime::initialize()?;
    let _media_foundation = MediaFoundationRuntime::start()?;
    let initial_bounds = options.target.current_bounds()?;
    let (width, height) = output_size(initial_bounds, options.quality_preset);
    let paths = output::prepare(&options.save_directory)?;
    let result = run_with_output(&options, &paths, width, height, commands, events);
    if result.is_err() {
        output::discard_partial(&paths);
    }
    result
}

fn run_with_output(
    options: &RecordingOptions,
    paths: &output::OutputPaths,
    width: u32,
    height: u32,
    commands: Receiver<Command>,
    events: &Sender<Event>,
) -> Result<()> {
    let mut audio = AudioSources::initialize();
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

    let mut warnings = Vec::new();
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

    let writer = Mp4Writer::create(
        &paths.partial,
        width,
        height,
        options.frames_per_second,
        audio.has_any_source(),
    )?;
    let mut grabber = FrameGrabber::new(width, height)?;
    events
        .send(Event::Started {
            output_path: paths.final_path.clone(),
            system_available: audio.system_available(),
            microphone_available: audio.microphone_available(),
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
                    audio.discard();
                    events.send(Event::Paused(paused)).ok();
                }
                Ok(Command::Stop) => {
                    stopping = true;
                    break;
                }
                Ok(Command::SystemAudio(enabled)) => {
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

        audio.pump()?;
        if paused {
            audio.discard();
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
            let bounds = options.target.current_bounds()?;
            let pixels = grabber.capture(bounds, show_cursor, highlight_clicks)?;
            writer.write_video(next_video_index, pixels)?;
            next_video_index += 1;
        }

        let expected_audio_frames =
            (elapsed.as_secs_f64() * AUDIO_SAMPLE_RATE as f64).floor() as u64;
        while audio_frame_index + AUDIO_CHUNK_FRAMES <= expected_audio_frames {
            let pcm = audio.mix(AUDIO_CHUNK_FRAMES as usize, system_audio, microphone);
            writer.write_audio(audio_frame_index, &pcm)?;
            audio_frame_index += AUDIO_CHUNK_FRAMES;
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
    let expected_audio_frames =
        (active_duration.as_secs_f64() * AUDIO_SAMPLE_RATE as f64).floor() as u64;
    if audio_frame_index < expected_audio_frames {
        let remaining = (expected_audio_frames - audio_frame_index) as usize;
        let pcm = audio.mix(remaining, system_audio, microphone);
        writer.write_audio(audio_frame_index, &pcm)?;
    }
    drop(audio);
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
        fs, thread,
        time::{Duration, Instant},
    };

    use super::{Command, Event, RecorderHandle, RecordingOptions};
    use crate::platform::target::{RecordingTarget, primary_screen_bounds};

    #[test]
    #[ignore = "需要 Windows 桌面、Media Foundation 编码器和实际屏幕采集"]
    fn records_a_short_mp4() {
        let directory =
            std::env::temp_dir().join(format!("shiping-recorder-smoke-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let recorder = RecorderHandle::start(RecordingOptions {
            target: RecordingTarget::Screen(primary_screen_bounds().unwrap()),
            quality_preset: 1,
            frames_per_second: 30,
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
