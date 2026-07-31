use std::{
    ffi::c_void,
    path::Path,
    ptr::{NonNull, null, null_mut},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, ensure};
use block2::RcBlock;
use objc2::{rc::Retained, runtime::AnyObject};
use objc2_av_foundation::{
    AVAssetWriter, AVAssetWriterInput, AVAssetWriterInputPixelBufferAdaptor, AVAssetWriterStatus,
    AVFileTypeMPEG4, AVMediaTypeAudio, AVMediaTypeVideo, AVVideoCodecKey, AVVideoCodecTypeH264,
    AVVideoHeightKey, AVVideoWidthKey,
};
use objc2_avf_audio::{AVEncoderBitRateKey, AVFormatIDKey, AVNumberOfChannelsKey, AVSampleRateKey};
use objc2_core_audio_types::{
    AudioStreamBasicDescription, kAudioFormatFlagIsPacked, kAudioFormatFlagIsSignedInteger,
    kAudioFormatLinearPCM, kAudioFormatMPEG4AAC,
};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{
    CMAudioFormatDescriptionCreate, CMBlockBuffer, CMFormatDescription, CMSampleBuffer,
    CMSampleTimingInfo, CMTime, kCMBlockBufferAssureMemoryNowFlag, kCMBlockBufferNoErr,
    kCMTimeInvalid,
};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferPool,
    CVPixelBufferUnlockBaseAddress, kCVPixelBufferHeightKey, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelBufferWidthKey, kCVPixelFormatType_32BGRA, kCVReturnSuccess,
};
use objc2_foundation::{NSDictionary, NSMutableDictionary, NSNumber, NSString, NSURL};

use crate::ports::MediaWriter;

const AUDIO_SAMPLE_RATE: u32 = 48_000;
const AUDIO_CHANNELS: u32 = 2;
const AUDIO_BYTES_PER_FRAME: u32 = AUDIO_CHANNELS * size_of::<i16>() as u32;
const WRITER_READY_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) struct MacOsMp4Writer {
    writer: Retained<AVAssetWriter>,
    video_input: Retained<AVAssetWriterInput>,
    video_adaptor: Retained<AVAssetWriterInputPixelBufferAdaptor>,
    audio_input: Option<Retained<AVAssetWriterInput>>,
    audio_format: Option<CFRetained<CMFormatDescription>>,
    width: u32,
    height: u32,
    frames_per_second: u32,
    finished: bool,
}

impl MacOsMp4Writer {
    pub(super) fn create(
        path: &Path,
        width: u32,
        height: u32,
        frames_per_second: u32,
        include_audio: bool,
    ) -> Result<Self> {
        ensure!(width > 0 && height > 0, "MP4 dimensions must be positive");
        ensure!(
            frames_per_second > 0 && frames_per_second <= i32::MAX as u32,
            "MP4 frame rate is invalid"
        );
        ensure!(
            !path.exists(),
            "Apple AVAssetWriter requires a new output path: {}",
            path.display()
        );

        let path = path
            .to_str()
            .with_context(|| format!("The output path is not valid UTF-8: {}", path.display()))?;
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        let file_type = unsafe { AVFileTypeMPEG4 }
            .context("AVFoundation did not expose the MPEG-4 file type")?;
        let writer = unsafe { AVAssetWriter::assetWriterWithURL_fileType_error(&url, file_type) }
            .map_err(|error| {
            anyhow!(
                "Failed to create the Apple MP4 writer: {}",
                error.localizedDescription()
            )
        })?;

        let video_settings = video_settings(width, height)?;
        let video_media_type =
            unsafe { AVMediaTypeVideo }.context("AVFoundation did not expose video media type")?;
        let video_input = unsafe {
            AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
                video_media_type,
                Some(&video_settings),
            )
        };
        unsafe { video_input.setExpectsMediaDataInRealTime(true) };
        ensure!(
            unsafe { writer.canAddInput(&video_input) },
            "AVAssetWriter rejected the H.264 video input"
        );
        unsafe { writer.addInput(&video_input) };

        let pixel_attributes = pixel_buffer_attributes(width, height);
        let video_adaptor = unsafe {
            AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(
                &video_input,
                Some(&pixel_attributes),
            )
        };

        let (audio_input, audio_format) = if include_audio {
            let settings = audio_settings()?;
            let audio_media_type = unsafe { AVMediaTypeAudio }
                .context("AVFoundation did not expose audio media type")?;
            let input = unsafe {
                AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
                    audio_media_type,
                    Some(&settings),
                )
            };
            unsafe { input.setExpectsMediaDataInRealTime(true) };
            ensure!(
                unsafe { writer.canAddInput(&input) },
                "AVAssetWriter rejected the AAC audio input"
            );
            unsafe { writer.addInput(&input) };
            (Some(input), Some(create_pcm_format_description()?))
        } else {
            (None, None)
        };

        ensure!(
            unsafe { writer.startWriting() },
            "{}",
            writer_failure(&writer, "AVAssetWriter could not start")
        );
        unsafe { writer.startSessionAtSourceTime(CMTime::new(0, 1)) };

        Ok(Self {
            writer,
            video_input,
            video_adaptor,
            audio_input,
            audio_format,
            width,
            height,
            frames_per_second,
            finished: false,
        })
    }

    fn wait_until_ready(&self, input: &AVAssetWriterInput, kind: &str) -> Result<()> {
        let deadline = Instant::now() + WRITER_READY_TIMEOUT;
        loop {
            if unsafe { input.isReadyForMoreMediaData() } {
                return Ok(());
            }
            match unsafe { self.writer.status() } {
                AVAssetWriterStatus::Failed
                | AVAssetWriterStatus::Cancelled
                | AVAssetWriterStatus::Completed => {
                    return Err(writer_failure(
                        &self.writer,
                        &format!("AVAssetWriter stopped while waiting for {kind}"),
                    ));
                }
                _ if Instant::now() >= deadline => {
                    return Err(anyhow!(
                        "Timed out waiting for AVAssetWriter to accept {kind} data"
                    ));
                }
                _ => thread::sleep(Duration::from_millis(1)),
            }
        }
    }

    fn pixel_buffer(&self, bgra: &[u8]) -> Result<CFRetained<CVPixelBuffer>> {
        let expected = self.width as usize * self.height as usize * 4;
        ensure!(
            bgra.len() == expected,
            "Unexpected BGRA frame size: expected {expected}, got {}",
            bgra.len()
        );
        let pool = unsafe { self.video_adaptor.pixelBufferPool() }
            .context("AVAssetWriter did not create a video pixel buffer pool")?;
        let mut raw = null_mut();
        let status =
            unsafe { CVPixelBufferPool::create_pixel_buffer(None, &pool, NonNull::from(&mut raw)) };
        ensure!(
            status == kCVReturnSuccess,
            "CVPixelBufferPool failed with status {status}"
        );
        let raw = NonNull::new(raw).context("CVPixelBufferPool returned a null buffer")?;
        let buffer = unsafe { CFRetained::from_raw(raw) };

        let flags = CVPixelBufferLockFlags::empty();
        let lock_status = unsafe { CVPixelBufferLockBaseAddress(&buffer, flags) };
        ensure!(
            lock_status == kCVReturnSuccess,
            "CVPixelBuffer lock failed with status {lock_status}"
        );

        let copy_result = (|| {
            let destination = CVPixelBufferGetBaseAddress(&buffer).cast::<u8>();
            ensure!(
                !destination.is_null(),
                "CVPixelBuffer returned a null base address"
            );
            let source_stride = self.width as usize * 4;
            let destination_stride = CVPixelBufferGetBytesPerRow(&buffer);
            ensure!(
                destination_stride >= source_stride,
                "CVPixelBuffer row stride is smaller than the BGRA source"
            );
            for row in 0..self.height as usize {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        bgra.as_ptr().add(row * source_stride),
                        destination.add(row * destination_stride),
                        source_stride,
                    );
                }
            }
            Ok(())
        })();

        let unlock_status = unsafe { CVPixelBufferUnlockBaseAddress(&buffer, flags) };
        ensure!(
            unlock_status == kCVReturnSuccess,
            "CVPixelBuffer unlock failed with status {unlock_status}"
        );
        copy_result?;
        Ok(buffer)
    }
}

impl MediaWriter for MacOsMp4Writer {
    fn write_video(&mut self, frame_index: u64, bgra: &[u8]) -> Result<()> {
        self.wait_until_ready(&self.video_input, "video")?;
        let buffer = self.pixel_buffer(bgra)?;
        let frame_index = i64::try_from(frame_index).context("Video timestamp overflow")?;
        let timestamp = unsafe { CMTime::new(frame_index, self.frames_per_second as i32) };
        ensure!(
            unsafe {
                self.video_adaptor
                    .appendPixelBuffer_withPresentationTime(&buffer, timestamp)
            },
            "{}",
            writer_failure(&self.writer, "AVAssetWriter rejected a video frame")
        );
        Ok(())
    }

    fn write_audio(&mut self, start_frame: u64, pcm: &[i16]) -> Result<()> {
        let Some(input) = self.audio_input.as_ref() else {
            ensure!(
                pcm.is_empty(),
                "Audio data was provided to a video-only writer"
            );
            return Ok(());
        };
        if pcm.is_empty() {
            return Ok(());
        }
        ensure!(
            pcm.len().is_multiple_of(AUDIO_CHANNELS as usize),
            "Audio PCM is not aligned to stereo frames"
        );
        self.wait_until_ready(input, "audio")?;

        let byte_length = size_of_val(pcm);
        let mut block_raw = null_mut();
        let status = unsafe {
            CMBlockBuffer::create_with_memory_block(
                None,
                null_mut(),
                byte_length,
                None,
                null(),
                0,
                byte_length,
                kCMBlockBufferAssureMemoryNowFlag,
                NonNull::from(&mut block_raw),
            )
        };
        ensure!(
            status == kCMBlockBufferNoErr,
            "CMBlockBuffer allocation failed with status {status}"
        );
        let block_raw = NonNull::new(block_raw).context("CMBlockBuffer returned a null buffer")?;
        let block = unsafe { CFRetained::from_raw(block_raw) };
        let source = NonNull::new(pcm.as_ptr().cast_mut().cast::<c_void>())
            .context("Audio PCM returned a null pointer")?;
        let status = unsafe { CMBlockBuffer::replace_data_bytes(source, &block, 0, byte_length) };
        ensure!(
            status == kCMBlockBufferNoErr,
            "CMBlockBuffer copy failed with status {status}"
        );

        let start_frame = i64::try_from(start_frame).context("Audio timestamp overflow")?;
        let timing = CMSampleTimingInfo {
            duration: unsafe { CMTime::new(1, AUDIO_SAMPLE_RATE as i32) },
            presentationTimeStamp: unsafe { CMTime::new(start_frame, AUDIO_SAMPLE_RATE as i32) },
            decodeTimeStamp: unsafe { kCMTimeInvalid },
        };
        let sample_frames = pcm.len() / AUDIO_CHANNELS as usize;
        let sample_size = AUDIO_BYTES_PER_FRAME as usize;
        let mut sample_raw = null_mut();
        let format = self
            .audio_format
            .as_deref()
            .context("The PCM audio format description is missing")?;
        let status = unsafe {
            CMSampleBuffer::create_ready(
                None,
                Some(&block),
                Some(format),
                sample_frames as _,
                1,
                &timing,
                1,
                &sample_size,
                NonNull::from(&mut sample_raw),
            )
        };
        ensure!(
            status == 0,
            "CMSampleBuffer creation failed with status {status}"
        );
        let sample_raw =
            NonNull::new(sample_raw).context("CMSampleBuffer returned a null buffer")?;
        let sample = unsafe { CFRetained::from_raw(sample_raw) };
        ensure!(
            unsafe { input.appendSampleBuffer(&sample) },
            "{}",
            writer_failure(&self.writer, "AVAssetWriter rejected an audio buffer")
        );
        Ok(())
    }

    fn finalize(mut self: Box<Self>) -> Result<()> {
        unsafe { self.video_input.markAsFinished() };
        if let Some(input) = self.audio_input.as_ref() {
            unsafe { input.markAsFinished() };
        }

        let (sender, receiver) = mpsc::channel();
        let completion = RcBlock::<dyn Fn()>::new(move || {
            let _ = sender.send(());
        });
        unsafe { self.writer.finishWritingWithCompletionHandler(&completion) };
        receiver
            .recv()
            .context("AVAssetWriter completion handler was disconnected")?;
        ensure!(
            unsafe { self.writer.status() } == AVAssetWriterStatus::Completed,
            "{}",
            writer_failure(&self.writer, "AVAssetWriter did not finish successfully")
        );
        self.finished = true;
        Ok(())
    }
}

impl Drop for MacOsMp4Writer {
    fn drop(&mut self) {
        if !self.finished {
            unsafe { self.writer.cancelWriting() };
        }
    }
}

fn video_settings(width: u32, height: u32) -> Result<Retained<NSDictionary<NSString, AnyObject>>> {
    let codec_key =
        unsafe { AVVideoCodecKey }.context("AVFoundation did not expose AVVideoCodecKey")?;
    let width_key =
        unsafe { AVVideoWidthKey }.context("AVFoundation did not expose AVVideoWidthKey")?;
    let height_key =
        unsafe { AVVideoHeightKey }.context("AVFoundation did not expose AVVideoHeightKey")?;
    let codec =
        unsafe { AVVideoCodecTypeH264 }.context("AVFoundation did not expose H.264 codec")?;
    let width = NSNumber::new_u32(width);
    let height = NSNumber::new_u32(height);

    let settings = NSMutableDictionary::<NSString, AnyObject>::new();
    settings.insert(codec_key, codec.as_ref());
    settings.insert(width_key, width.as_ref());
    settings.insert(height_key, height.as_ref());
    Ok((&settings).into())
}

fn audio_settings() -> Result<Retained<NSDictionary<NSString, AnyObject>>> {
    let format_key = unsafe { AVFormatIDKey }.context("AVFAudio did not expose AVFormatIDKey")?;
    let sample_rate_key =
        unsafe { AVSampleRateKey }.context("AVFAudio did not expose AVSampleRateKey")?;
    let channels_key = unsafe { AVNumberOfChannelsKey }
        .context("AVFAudio did not expose AVNumberOfChannelsKey")?;
    let bit_rate_key =
        unsafe { AVEncoderBitRateKey }.context("AVFAudio did not expose AVEncoderBitRateKey")?;
    let format = NSNumber::new_u32(kAudioFormatMPEG4AAC);
    let sample_rate = NSNumber::new_f64(AUDIO_SAMPLE_RATE as f64);
    let channels = NSNumber::new_u32(AUDIO_CHANNELS);
    let bit_rate = NSNumber::new_u32(192_000);

    let settings = NSMutableDictionary::<NSString, AnyObject>::new();
    settings.insert(format_key, format.as_ref());
    settings.insert(sample_rate_key, sample_rate.as_ref());
    settings.insert(channels_key, channels.as_ref());
    settings.insert(bit_rate_key, bit_rate.as_ref());
    Ok((&settings).into())
}

fn pixel_buffer_attributes(width: u32, height: u32) -> Retained<NSDictionary<NSString, AnyObject>> {
    let pixel_format = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
    let width = NSNumber::new_u32(width);
    let height = NSNumber::new_u32(height);
    let pixel_format_key: &NSString = unsafe { kCVPixelBufferPixelFormatTypeKey }.as_ref();
    let width_key: &NSString = unsafe { kCVPixelBufferWidthKey }.as_ref();
    let height_key: &NSString = unsafe { kCVPixelBufferHeightKey }.as_ref();

    let attributes = NSMutableDictionary::<NSString, AnyObject>::new();
    attributes.insert(pixel_format_key, pixel_format.as_ref());
    attributes.insert(width_key, width.as_ref());
    attributes.insert(height_key, height.as_ref());
    (&attributes).into()
}

fn create_pcm_format_description() -> Result<CFRetained<CMFormatDescription>> {
    let mut description = null();
    let mut format = AudioStreamBasicDescription {
        mSampleRate: AUDIO_SAMPLE_RATE as f64,
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked,
        mBytesPerPacket: AUDIO_BYTES_PER_FRAME,
        mFramesPerPacket: 1,
        mBytesPerFrame: AUDIO_BYTES_PER_FRAME,
        mChannelsPerFrame: AUDIO_CHANNELS,
        mBitsPerChannel: 16,
        mReserved: 0,
    };
    let status = unsafe {
        CMAudioFormatDescriptionCreate(
            None,
            NonNull::from(&mut format),
            0,
            null(),
            0,
            null(),
            None,
            NonNull::from(&mut description),
        )
    };
    ensure!(
        status == 0,
        "PCM format description creation failed with status {status}"
    );
    let description = NonNull::new(description.cast_mut())
        .context("CMAudioFormatDescriptionCreate returned null")?;
    Ok(unsafe { CFRetained::from_raw(description) })
}

fn writer_failure(writer: &AVAssetWriter, context: &str) -> anyhow::Error {
    let detail = unsafe { writer.error() }
        .map(|error| error.localizedDescription().to_string())
        .unwrap_or_else(|| format!("status {:?}", unsafe { writer.status() }));
    anyhow!("{context}: {detail}")
}
