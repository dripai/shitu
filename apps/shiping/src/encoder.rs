use std::{os::windows::ffi::OsStrExt, path::Path, ptr};

use anyhow::{Context, Result};
use windows::{
    Win32::Media::MediaFoundation::{
        IMFAttributes, IMFMediaBuffer, IMFSample, IMFSinkWriter,
        MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
        MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT, MF_MT_AUDIO_NUM_CHANNELS,
        MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_AVG_BITRATE, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE,
        MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_PIXEL_ASPECT_RATIO,
        MF_MT_SUBTYPE, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_TRANSCODE_CONTAINERTYPE,
        MF_VERSION, MFAudioFormat_AAC, MFAudioFormat_PCM, MFCreateAttributes, MFCreateMediaType,
        MFCreateMemoryBuffer, MFCreateSample, MFCreateSinkWriterFromURL, MFMediaType_Audio,
        MFMediaType_Video, MFSTARTUP_FULL, MFShutdown, MFStartup, MFTranscodeContainerType_MPEG4,
        MFVideoFormat_H264, MFVideoFormat_RGB32, MFVideoInterlace_Progressive,
    },
    core::PCWSTR,
};

const HUNDRED_NS_PER_SECOND: i64 = 10_000_000;
pub const AUDIO_SAMPLE_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: u32 = 2;

pub struct MediaFoundationRuntime;

impl MediaFoundationRuntime {
    pub fn start() -> Result<Self> {
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.context("初始化 Media Foundation 失败")?;
        Ok(Self)
    }
}

impl Drop for MediaFoundationRuntime {
    fn drop(&mut self) {
        let _ = unsafe { MFShutdown() };
    }
}

pub struct Mp4Writer {
    writer: IMFSinkWriter,
    video_stream: u32,
    audio_stream: Option<u32>,
    frame_duration: i64,
    finalized: bool,
}

impl Mp4Writer {
    pub fn create(
        path: &Path,
        width: u32,
        height: u32,
        frames_per_second: u32,
        include_audio: bool,
    ) -> Result<Self> {
        let attributes = create_attributes(3)?;
        unsafe {
            attributes.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4)?;
            attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)?;
        }
        let path_wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let writer =
            unsafe { MFCreateSinkWriterFromURL(PCWSTR(path_wide.as_ptr()), None, &attributes) }
                .with_context(|| format!("创建 MP4 写入器失败：{}", path.display()))?;

        let video_output = video_output_type(width, height, frames_per_second)?;
        let video_stream =
            unsafe { writer.AddStream(&video_output) }.context("创建 H.264 视频流失败")?;
        let video_input = video_input_type(width, height, frames_per_second)?;
        unsafe { writer.SetInputMediaType(video_stream, &video_input, None) }
            .context("设置 RGB32 视频输入格式失败")?;

        let audio_stream = if include_audio {
            let output = audio_output_type()?;
            let stream = unsafe { writer.AddStream(&output) }.context("创建 AAC 音频流失败")?;
            let input = audio_input_type()?;
            unsafe { writer.SetInputMediaType(stream, &input, None) }
                .context("设置 PCM 音频输入格式失败")?;
            Some(stream)
        } else {
            None
        };

        unsafe { writer.BeginWriting() }.context("开始写入 MP4 失败")?;
        Ok(Self {
            writer,
            video_stream,
            audio_stream,
            frame_duration: HUNDRED_NS_PER_SECOND / frames_per_second as i64,
            finalized: false,
        })
    }

    pub fn write_video(&self, frame_index: u64, bgra: &[u8]) -> Result<()> {
        let sample = sample_from_bytes(
            bgra,
            frame_index as i64 * self.frame_duration,
            self.frame_duration,
        )?;
        unsafe { self.writer.WriteSample(self.video_stream, &sample) }.context("写入视频帧失败")
    }

    pub fn write_audio(&self, start_frame: u64, pcm: &[i16]) -> Result<()> {
        let Some(stream) = self.audio_stream else {
            return Ok(());
        };
        let frame_count = pcm.len() as u64 / AUDIO_CHANNELS as u64;
        if frame_count == 0 {
            return Ok(());
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(pcm.as_ptr().cast::<u8>(), size_of_val(pcm)) };
        let time = start_frame as i64 * HUNDRED_NS_PER_SECOND / AUDIO_SAMPLE_RATE as i64;
        let duration = frame_count as i64 * HUNDRED_NS_PER_SECOND / AUDIO_SAMPLE_RATE as i64;
        let sample = sample_from_bytes(bytes, time, duration)?;
        unsafe { self.writer.WriteSample(stream, &sample) }.context("写入音频采样失败")
    }

    pub fn finalize(mut self) -> Result<()> {
        unsafe { self.writer.Finalize() }.context("完成 MP4 文件失败")?;
        self.finalized = true;
        Ok(())
    }
}

fn create_attributes(capacity: u32) -> Result<IMFAttributes> {
    let mut value = None;
    unsafe { MFCreateAttributes(&mut value, capacity) }.context("创建媒体属性失败")?;
    value.context("Media Foundation 未返回属性对象")
}

fn video_output_type(
    width: u32,
    height: u32,
    fps: u32,
) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType> {
    let media = unsafe { MFCreateMediaType() }.context("创建视频输出格式失败")?;
    let bitrate =
        (width as u64 * height as u64 * fps as u64 / 8).clamp(2_000_000, 25_000_000) as u32;
    unsafe {
        media.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        media.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
        media.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
        media.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        media.SetUINT64(&MF_MT_FRAME_SIZE, pack_pair(width, height))?;
        media.SetUINT64(&MF_MT_FRAME_RATE, pack_pair(fps, 1))?;
        media.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_pair(1, 1))?;
    }
    Ok(media)
}

fn video_input_type(
    width: u32,
    height: u32,
    fps: u32,
) -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType> {
    let media = unsafe { MFCreateMediaType() }.context("创建视频输入格式失败")?;
    unsafe {
        media.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        media.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        media.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        media.SetUINT32(&MF_MT_DEFAULT_STRIDE, width.saturating_mul(4))?;
        media.SetUINT64(&MF_MT_FRAME_SIZE, pack_pair(width, height))?;
        media.SetUINT64(&MF_MT_FRAME_RATE, pack_pair(fps, 1))?;
        media.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_pair(1, 1))?;
    }
    Ok(media)
}

fn audio_output_type() -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType> {
    let media = unsafe { MFCreateMediaType() }.context("创建音频输出格式失败")?;
    unsafe {
        media.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        media.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
        media.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
        media.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, AUDIO_SAMPLE_RATE)?;
        media.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, AUDIO_CHANNELS)?;
        media.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24_000)?;
        media.SetUINT32(&MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, 0x29)?;
    }
    Ok(media)
}

fn audio_input_type() -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType> {
    let media = unsafe { MFCreateMediaType() }.context("创建音频输入格式失败")?;
    unsafe {
        media.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        media.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
        media.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
        media.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, AUDIO_SAMPLE_RATE)?;
        media.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, AUDIO_CHANNELS)?;
        media.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, AUDIO_CHANNELS * 2)?;
        media.SetUINT32(
            &MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
            AUDIO_SAMPLE_RATE * AUDIO_CHANNELS * 2,
        )?;
    }
    Ok(media)
}

fn sample_from_bytes(bytes: &[u8], time: i64, duration: i64) -> Result<IMFSample> {
    let buffer: IMFMediaBuffer =
        unsafe { MFCreateMemoryBuffer(bytes.len() as u32) }.context("创建媒体缓冲区失败")?;
    let mut destination = ptr::null_mut();
    unsafe {
        buffer.Lock(&mut destination, None, None)?;
        ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
        buffer.Unlock()?;
        buffer.SetCurrentLength(bytes.len() as u32)?;
    }
    let sample = unsafe { MFCreateSample() }.context("创建媒体采样失败")?;
    unsafe {
        sample.AddBuffer(&buffer)?;
        sample.SetSampleTime(time)?;
        sample.SetSampleDuration(duration)?;
    }
    Ok(sample)
}

fn pack_pair(first: u32, second: u32) -> u64 {
    ((first as u64) << 32) | second as u64
}

use std::mem::size_of_val;
