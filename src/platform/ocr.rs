use crate::image::CapturedImage;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OcrFailure {
    MissingLanguagePack,
    Unsupported,
    Failed(String),
}

pub trait OcrEngine {
    fn is_available(&self) -> bool;
    fn recognize(&self, image: &CapturedImage) -> Result<String, OcrFailure>;
}

#[cfg(windows)]
pub fn system_engine() -> impl OcrEngine {
    super::windows::ocr::WindowsOcrEngine
}
