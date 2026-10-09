use std::{thread, time::Duration};

use windows::{
    Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::OcrEngine as NativeOcrEngine,
    Storage::Streams::DataWriter,
    Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
};

use crate::{
    image::CapturedImage,
    platform::ocr::{OcrEngine, OcrFailure},
};

pub struct WindowsOcrEngine;

impl OcrEngine for WindowsOcrEngine {
    fn availability(&self) -> Result<(), OcrFailure> {
        probe().map_err(classify_error)
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
    let pixels = rgba_to_bgra(image.rgba_bytes());
    writer.WriteBytes(&pixels)?;
    let buffer = writer.DetachBuffer()?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        image.width() as i32,
        image.height() as i32,
        BitmapAlphaMode::Ignore,
    )?;
    let operation = engine.RecognizeAsync(&bitmap)?;
    // AsyncStatus::Started is 0; windows-future does not re-export the enum through windows.
    while operation.Status()?.0 == 0 {
        thread::sleep(Duration::from_millis(5));
    }
    let result = operation.GetResults()?;
    Ok(result.Text()?.to_string())
}

fn rgba_to_bgra(mut pixels: Vec<u8>) -> Vec<u8> {
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    pixels
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

#[cfg(test)]
mod tests {
    use super::rgba_to_bgra;

    #[test]
    fn ocr_pixels_are_converted_to_opaque_bgra() {
        assert_eq!(
            rgba_to_bgra(vec![10, 20, 30, 40, 50, 60, 70, 80]),
            vec![30, 20, 10, 255, 70, 60, 50, 255]
        );
    }
}
