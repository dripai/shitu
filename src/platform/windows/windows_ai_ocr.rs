use std::{thread, time::Duration};

use windows::{
    Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap},
    Storage::Streams::DataWriter,
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
};

use super::windows_ai_bindings::Microsoft::{
    Graphics::Imaging::ImageBuffer,
    Windows::AI::{
        AIFeatureReadyResultState, AIFeatureReadyState,
        Imaging::{RecognizedLine, TextRecognizer},
    },
};
use crate::{
    image::CapturedImage,
    platform::ocr::{AiOcrState, OcrFailure},
};

pub fn availability() -> Result<AiOcrState, OcrFailure> {
    let _apartment = Apartment::initialize().map_err(classify_error)?;
    let state = TextRecognizer::GetReadyState().map_err(classify_error)?;
    Ok(match state {
        AIFeatureReadyState::Ready => AiOcrState::Ready,
        AIFeatureReadyState::NotReady => AiOcrState::ModelNotInstalled,
        AIFeatureReadyState::NotSupportedOnCurrentSystem => AiOcrState::Unsupported,
        AIFeatureReadyState::DisabledByUser => AiOcrState::DisabledByUser,
        other => AiOcrState::Failed(format!("未知 Windows AI 状态：{}", other.0)),
    })
}

pub fn recognize(image: &CapturedImage, minimum_confidence: u8) -> Result<String, OcrFailure> {
    match availability()? {
        AiOcrState::Ready => {}
        state => return Err(OcrFailure::AiUnavailable(state)),
    }

    let writer = DataWriter::new().map_err(classify_error)?;
    let pixels = rgba_to_bgra(image.rgba_bytes());
    writer.WriteBytes(&pixels).map_err(classify_error)?;
    let buffer = writer.DetachBuffer().map_err(classify_error)?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        image.width() as i32,
        image.height() as i32,
        BitmapAlphaMode::Ignore,
    )
    .map_err(classify_error)?;
    let image_buffer = ImageBuffer::CreateForSoftwareBitmap(&bitmap).map_err(classify_error)?;
    let create = TextRecognizer::CreateAsync().map_err(classify_error)?;
    while create.Status().map_err(classify_error)?.0 == 0 {
        thread::sleep(Duration::from_millis(5));
    }
    let recognizer = create.GetResults().map_err(classify_error)?;
    let result = recognizer
        .RecognizeTextFromImage(&image_buffer)
        .map_err(classify_error)?;
    let threshold = minimum_confidence.clamp(0, 100) as f32 / 100.0;
    let lines = result.Lines().map_err(classify_error)?;
    let mut filtered = Vec::new();
    for line in lines.iter().flatten() {
        if let Some(line) = filtered_line(line, threshold)? {
            filtered.push(line);
        }
    }
    let text = filtered.join("\n");
    Ok(text)
}

pub fn prepare() -> Result<AiOcrState, OcrFailure> {
    match availability()? {
        AiOcrState::Ready => return Ok(AiOcrState::Ready),
        AiOcrState::ModelNotInstalled => {}
        state => return Err(OcrFailure::AiUnavailable(state)),
    }
    let operation = TextRecognizer::EnsureReadyAsync().map_err(classify_error)?;
    while operation.Status().map_err(classify_error)?.0 == 0 {
        thread::sleep(Duration::from_millis(50));
    }
    let result = operation.GetResults().map_err(classify_error)?;
    if result.Status().map_err(classify_error)? != AIFeatureReadyResultState::Success {
        let message = result
            .ErrorDisplayText()
            .map(|value| value.to_string())
            .unwrap_or_else(|_| "Windows AI OCR 模型准备失败".to_owned());
        return Err(OcrFailure::Failed(message));
    }
    availability()
}

fn filtered_line(line: &RecognizedLine, threshold: f32) -> Result<Option<String>, OcrFailure> {
    let words = line.Words().map_err(classify_error)?;
    let mut kept = Vec::new();
    for word in words.iter().flatten() {
        if word.MatchConfidence().map_err(classify_error)? >= threshold {
            kept.push(word.Text().map_err(classify_error)?.to_string());
        }
    }
    if kept.is_empty() {
        return Ok(None);
    }
    Ok(Some(join_words(&kept)))
}

fn join_words(words: &[String]) -> String {
    let mut result = String::new();
    for word in words {
        if !result.is_empty()
            && !result.chars().last().is_some_and(is_cjk)
            && !word.chars().next().is_some_and(is_cjk)
        {
            result.push(' ');
        }
        result.push_str(word);
    }
    result
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

fn rgba_to_bgra(mut pixels: Vec<u8>) -> Vec<u8> {
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    pixels
}

fn classify_error(error: windows_core::Error) -> OcrFailure {
    let message = error.message().to_string();
    let lower = message.to_ascii_lowercase();
    const REGDB_E_CLASSNOTREG: windows_core::HRESULT =
        windows_core::HRESULT(0x8004_0154_u32 as i32);
    if error.code() == REGDB_E_CLASSNOTREG
        || lower.contains("class not registered")
        || message.contains("没有注册类")
        || lower.contains("package")
    {
        OcrFailure::AiUnavailable(AiOcrState::ComponentMissing)
    } else {
        OcrFailure::Failed(message)
    }
}

struct Apartment;

impl Apartment {
    fn initialize() -> windows_core::Result<Self> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

#[cfg(test)]
mod tests {
    use super::join_words;

    #[test]
    fn word_joining_preserves_cjk_and_spaces_latin_words() {
        assert_eq!(join_words(&["截图".into(), "工具".into()]), "截图工具");
        assert_eq!(join_words(&["hello".into(), "world".into()]), "hello world");
    }
}
