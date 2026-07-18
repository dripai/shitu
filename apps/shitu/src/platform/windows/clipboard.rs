use std::{mem::size_of, ptr};

use anyhow::{Result, anyhow};
use windows::Win32::{
    Foundation::{GlobalFree, HANDLE, HGLOBAL},
    Graphics::Gdi::BITMAPINFOHEADER,
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
            OpenClipboard, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
    },
};

use crate::image::CapturedImage;

const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;

pub fn copy_image(image: &CapturedImage) -> Result<()> {
    unsafe { write_image(image) }
}

pub fn copy_text(text: &str) -> Result<()> {
    unsafe { write_text(text) }
}

pub fn read_image(left: i32, top: i32) -> Result<CapturedImage> {
    unsafe { read_image_impl(left, top) }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn write_image(image: &CapturedImage) -> Result<()> {
    let bitmap_info = super::bitmap_info(image.bounds.width, -image.bounds.height);
    let rgba = image.rgba_bytes();
    let pixel_bytes = rgba.len();
    let dib_bytes = size_of::<BITMAPINFOHEADER>() + pixel_bytes;
    let allocation = GlobalAlloc(GMEM_MOVEABLE, dib_bytes)?;
    let target = GlobalLock(allocation);
    if target.is_null() {
        let _ = GlobalFree(Some(allocation));
        return Err(anyhow!("GlobalLock 失败"));
    }

    ptr::copy_nonoverlapping(
        (&bitmap_info.bmiHeader as *const BITMAPINFOHEADER).cast::<u8>(),
        target.cast::<u8>(),
        size_of::<BITMAPINFOHEADER>(),
    );
    let target_pixels = std::slice::from_raw_parts_mut(
        target.cast::<u8>().add(size_of::<BITMAPINFOHEADER>()),
        pixel_bytes,
    );
    for (source, target) in rgba.chunks_exact(4).zip(target_pixels.chunks_exact_mut(4)) {
        target.copy_from_slice(&[source[2], source[1], source[0], source[3]]);
    }
    let _ = GlobalUnlock(allocation);

    if let Err(error) = OpenClipboard(None) {
        let _ = GlobalFree(Some(allocation));
        return Err(error.into());
    }
    let result = (|| -> windows::core::Result<()> {
        EmptyClipboard()?;
        SetClipboardData(CF_DIB, Some(HANDLE(allocation.0)))?;
        Ok(())
    })();
    let _ = CloseClipboard();
    if let Err(error) = result {
        let _ = GlobalFree(Some(allocation));
        return Err(error.into());
    }
    Ok(())
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn write_text(text: &str) -> Result<()> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let byte_len = wide.len() * size_of::<u16>();
    let allocation = GlobalAlloc(GMEM_MOVEABLE, byte_len)?;
    let target = GlobalLock(allocation);
    if target.is_null() {
        let _ = GlobalFree(Some(allocation));
        return Err(anyhow!("GlobalLock 失败"));
    }
    ptr::copy_nonoverlapping(wide.as_ptr().cast::<u8>(), target.cast::<u8>(), byte_len);
    let _ = GlobalUnlock(allocation);

    if let Err(error) = OpenClipboard(None) {
        let _ = GlobalFree(Some(allocation));
        return Err(error.into());
    }
    let result = (|| -> windows::core::Result<()> {
        EmptyClipboard()?;
        SetClipboardData(CF_UNICODETEXT, Some(HANDLE(allocation.0)))?;
        Ok(())
    })();
    let _ = CloseClipboard();
    if let Err(error) = result {
        let _ = GlobalFree(Some(allocation));
        return Err(error.into());
    }
    Ok(())
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn read_image_impl(left: i32, top: i32) -> Result<CapturedImage> {
    if IsClipboardFormatAvailable(CF_DIB).is_err() {
        return Err(anyhow!("剪贴板中没有可用图像"));
    }
    OpenClipboard(None)?;
    let result = (|| -> Result<CapturedImage> {
        let handle = GetClipboardData(CF_DIB)?;
        let global = HGLOBAL(handle.0);
        let size = GlobalSize(global);
        let data = GlobalLock(global);
        if data.is_null() {
            return Err(anyhow!("剪贴板图像数据无效"));
        }

        let result = decode_locked_dib(data.cast::<u8>(), size, left, top);
        let _ = GlobalUnlock(global);
        result
    })();
    let _ = CloseClipboard();
    result
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn decode_locked_dib(
    data: *const u8,
    size: usize,
    left: i32,
    top: i32,
) -> Result<CapturedImage> {
    if size < size_of::<BITMAPINFOHEADER>() {
        return Err(anyhow!("剪贴板图像数据无效"));
    }
    let bytes = std::slice::from_raw_parts(data, size);
    let header = ptr::read_unaligned(bytes.as_ptr().cast::<BITMAPINFOHEADER>());
    let width = header.biWidth.unsigned_abs();
    let height = header.biHeight.unsigned_abs();
    if width == 0 || height == 0 || !matches!(header.biBitCount, 24 | 32) {
        return Err(anyhow!("暂不支持该剪贴板图像格式"));
    }

    let header_size = header.biSize as usize;
    let stride = (width as usize * header.biBitCount as usize).div_ceil(32) * 4;
    let required = header_size.saturating_add(stride.saturating_mul(height as usize));
    if header_size < size_of::<BITMAPINFOHEADER>() || required > bytes.len() {
        return Err(anyhow!("剪贴板图像数据不完整"));
    }

    let pixels = &bytes[header_size..required];
    let mut rgba = vec![0_u8; width as usize * height as usize * 4];
    for output_y in 0..height as usize {
        let source_y = if header.biHeight > 0 {
            height as usize - 1 - output_y
        } else {
            output_y
        };
        let row = &pixels[source_y * stride..source_y * stride + stride];
        for x in 0..width as usize {
            let source = x * (header.biBitCount as usize / 8);
            let target = (output_y * width as usize + x) * 4;
            rgba[target] = row[source + 2];
            rgba[target + 1] = row[source + 1];
            rgba[target + 2] = row[source];
            rgba[target + 3] = if header.biBitCount == 32 {
                let alpha = row[source + 3];
                if alpha == 0 { 255 } else { alpha }
            } else {
                255
            };
        }
    }
    CapturedImage::from_rgba(left, top, width, height, &rgba)
}
