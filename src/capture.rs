use anyhow::Result;
#[cfg(not(windows))]
use anyhow::anyhow;

use crate::image::CapturedImage;

#[cfg(windows)]
pub fn capture_virtual_desktop() -> Result<CapturedImage> {
    crate::platform::windows::screen_capture::capture_virtual_desktop()
}

#[cfg(not(windows))]
pub fn capture_virtual_desktop() -> Result<CapturedImage> {
    Err(anyhow!("当前平台尚未实现屏幕截图"))
}

#[cfg(windows)]
pub fn copy_to_clipboard(image: &CapturedImage) -> Result<()> {
    crate::platform::windows::clipboard::copy_image(image)
}

#[cfg(not(windows))]
pub fn copy_to_clipboard(_image: &CapturedImage) -> Result<()> {
    Err(anyhow!("当前平台尚未实现图像剪贴板"))
}

#[cfg(windows)]
pub fn copy_text_to_clipboard(text: &str) -> Result<()> {
    crate::platform::windows::clipboard::copy_text(text)
}

#[cfg(not(windows))]
pub fn copy_text_to_clipboard(_text: &str) -> Result<()> {
    Err(anyhow!("当前平台尚未实现文字剪贴板"))
}

#[cfg(windows)]
pub fn image_from_clipboard(left: i32, top: i32) -> Result<CapturedImage> {
    crate::platform::windows::clipboard::read_image(left, top)
}

#[cfg(not(windows))]
pub fn image_from_clipboard(_left: i32, _top: i32) -> Result<CapturedImage> {
    Err(anyhow!("当前平台尚未实现图像剪贴板"))
}
