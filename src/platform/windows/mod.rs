pub mod clipboard;
pub mod clock;
pub mod file;
pub mod ocr;
pub mod screen_capture;
pub mod shell;
pub mod startup;
pub mod window;
pub mod window_target;
mod windows_ai_bindings;
pub mod windows_ai_ocr;

use std::mem::size_of;

use windows::Win32::Graphics::Gdi::{BI_RGB, BITMAPINFO, BITMAPINFOHEADER};

fn bitmap_info(width: i32, height: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    }
}
