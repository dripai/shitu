use std::collections::VecDeque;

use anyhow::{Context, Result, anyhow};
use windows::Win32::{
    Media::{
        Audio::{
            AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
            IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
            WAVE_FORMAT_PCM, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, eCapture, eConsole, eRender,
        },
        KernelStreaming::{KSDATAFORMAT_SUBTYPE_PCM, WAVE_FORMAT_EXTENSIBLE},
        Multimedia::{KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, WAVE_FORMAT_IEEE_FLOAT},
    },
    System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree},
};

use crate::encoder::{AUDIO_CHANNELS, AUDIO_SAMPLE_RATE};

const MAX_QUEUED_FRAMES: usize = AUDIO_SAMPLE_RATE as usize * 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    System,
    Microphone,
}

pub struct AudioSources {
    system: Option<AudioCapture>,
    microphone: Option<AudioCapture>,
    system_error: Option<String>,
    microphone_error: Option<String>,
}

impl AudioSources {
    pub fn initialize() -> Self {
        let (system, system_error) = initialize_source(SourceKind::System);
        let (microphone, microphone_error) = initialize_source(SourceKind::Microphone);
        Self {
            system,
            microphone,
            system_error,
            microphone_error,
        }
    }

    pub fn system_available(&self) -> bool {
        self.system.is_some()
    }

    pub fn microphone_available(&self) -> bool {
        self.microphone.is_some()
    }

    pub fn error(&self, kind: SourceKind) -> Option<&str> {
        match kind {
            SourceKind::System => self.system_error.as_deref(),
            SourceKind::Microphone => self.microphone_error.as_deref(),
        }
    }

    pub fn has_any_source(&self) -> bool {
        self.system.is_some() || self.microphone.is_some()
    }

    pub fn pump(&mut self) -> Result<()> {
        if let Some(source) = &mut self.system {
            source.read_available().context("读取系统声音失败")?;
        }
        if let Some(source) = &mut self.microphone {
            source.read_available().context("读取麦克风失败")?;
        }
        Ok(())
    }

    pub fn discard(&mut self) {
        if let Some(source) = &mut self.system {
            source.queue.clear();
        }
        if let Some(source) = &mut self.microphone {
            source.queue.clear();
        }
    }

    pub fn mix(
        &mut self,
        frames: usize,
        system_enabled: bool,
        microphone_enabled: bool,
    ) -> Vec<i16> {
        let mut output = Vec::with_capacity(frames * AUDIO_CHANNELS as usize);
        for _ in 0..frames {
            let system = self
                .system
                .as_mut()
                .and_then(|source| source.queue.pop_front())
                .unwrap_or([0.0, 0.0]);
            let microphone = self
                .microphone
                .as_mut()
                .and_then(|source| source.queue.pop_front())
                .unwrap_or([0.0, 0.0]);
            for channel in 0..2 {
                let mut value = 0.0_f32;
                if system_enabled {
                    value += system[channel];
                }
                if microphone_enabled {
                    value += microphone[channel];
                }
                value = value.clamp(-1.0, 1.0);
                output.push((value * i16::MAX as f32).round() as i16);
            }
        }
        output
    }
}

fn initialize_source(kind: SourceKind) -> (Option<AudioCapture>, Option<String>) {
    match AudioCapture::new(kind) {
        Ok(source) => (Some(source), None),
        Err(error) => (None, Some(error.to_string())),
    }
}

struct AudioCapture {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    format: InputFormat,
    resampler: LinearResampler,
    queue: VecDeque<[f32; 2]>,
}

impl AudioCapture {
    fn new(kind: SourceKind) -> Result<Self> {
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .context("创建音频设备枚举器失败")?;
        let (flow, flags) = match kind {
            SourceKind::System => (eRender, AUDCLNT_STREAMFLAGS_LOOPBACK),
            SourceKind::Microphone => (eCapture, 0),
        };
        let device = unsafe { enumerator.GetDefaultAudioEndpoint(flow, eConsole) }
            .context("没有可用的默认音频设备")?;
        let client: IAudioClient =
            unsafe { device.Activate(CLSCTX_ALL, None) }.context("激活音频设备失败")?;
        let wave_pointer = unsafe { client.GetMixFormat() }.context("读取音频混合格式失败")?;
        if wave_pointer.is_null() {
            return Err(anyhow!("音频设备返回了空格式"));
        }
        let format = unsafe { InputFormat::from_wave_format(wave_pointer) }?;
        let initialize_result = unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                flags,
                1_000_000,
                0,
                wave_pointer,
                None,
            )
        };
        unsafe {
            CoTaskMemFree(Some(wave_pointer.cast()));
        }
        initialize_result.context("初始化共享音频采集流失败")?;
        let capture: IAudioCaptureClient =
            unsafe { client.GetService() }.context("获取音频采集服务失败")?;
        unsafe { client.Start() }.context("启动音频采集失败")?;
        Ok(Self {
            client,
            capture,
            format,
            resampler: LinearResampler::new(format.sample_rate),
            queue: VecDeque::new(),
        })
    }

    fn read_available(&mut self) -> Result<()> {
        loop {
            let packet_frames =
                unsafe { self.capture.GetNextPacketSize() }.context("读取音频包大小失败")?;
            if packet_frames == 0 {
                break;
            }
            let mut data = std::ptr::null_mut();
            let mut frames = 0_u32;
            let mut flags = 0_u32;
            unsafe {
                self.capture
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
            }
            .context("锁定音频采集缓冲区失败")?;
            let parsed = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 {
                Ok(vec![[0.0, 0.0]; frames as usize])
            } else if data.is_null() {
                Err(anyhow!("音频采集缓冲区为空"))
            } else {
                let byte_count = frames as usize * self.format.block_align as usize;
                let bytes = unsafe { std::slice::from_raw_parts(data, byte_count) };
                self.format.decode(bytes, frames as usize)
            };
            let release_result = unsafe { self.capture.ReleaseBuffer(frames) };
            release_result.context("释放音频采集缓冲区失败")?;
            let decoded = parsed?;
            self.resampler.push(&decoded, &mut self.queue);
            while self.queue.len() > MAX_QUEUED_FRAMES {
                self.queue.pop_front();
            }
        }
        Ok(())
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        let _ = unsafe { self.client.Stop() };
    }
}

#[derive(Clone, Copy)]
enum SampleFormat {
    Float32,
    Pcm16,
    Pcm24,
    Pcm32,
}

#[derive(Clone, Copy)]
struct InputFormat {
    channels: u16,
    sample_rate: u32,
    block_align: u16,
    bytes_per_sample: usize,
    sample_format: SampleFormat,
}

impl InputFormat {
    unsafe fn from_wave_format(pointer: *const WAVEFORMATEX) -> Result<Self> {
        let wave = unsafe { *pointer };
        let format_tag = wave.wFormatTag as u32;
        let channels = wave.nChannels;
        let sample_rate = wave.nSamplesPerSec;
        let block_align = wave.nBlockAlign;
        let bits = wave.wBitsPerSample;
        if channels == 0 || sample_rate == 0 || block_align == 0 {
            return Err(anyhow!("音频设备格式无效"));
        }
        let subtype = if format_tag == WAVE_FORMAT_EXTENSIBLE {
            let extended = unsafe { *(pointer.cast::<WAVEFORMATEXTENSIBLE>()) };
            Some(extended.SubFormat)
        } else {
            None
        };
        let is_float = format_tag == WAVE_FORMAT_IEEE_FLOAT
            || subtype == Some(KSDATAFORMAT_SUBTYPE_IEEE_FLOAT);
        let is_pcm = format_tag == WAVE_FORMAT_PCM || subtype == Some(KSDATAFORMAT_SUBTYPE_PCM);
        let sample_format = if is_float && bits == 32 {
            SampleFormat::Float32
        } else if is_pcm && bits == 16 {
            SampleFormat::Pcm16
        } else if is_pcm && bits == 24 {
            SampleFormat::Pcm24
        } else if is_pcm && bits == 32 {
            SampleFormat::Pcm32
        } else {
            return Err(anyhow!(
                "不支持的音频设备格式：tag={format_tag}, bits={bits}"
            ));
        };
        let bytes_per_sample = (bits as usize).div_ceil(8);
        Ok(Self {
            channels,
            sample_rate,
            block_align,
            bytes_per_sample,
            sample_format,
        })
    }

    fn decode(&self, bytes: &[u8], frames: usize) -> Result<Vec<[f32; 2]>> {
        if bytes.len() < frames * self.block_align as usize {
            return Err(anyhow!("音频采集数据长度不足"));
        }
        let mut output = Vec::with_capacity(frames);
        for frame_index in 0..frames {
            let frame_start = frame_index * self.block_align as usize;
            let channel = |index: usize| -> f32 {
                if index >= self.channels as usize {
                    return 0.0;
                }
                let start = frame_start + index * self.bytes_per_sample;
                decode_sample(
                    &bytes[start..start + self.bytes_per_sample],
                    self.sample_format,
                )
            };
            let sample = if self.channels == 1 {
                let value = channel(0);
                [value, value]
            } else {
                let mut left = channel(0);
                let mut right = channel(1);
                if self.channels > 2 {
                    let center = channel(2) * 0.5;
                    left += center;
                    right += center;
                }
                [left.clamp(-1.0, 1.0), right.clamp(-1.0, 1.0)]
            };
            output.push(sample);
        }
        Ok(output)
    }
}

fn decode_sample(bytes: &[u8], format: SampleFormat) -> f32 {
    match format {
        SampleFormat::Float32 => f32::from_le_bytes(bytes.try_into().unwrap()).clamp(-1.0, 1.0),
        SampleFormat::Pcm16 => i16::from_le_bytes(bytes.try_into().unwrap()) as f32 / 32_768.0,
        SampleFormat::Pcm24 => {
            let raw = (bytes[0] as i32) | ((bytes[1] as i32) << 8) | ((bytes[2] as i32) << 16);
            let signed = if raw & 0x0080_0000 != 0 {
                raw | !0x00ff_ffff
            } else {
                raw
            };
            signed as f32 / 8_388_608.0
        }
        SampleFormat::Pcm32 => {
            i32::from_le_bytes(bytes.try_into().unwrap()) as f32 / 2_147_483_648.0
        }
    }
}

struct LinearResampler {
    step: f64,
    position: f64,
    pending: Vec<[f32; 2]>,
}

impl LinearResampler {
    fn new(input_rate: u32) -> Self {
        Self {
            step: input_rate as f64 / AUDIO_SAMPLE_RATE as f64,
            position: 0.0,
            pending: Vec::new(),
        }
    }

    fn push(&mut self, input: &[[f32; 2]], output: &mut VecDeque<[f32; 2]>) {
        self.pending.extend_from_slice(input);
        while self.position + 1.0 < self.pending.len() as f64 {
            let left = self.position.floor() as usize;
            let fraction = (self.position - left as f64) as f32;
            let first = self.pending[left];
            let second = self.pending[left + 1];
            output.push_back([
                first[0] + (second[0] - first[0]) * fraction,
                first[1] + (second[1] - first[1]) * fraction,
            ]);
            self.position += self.step;
        }
        let discard = self.position.floor() as usize;
        if discard > 0 {
            self.pending.drain(..discard.min(self.pending.len()));
            self.position -= discard as f64;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::{LinearResampler, SampleFormat, decode_sample};

    #[test]
    fn pcm_samples_are_normalized() {
        assert_eq!(
            decode_sample(&i16::MAX.to_le_bytes(), SampleFormat::Pcm16),
            i16::MAX as f32 / 32768.0
        );
        assert_eq!(
            decode_sample(&i16::MIN.to_le_bytes(), SampleFormat::Pcm16),
            -1.0
        );
    }

    #[test]
    fn resampler_converts_44100_to_48000() {
        let mut resampler = LinearResampler::new(44_100);
        let input = vec![[0.25, -0.25]; 4_410];
        let mut output = VecDeque::new();
        resampler.push(&input, &mut output);
        assert!((output.len() as i32 - 4_800).abs() <= 2);
    }
}
