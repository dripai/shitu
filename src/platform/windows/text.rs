use std::{ffi::c_void, mem::size_of, ptr};

use anyhow::{Result, anyhow, ensure};
use windows::Win32::{
    Foundation::{COLORREF, HWND},
    Graphics::Gdi::{
        ANTIALIASED_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateDIBSection,
        CreateFontIndirectW, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DeleteDC,
        DeleteObject, FF_SWISS, GdiFlush, GetDC, GetDeviceCaps, LOGFONTW, LOGPIXELSY,
        OUT_TT_PRECIS, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TRANSPARENT, TextOutW,
    },
    UI::Controls::Dialogs::{
        CF_FORCEFONTEXIST, CF_INITTOLOGFONTSTRUCT, CF_LIMITSIZE, CF_NOSCRIPTSEL, CF_NOVERTFONTS,
        CHOOSEFONTW, ChooseFontW, CommDlgExtendedError,
    },
};

use crate::i18n;
use crate::image::TextFont;

#[derive(Clone, Debug)]
pub struct FontChoice {
    pub font: TextFont,
    pub size: u32,
}

fn logical_font(font: &TextFont, height: i32) -> Result<LOGFONTW> {
    let family = font.family.encode_utf16().collect::<Vec<_>>();
    ensure!(
        !family.is_empty() && family.len() < 32 && !family.contains(&0),
        "Invalid font family"
    );
    let mut value = LOGFONTW {
        lfHeight: -height,
        lfWeight: font.weight,
        lfItalic: u8::from(font.italic),
        lfCharSet: DEFAULT_CHARSET,
        lfOutPrecision: OUT_TT_PRECIS,
        lfClipPrecision: CLIP_DEFAULT_PRECIS,
        lfQuality: ANTIALIASED_QUALITY,
        lfPitchAndFamily: DEFAULT_PITCH.0 | FF_SWISS.0,
        ..Default::default()
    };
    value.lfFaceName[..family.len()].copy_from_slice(&family);
    Ok(value)
}

fn pixels_from_points(tenths: i32) -> Result<u32> {
    ensure!(
        (60..=720).contains(&tenths),
        "Font size outside supported range"
    );
    Ok(((tenths * 96 + 360) / 720) as u32)
}

// GPUI Kit 0.7.1 has no font chooser. Use the OS dialog on a worker thread,
// so its modal message loop never reenters a borrowed GPUI view. Windows owns
// the dialog's positioning, keyboard navigation and cancellation semantics.
// https://learn.microsoft.com/windows/win32/api/commdlg/nf-commdlg-choosefontw
pub fn choose_font(owner: isize, current: FontChoice) -> Result<Option<FontChoice>> {
    unsafe {
        let dc = GetDC(None);
        ensure!(!dc.0.is_null(), "Get font dialog screen DC failed");
        let dpi = GetDeviceCaps(Some(dc), LOGPIXELSY);
        ReleaseDC(None, dc);
        ensure!(dpi > 0, "Invalid font dialog DPI");
        let mut font = logical_font(&current.font, (current.size as i32 * dpi + 48) / 96)?;
        let mut dialog = CHOOSEFONTW {
            lStructSize: size_of::<CHOOSEFONTW>() as u32,
            hwndOwner: HWND(owner as *mut c_void),
            lpLogFont: &mut font,
            Flags: CF_INITTOLOGFONTSTRUCT
                | CF_FORCEFONTEXIST
                | CF_LIMITSIZE
                | CF_NOSCRIPTSEL
                | CF_NOVERTFONTS,
            nSizeMin: 6,
            nSizeMax: 72,
            ..Default::default()
        };
        if !ChooseFontW(&mut dialog).as_bool() {
            let error = CommDlgExtendedError();
            ensure!(error.0 == 0, "ChooseFontW failed: 0x{:08x}", error.0);
            return Ok(None);
        }
        let length = font
            .lfFaceName
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(font.lfFaceName.len());
        let family = String::from_utf16(&font.lfFaceName[..length])?;
        ensure!(!family.is_empty(), "Font dialog returned an empty family");
        Ok(Some(FontChoice {
            font: TextFont {
                family,
                weight: font.lfWeight,
                italic: font.lfItalic != 0,
            },
            size: pixels_from_points(dialog.iPointSize)?,
        }))
    }
}

pub fn render_text_mask(
    width: u32,
    height: u32,
    position: (u32, u32),
    text: &str,
    font_size: u32,
    font: &TextFont,
) -> Result<Vec<u8>> {
    if width == 0 || height == 0 || text.is_empty() || font_size == 0 {
        return Err(anyhow!(i18n::text("文字标注参数无效")));
    }
    let font = logical_font(font, font_size as i32)?;
    unsafe { render_text_mask_impl(width, height, position, text, &font) }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn render_text_mask_impl(
    width: u32,
    height: u32,
    position: (u32, u32),
    text: &str,
    font: &LOGFONTW,
) -> Result<Vec<u8>> {
    let dc = CreateCompatibleDC(None);
    if dc.0.is_null() {
        return Err(anyhow!(i18n::text("创建文字绘制上下文失败")));
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
    let font = CreateFontIndirectW(font);
    if font.0.is_null() {
        let _ = SelectObject(dc, previous_bitmap);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        return Err(anyhow!(i18n::text("创建文字字体失败")));
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
        return Err(anyhow!(i18n::text("绘制文字标注失败")));
    }
    Ok(mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_choice_uses_image_pixels_and_preserves_face_and_style() {
        for (points, pixels) in [(60, 8), (150, 20), (180, 24), (720, 96)] {
            assert_eq!(pixels_from_points(points).unwrap(), pixels);
        }
        assert!(pixels_from_points(0).is_err());
        assert!(pixels_from_points(730).is_err());
        let font = TextFont {
            family: "Microsoft YaHei UI".into(),
            weight: 700,
            italic: true,
        };
        let logical = logical_font(&font, 24).unwrap();
        assert_eq!(logical.lfHeight, -24);
        assert_eq!(logical.lfWeight, 700);
        assert_eq!(logical.lfItalic, 1);
        let name = String::from_utf16(&logical.lfFaceName[..font.family.len()]).unwrap();
        assert_eq!(name, font.family);
        assert!(
            logical_font(
                &TextFont {
                    family: "x".repeat(32),
                    ..font
                },
                24
            )
            .is_err()
        );
    }

    #[test]
    fn exported_font_mask_changes_with_weight_and_italic() {
        let plain = TextFont::default();
        let styled = TextFont {
            weight: 700,
            italic: true,
            ..plain.clone()
        };
        let regular = render_text_mask(240, 80, (4, 4), "Font Aa", 32, &plain).unwrap();
        let bold_italic = render_text_mask(240, 80, (4, 4), "Font Aa", 32, &styled).unwrap();
        assert!(regular.iter().any(|v| *v > 0));
        assert!(bold_italic.iter().any(|v| *v > 0));
        assert_ne!(regular, bold_italic);
    }
}
