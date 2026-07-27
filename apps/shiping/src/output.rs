use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use gif::{Encoder, Frame, Repeat};
use shi_foundation::i18n;

use crate::{
    config::OutputFormat,
    platform::encoder::{MediaFoundationRuntime, Mp4Writer},
};

pub struct OutputPaths {
    pub partial: PathBuf,
    pub final_path: PathBuf,
}

pub enum OutputWriter {
    Mp4 {
        writer: Mp4Writer,
        runtime: MediaFoundationRuntime,
    },
    Gif(GifWriter),
}

impl OutputWriter {
    pub fn create(
        format: OutputFormat,
        path: &Path,
        width: u32,
        height: u32,
        frames_per_second: u32,
        include_audio: bool,
    ) -> Result<Self> {
        match format {
            OutputFormat::Mp4 => {
                let runtime = MediaFoundationRuntime::start()?;
                let writer =
                    Mp4Writer::create(path, width, height, frames_per_second, include_audio)?;
                Ok(Self::Mp4 { writer, runtime })
            }
            OutputFormat::Gif => {
                if include_audio {
                    return Err(anyhow!(i18n::text(
                        "GIF 格式不支持音频",
                        "GIF output does not support audio"
                    )));
                }
                Ok(Self::Gif(GifWriter::create(
                    path,
                    width,
                    height,
                    frames_per_second,
                )?))
            }
        }
    }

    pub fn write_video(&mut self, frame_index: u64, bgra: &[u8]) -> Result<()> {
        match self {
            Self::Mp4 { writer, .. } => writer.write_video(frame_index, bgra),
            Self::Gif(writer) => writer.write_video(frame_index, bgra),
        }
    }

    pub fn write_audio(&mut self, start_frame: u64, pcm: &[i16]) -> Result<()> {
        match self {
            Self::Mp4 { writer, .. } => writer.write_audio(start_frame, pcm),
            Self::Gif(_) => Err(anyhow!(i18n::text(
                "不能向 GIF 文件写入音频",
                "Audio cannot be written to a GIF file"
            ))),
        }
    }

    pub fn finalize(self) -> Result<()> {
        match self {
            Self::Mp4 { writer, runtime } => {
                let result = writer.finalize();
                drop(runtime);
                result
            }
            Self::Gif(writer) => writer.finalize(),
        }
    }
}

pub struct GifWriter {
    encoder: Encoder<File>,
    frame_delay: u16,
    width: u16,
    height: u16,
    pending_frame: Option<(u64, Vec<u8>)>,
}

impl GifWriter {
    fn create(path: &Path, width: u32, height: u32, frames_per_second: u32) -> Result<Self> {
        let width = u16::try_from(width).context(i18n::text(
            "GIF 宽度超过格式限制",
            "The GIF width exceeds the format limit",
        ))?;
        let height = u16::try_from(height).context(i18n::text(
            "GIF 高度超过格式限制",
            "The GIF height exceeds the format limit",
        ))?;
        let frame_delay = match frames_per_second {
            10 => 10,
            20 => 5,
            _ => {
                return Err(anyhow!(i18n::text(
                    "GIF 帧率必须为 10 或 20 FPS",
                    "The GIF frame rate must be 10 or 20 FPS"
                )));
            }
        };
        let file = File::create(path).with_context(|| {
            format!(
                "{}: {}",
                i18n::text("创建 GIF 文件失败", "Failed to create the GIF file"),
                path.display()
            )
        })?;
        let mut encoder = Encoder::new(file, width, height, &[]).context(i18n::text(
            "初始化 GIF 编码器失败",
            "Failed to initialize the GIF encoder",
        ))?;
        encoder.set_repeat(Repeat::Infinite).context(i18n::text(
            "设置 GIF 循环播放失败",
            "Failed to enable GIF looping",
        ))?;
        Ok(Self {
            encoder,
            frame_delay,
            width,
            height,
            pending_frame: None,
        })
    }

    fn write_video(&mut self, frame_index: u64, bgra: &[u8]) -> Result<()> {
        let expected_length = self.width as usize * self.height as usize * 4;
        if bgra.len() != expected_length {
            return Err(anyhow!(
                "{}: {} != {}",
                i18n::text(
                    "GIF 输入帧的像素长度无效",
                    "The GIF input frame has an invalid pixel length"
                ),
                bgra.len(),
                expected_length
            ));
        }

        let mut rgba = Vec::with_capacity(expected_length);
        for pixel in bgra.chunks_exact(4) {
            rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 0xff]);
        }

        if let Some((previous_index, previous_rgba)) =
            self.pending_frame.replace((frame_index, rgba))
        {
            let elapsed_frames = frame_index.saturating_sub(previous_index).max(1);
            let delay = elapsed_frames
                .checked_mul(u64::from(self.frame_delay))
                .and_then(|delay| u16::try_from(delay).ok())
                .context(i18n::text(
                    "GIF 相邻帧的时间间隔超过格式限制",
                    "The interval between GIF frames exceeds the format limit",
                ))?;
            self.write_rgba(previous_rgba, delay)?;
        }
        Ok(())
    }

    fn write_rgba(&mut self, mut rgba: Vec<u8>, delay: u16) -> Result<()> {
        let mut frame = Frame::from_rgba_speed(self.width, self.height, &mut rgba, 10);
        frame.delay = delay;
        self.encoder
            .write_frame(&frame)
            .context(i18n::text("写入 GIF 帧失败", "Failed to write a GIF frame"))
    }

    fn finalize(mut self) -> Result<()> {
        if let Some((_, rgba)) = self.pending_frame.take() {
            self.write_rgba(rgba, self.frame_delay)?;
        }
        let file = self.encoder.into_inner().context(i18n::text(
            "完成 GIF 文件失败",
            "Failed to finalize the GIF file",
        ))?;
        file.sync_all().context(i18n::text(
            "同步 GIF 文件失败",
            "Failed to flush the GIF file",
        ))
    }
}

pub fn prepare(directory: &Path, format: OutputFormat) -> Result<OutputPaths> {
    fs::create_dir_all(directory).with_context(|| {
        format!(
            "{}: {}",
            i18n::text("创建保存目录失败", "Failed to create the save folder"),
            directory.display()
        )
    })?;
    let timestamp = timestamp();
    for suffix in 0..10_000_u32 {
        let stem = if suffix == 0 {
            format!("Recording_{timestamp}")
        } else {
            format!("Recording_{timestamp}_{suffix}")
        };
        let extension = format.extension();
        let final_path = directory.join(format!("{stem}.{extension}"));
        let partial = directory.join(format!(".{stem}.partial.{extension}"));
        if !final_path.exists() && !partial.exists() {
            return Ok(OutputPaths {
                partial,
                final_path,
            });
        }
    }
    Err(anyhow!(i18n::text(
        "无法生成不重复的录制文件名",
        "Could not generate a unique recording filename"
    )))
}

fn timestamp() -> String {
    crate::platform::local_timestamp()
}

pub fn commit(paths: &OutputPaths) -> Result<()> {
    fs::rename(&paths.partial, &paths.final_path).with_context(|| {
        format!(
            "{}: {} -> {}",
            i18n::text("完成录制文件失败", "Failed to finalize the recorded file"),
            paths.partial.display(),
            paths.final_path.display()
        )
    })
}

pub fn discard_partial(paths: &OutputPaths) {
    let _ = fs::remove_file(&paths.partial);
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use gif::{ColorOutput, DecodeOptions};

    use super::{OutputWriter, commit, prepare};
    use crate::config::OutputFormat;

    #[test]
    fn output_uses_selected_extension_and_hidden_partial_file() {
        let directory =
            std::env::temp_dir().join(format!("shiping-output-test-{}", std::process::id()));
        let paths = prepare(&directory, OutputFormat::Mp4).unwrap();
        assert_eq!(
            paths
                .final_path
                .extension()
                .and_then(|value| value.to_str()),
            Some("mp4")
        );
        assert!(
            paths
                .partial
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.starts_with('.') && value.ends_with(".partial.mp4"))
        );
        let gif_paths = prepare(&directory, OutputFormat::Gif).unwrap();
        assert_eq!(
            gif_paths
                .final_path
                .extension()
                .and_then(|value| value.to_str()),
            Some("gif")
        );
        assert!(
            gif_paths
                .partial
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.starts_with('.') && value.ends_with(".partial.gif"))
        );
        let _ = std::fs::remove_dir(directory);
    }

    #[test]
    fn gif_writer_creates_a_two_frame_animation() {
        let directory =
            std::env::temp_dir().join(format!("shiping-gif-test-{}", std::process::id()));
        let paths = prepare(&directory, OutputFormat::Gif).unwrap();
        let mut writer =
            OutputWriter::create(OutputFormat::Gif, &paths.partial, 2, 2, 10, false).unwrap();
        let first = [
            0_u8, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0,
        ];
        let second = [
            255_u8, 255, 255, 0, 255, 0, 0, 0, 0, 255, 0, 0, 0, 0, 255, 0,
        ];
        writer.write_video(0, &first).unwrap();
        writer.write_video(1, &second).unwrap();
        writer.finalize().unwrap();
        commit(&paths).unwrap();

        let mut options = DecodeOptions::new();
        options.set_color_output(ColorOutput::RGBA);
        let mut decoder = options
            .read_info(File::open(&paths.final_path).unwrap())
            .unwrap();
        let mut delays = Vec::new();
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            delays.push(frame.delay);
        }
        assert_eq!(delays, [10, 10]);

        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn gif_writer_preserves_elapsed_time_when_frame_indices_skip() {
        let directory =
            std::env::temp_dir().join(format!("shiping-gif-delay-test-{}", std::process::id()));
        let paths = prepare(&directory, OutputFormat::Gif).unwrap();
        let mut writer =
            OutputWriter::create(OutputFormat::Gif, &paths.partial, 1, 1, 10, false).unwrap();
        let pixel = [0_u8, 0, 255, 0];
        writer.write_video(0, &pixel).unwrap();
        writer.write_video(3, &pixel).unwrap();
        writer.finalize().unwrap();
        commit(&paths).unwrap();

        let mut decoder = DecodeOptions::new()
            .read_info(File::open(&paths.final_path).unwrap())
            .unwrap();
        let mut delays = Vec::new();
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            delays.push(frame.delay);
        }
        assert_eq!(delays, [30, 10]);

        let _ = std::fs::remove_dir_all(directory);
    }
}
