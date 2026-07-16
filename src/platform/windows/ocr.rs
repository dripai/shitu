use windows::{
    Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::OcrEngine as NativeOcrEngine,
    Storage::Streams::DataWriter,
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
};

use crate::{
    capture::CapturedImage,
    platform::ocr::{OcrEngine, OcrFailure},
};

pub struct WindowsOcrEngine;

impl OcrEngine for WindowsOcrEngine {
    fn is_available(&self) -> bool {
        probe().is_ok()
    }

    fn recognize(&self, image: &CapturedImage) -> Result<String, OcrFailure> {
        recognize_impl(image).map_err(classify_error)
    }
}

fn probe() -> windows::core::Result<()> {
    let _apartment = Apartment::initialize()?;
    NativeOcrEngine::TryCreateFromUserProfileLanguages().map(|_| ())
}

fn recognize_impl(image: &CapturedImage) -> windows::core::Result<String> {
    let _apartment = Apartment::initialize()?;
    let engine = NativeOcrEngine::TryCreateFromUserProfileLanguages()?;
    let writer = DataWriter::new()?;
    writer.WriteBytes(&image.rgba_bytes())?;
    let buffer = writer.DetachBuffer()?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
        &buffer,
        BitmapPixelFormat::Rgba8,
        image.width() as i32,
        image.height() as i32,
    )?;
    let result = engine.RecognizeAsync(&bitmap)?.join()?;
    Ok(result.Text()?.to_string())
}

fn classify_error(error: windows::core::Error) -> OcrFailure {
    let message = error.message().to_string();
    let lower = message.to_ascii_lowercase();
    if lower.contains("language") || lower.contains("语言") {
        OcrFailure::MissingLanguagePack
    } else if lower.contains("package")
        || lower.contains("identity")
        || lower.contains("class not registered")
        || lower.contains("illegal method call")
    {
        OcrFailure::Unsupported
    } else {
        OcrFailure::Failed(message)
    }
}

struct Apartment;

impl Apartment {
    fn initialize() -> windows::core::Result<Self> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}
