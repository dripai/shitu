use std::{ffi::c_void, ptr};

use anyhow::{Result, anyhow};
use windows::{
    Win32::{
        Foundation::COLORREF,
        Graphics::Gdi::{
            ANTIALIASED_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateDIBSection,
            CreateFontW, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DeleteDC, DeleteObject,
            FF_SWISS, FW_NORMAL, GdiFlush, OUT_TT_PRECIS, SelectObject, SetBkMode, SetTextColor,
            TRANSPARENT, TextOutW,
        },
    },
    core::PCWSTR,
};

use crate::i18n;

pub fn render_text_mask(
    width: u32,
    height: u32,
    position: (u32, u32),
    text: &str,
    font_size: u32,
) -> Result<Vec<u8>> {
    if width == 0 || height == 0 || text.is_empty() || font_size == 0 {
        return Err(anyhow!(i18n::text(
            "文字标注参数无效",
            "Invalid text annotation parameters"
        )));
    }
    unsafe { render_text_mask_impl(width, height, position, text, font_size) }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn render_text_mask_impl(
    width: u32,
    height: u32,
    position: (u32, u32),
    text: &str,
    font_size: u32,
) -> Result<Vec<u8>> {
    let dc = CreateCompatibleDC(None);
    if dc.0.is_null() {
        return Err(anyhow!(i18n::text(
            "创建文字绘制上下文失败",
            "Failed to create text rendering context"
        )));
    }

    let bitmap_info = super::bitmap_info(width as i32, -(height as i32));
    let mut bits: *mut c_void = ptr::null_mut();
    let bitmap = match CreateDIBSection(Some(dc), &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0)
    {
        Ok(bitmap) => bitmap,
        Err(error) => {
            let _ = DeleteDC(dc);
            return Err(error.into());
        }
    };
    let pixel_bytes = width as usize * height as usize * 4;
    ptr::write_bytes(bits.cast::<u8>(), 0, pixel_bytes);

    let previous_bitmap = SelectObject(dc, bitmap.into());
    let face_name = "Microsoft YaHei UI\0".encode_utf16().collect::<Vec<_>>();
    let font = CreateFontW(
        -(font_size as i32),
        0,
        0,
        0,
        FW_NORMAL.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_TT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        ANTIALIASED_QUALITY,
        DEFAULT_PITCH.0 as u32 | FF_SWISS.0 as u32,
        PCWSTR(face_name.as_ptr()),
    );
    if font.0.is_null() {
        let _ = SelectObject(dc, previous_bitmap);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        return Err(anyhow!(i18n::text(
            "创建文字字体失败",
            "Failed to create annotation font"
        )));
    }

    let previous_font = SelectObject(dc, font.into());
    let background_mode = SetBkMode(dc, TRANSPARENT);
    let _ = SetTextColor(dc, COLORREF(0x00ff_ffff));
    let utf16 = text.encode_utf16().collect::<Vec<_>>();
    let drawn = TextOutW(dc, position.0 as i32, position.1 as i32, &utf16).as_bool();
    let flushed = GdiFlush().as_bool();

    let bgra = std::slice::from_raw_parts(bits.cast::<u8>(), pixel_bytes);
    let mask = bgra
        .chunks_exact(4)
        .map(|pixel| pixel[0].max(pixel[1]).max(pixel[2]))
        .collect::<Vec<_>>();

    let _ = SelectObject(dc, previous_font);
    let _ = SelectObject(dc, previous_bitmap);
    let _ = DeleteObject(font.into());
    let _ = DeleteObject(bitmap.into());
    let _ = DeleteDC(dc);

    if background_mode == 0 || !drawn || !flushed {
        return Err(anyhow!(i18n::text(
            "绘制文字标注失败",
            "Failed to render text annotation"
        )));
    }
    Ok(mask)
}
