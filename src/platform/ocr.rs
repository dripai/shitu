use crate::image::CapturedImage;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OcrFailure {
    MissingLanguagePack,
    Unsupported,
    Failed(String),
}

pub trait OcrEngine {
    fn availability(&self) -> Result<(), OcrFailure>;
    fn recognize(&self, image: &CapturedImage) -> Result<String, OcrFailure>;
}

#[cfg(windows)]
pub fn system_engine() -> impl OcrEngine {
    super::windows::ocr::WindowsOcrEngine
}

#[cfg(windows)]
pub fn system_availability() -> Result<(), OcrFailure> {
    system_engine().availability()
}

#[cfg(windows)]
pub fn recognize_system(image: &CapturedImage) -> Result<String, OcrFailure> {
    system_engine().recognize(image)
}

impl OcrFailure {
    pub fn message(&self) -> String {
        match self {
            Self::MissingLanguagePack => "缺少可用的 Windows OCR 语言包".to_owned(),
            Self::Unsupported => "当前系统或程序安装方式不支持 Windows 系统 OCR".to_owned(),
            Self::Failed(message) if message.trim().is_empty() => "OCR 识别失败".to_owned(),
            Self::Failed(message) => message.clone(),
        }
    }
}
