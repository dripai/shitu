use std::{
    ffi::OsString,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow};
use shi_foundation::i18n;

use crate::ports::MediaWriter;

const AUDIO_SAMPLE_RATE: u32 = 48_000;
const AUDIO_CHANNELS: usize = 2;

pub(super) struct FfmpegWriter {
    video: BufWriter<File>,
    audio: Option<BufWriter<File>>,
    video_path: PathBuf,
    audio_path: Option<PathBuf>,
    output_path: PathBuf,
    width: u32,
    height: u32,
    frames_per_second: u32,
    next_video_frame: u64,
    next_audio_frame: u64,
}

impl FfmpegWriter {
    pub(super) fn create(
        output_path: &Path,
        width: u32,
        height: u32,
        frames_per_second: u32,
        include_audio: bool,
    ) -> Result<Self> {
        let video_path = sidecar_path(output_path, "video.bgra");
        let audio_path = include_audio.then(|| sidecar_path(output_path, "audio.s16le"));
        let video = BufWriter::new(File::create(&video_path).with_context(|| {
            format!(
                "{}: {}",
                i18n::text(
                    "创建临时视频数据文件失败",
                    "Failed to create the temporary video data file"
                ),
                video_path.display()
            )
        })?);
        let audio = audio_path
            .as_ref()
            .map(|path| {
                File::create(path).map(BufWriter::new).with_context(|| {
                    format!(
                        "{}: {}",
                        i18n::text(
                            "创建临时音频数据文件失败",
                            "Failed to create the temporary audio data file"
                        ),
                        path.display()
                    )
                })
            })
            .transpose()?;
        Ok(Self {
            video,
            audio,
            video_path,
            audio_path,
            output_path: output_path.to_owned(),
            width,
            height,
            frames_per_second,
            next_video_frame: 0,
            next_audio_frame: 0,
        })
    }
}

impl MediaWriter for FfmpegWriter {
    fn write_video(&mut self, frame_index: u64, bgra: &[u8]) -> Result<()> {
        let expected = self.width as usize * self.height as usize * 4;
        if bgra.len() != expected {
            return Err(anyhow!(
                "{}: expected {expected}, got {}",
                i18n::text(
                    "视频帧字节数不正确",
                    "The video frame byte count is invalid"
                ),
                bgra.len()
            ));
        }
        if frame_index != self.next_video_frame {
            return Err(anyhow!(
                "{}: expected {}, got {frame_index}",
                i18n::text(
                    "视频帧时间轴不连续",
                    "The video frame timeline is not contiguous"
                ),
                self.next_video_frame
            ));
        }
        self.video.write_all(bgra)?;
        self.next_video_frame += 1;
        Ok(())
    }

    fn write_audio(&mut self, start_frame: u64, pcm: &[i16]) -> Result<()> {
        let Some(audio) = self.audio.as_mut() else {
            return Ok(());
        };
        if start_frame != self.next_audio_frame {
            return Err(anyhow!(
                "{}: expected {}, got {start_frame}",
                i18n::text(
                    "音频采样时间轴不连续",
                    "The audio sample timeline is not contiguous"
                ),
                self.next_audio_frame
            ));
        }
        if !pcm.len().is_multiple_of(AUDIO_CHANNELS) {
            return Err(anyhow!(i18n::text(
                "音频采样不是立体声帧",
                "The audio samples do not contain complete stereo frames"
            )));
        }
        let mut bytes = Vec::with_capacity(std::mem::size_of_val(pcm));
        for sample in pcm {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        audio.write_all(&bytes)?;
        self.next_audio_frame += (pcm.len() / AUDIO_CHANNELS) as u64;
        Ok(())
    }

    fn finalize(mut self: Box<Self>) -> Result<()> {
        self.video.flush()?;
        self.video.get_ref().sync_all()?;
        if let Some(audio) = self.audio.as_mut() {
            audio.flush()?;
            audio.get_ref().sync_all()?;
        }

        let args = ffmpeg_arguments(
            &self.video_path,
            self.audio_path.as_deref(),
            &self.output_path,
            self.width,
            self.height,
            self.frames_per_second,
        );
        let status = Command::new("ffmpeg")
            .args(&args)
            .status()
            .context(i18n::text(
                "无法启动 FFmpeg；请安装 FFmpeg 并确保其位于 PATH 中",
                "Failed to start FFmpeg; install FFmpeg and make sure it is available on PATH",
            ))?;
        if !status.success() {
            return Err(anyhow!(
                "{}: {status}",
                i18n::text("FFmpeg 封装 MP4 失败", "FFmpeg failed to mux the MP4 file")
            ));
        }
        fs::remove_file(&self.video_path).with_context(|| {
            format!(
                "{}: {}",
                i18n::text(
                    "删除临时视频数据失败",
                    "Failed to remove temporary video data"
                ),
                self.video_path.display()
            )
        })?;
        if let Some(path) = &self.audio_path {
            fs::remove_file(path).with_context(|| {
                format!(
                    "{}: {}",
                    i18n::text(
                        "删除临时音频数据失败",
                        "Failed to remove temporary audio data"
                    ),
                    path.display()
                )
            })?;
        }
        Ok(())
    }
}

impl Drop for FfmpegWriter {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.video_path);
        if let Some(path) = &self.audio_path {
            let _ = fs::remove_file(path);
        }
    }
}

fn sidecar_path(output_path: &Path, suffix: &str) -> PathBuf {
    let mut name = output_path
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("shiping"));
    name.push(".");
    name.push(suffix);
    output_path.with_file_name(name)
}

fn ffmpeg_arguments(
    video_path: &Path,
    audio_path: Option<&Path>,
    output_path: &Path,
    width: u32,
    height: u32,
    frames_per_second: u32,
) -> Vec<OsString> {
    let mut args = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-f".into(),
        "rawvideo".into(),
        "-pixel_format".into(),
        "bgra".into(),
        "-video_size".into(),
        format!("{width}x{height}").into(),
        "-framerate".into(),
        frames_per_second.to_string().into(),
        "-i".into(),
        video_path.as_os_str().to_owned(),
    ];
    if let Some(audio_path) = audio_path {
        args.extend([
            "-f".into(),
            "s16le".into(),
            "-ar".into(),
            AUDIO_SAMPLE_RATE.to_string().into(),
            "-ac".into(),
            AUDIO_CHANNELS.to_string().into(),
            "-i".into(),
            audio_path.as_os_str().to_owned(),
        ]);
    }
    args.extend([
        "-c:v".into(),
        "libx264".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-preset".into(),
        "veryfast".into(),
    ]);
    if audio_path.is_some() {
        args.extend(["-c:a".into(), "aac".into(), "-shortest".into()]);
    }
    args.extend([
        "-movflags".into(),
        "+faststart".into(),
        output_path.as_os_str().to_owned(),
    ]);
    args
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::{FfmpegWriter, ffmpeg_arguments, sidecar_path};

    #[test]
    fn sidecar_paths_stay_next_to_partial_output() {
        assert_eq!(
            sidecar_path(Path::new("/tmp/recording.mp4.partial"), "video.bgra"),
            Path::new("/tmp/recording.mp4.partial.video.bgra")
        );
    }

    #[test]
    fn ffmpeg_arguments_describe_bgra_and_optional_pcm_inputs() {
        let args = ffmpeg_arguments(
            Path::new("video.raw"),
            Some(Path::new("audio.raw")),
            Path::new("out.mp4"),
            1920,
            1080,
            30,
        );
        let values = args
            .iter()
            .map(|value| value.to_string_lossy())
            .collect::<Vec<_>>();
        assert!(
            values
                .windows(2)
                .any(|pair| pair == ["-pixel_format", "bgra"])
        );
        assert!(values.windows(2).any(|pair| pair == ["-f", "s16le"]));
        assert!(values.windows(2).any(|pair| pair == ["-c:v", "libx264"]));
        assert_eq!(values.last().map(AsRef::as_ref), Some("out.mp4"));
    }

    #[test]
    fn dropping_an_unfinished_writer_removes_sidecars() {
        let directory =
            std::env::temp_dir().join(format!("shiping-ffmpeg-cleanup-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let output = directory.join("recording.partial.mp4");
        let video = sidecar_path(&output, "video.bgra");
        let audio = sidecar_path(&output, "audio.s16le");
        let writer = FfmpegWriter::create(&output, 16, 16, 30, true).unwrap();
        assert!(video.exists());
        assert!(audio.exists());
        drop(writer);
        assert!(!video.exists());
        assert!(!audio.exists());
        fs::remove_dir(directory).unwrap();
    }
}
