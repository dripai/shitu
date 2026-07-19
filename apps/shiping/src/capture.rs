use anyhow::{Context, Result, anyhow};

use crate::target::Bounds;

pub fn output_size(source: Bounds, quality_preset: u8) -> (u32, u32) {
    let maximum = match quality_preset {
        1 => Some((1280_u32, 720_u32)),
        2 | 0 => Some((1920_u32, 1080_u32)),
        _ => None,
    };
    let source_width = source.width.max(16) as u32;
    let source_height = source.height.max(16) as u32;
    let (mut width, mut height) = if let Some((max_width, max_height)) = maximum {
        let scale = (max_width as f64 / source_width as f64)
            .min(max_height as f64 / source_height as f64)
            .min(1.0);
        (
            (source_width as f64 * scale).round() as u32,
            (source_height as f64 * scale).round() as u32,
        )
    } else {
        (source_width, source_height)
    };
    width = width.max(16) & !1;
    height = height.max(16) & !1;
    (width, height)
}

#[cfg(windows)]
pub struct FrameGrabber {
    screen_dc: windows::Win32::Graphics::Gdi::HDC,
    memory_dc: windows::Win32::Graphics::Gdi::HDC,
    bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
    previous: windows::Win32::Graphics::Gdi::HGDIOBJ,
    pixels: *mut u8,
    width: u32,
    height: u32,
}

#[cfg(windows)]
unsafe impl Send for FrameGrabber {}

#[cfg(windows)]
impl FrameGrabber {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        use std::{ffi::c_void, ptr};
        use windows::Win32::Graphics::Gdi::{
            CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, GetDC, HALFTONE, SelectObject,
            SetStretchBltMode,
        };

        let screen_dc = unsafe { GetDC(None) };
        if screen_dc.0.is_null() {
            return Err(anyhow!("GetDC 失败"));
        }
        let memory_dc = unsafe { CreateCompatibleDC(Some(screen_dc)) };
        if memory_dc.0.is_null() {
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, screen_dc);
            }
            return Err(anyhow!("CreateCompatibleDC 失败"));
        }
        let info = bitmap_info(width as i32, height as i32);
        let mut pixels: *mut c_void = ptr::null_mut();
        let bitmap = unsafe {
            CreateDIBSection(Some(screen_dc), &info, DIB_RGB_COLORS, &mut pixels, None, 0)
        }
        .context("CreateDIBSection 失败")?;
        if pixels.is_null() {
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::DeleteObject(bitmap.into());
                let _ = windows::Win32::Graphics::Gdi::DeleteDC(memory_dc);
                let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, screen_dc);
            }
            return Err(anyhow!("DIB 像素缓冲区为空"));
        }
        let previous = unsafe { SelectObject(memory_dc, bitmap.into()) };
        unsafe {
            SetStretchBltMode(memory_dc, HALFTONE);
        }
        Ok(Self {
            screen_dc,
            memory_dc,
            bitmap,
            previous,
            pixels: pixels.cast(),
            width,
            height,
        })
    }

    pub fn capture(
        &mut self,
        source: Bounds,
        show_cursor: bool,
        highlight_clicks: bool,
    ) -> Result<&[u8]> {
        use windows::Win32::Graphics::Gdi::{CAPTUREBLT, GdiFlush, SRCCOPY, StretchBlt};
        let copied = unsafe {
            StretchBlt(
                self.memory_dc,
                0,
                0,
                self.width as i32,
                self.height as i32,
                Some(self.screen_dc),
                source.left,
                source.top,
                source.width,
                source.height,
                SRCCOPY | CAPTUREBLT,
            )
        };
        if !copied.as_bool() {
            return Err(anyhow!("StretchBlt 采集屏幕失败"));
        }
        if highlight_clicks {
            self.draw_click_highlight(source);
        }
        if show_cursor {
            self.draw_cursor(source);
        }
        // CreateDIBSection exposes its backing pixels directly. Microsoft requires
        // flushing pending GDI drawing before the application reads that memory.
        unsafe {
            let _ = GdiFlush();
        }
        let length = self.width as usize * self.height as usize * 4;
        Ok(unsafe { std::slice::from_raw_parts(self.pixels, length) })
    }

    fn draw_cursor(&self, source: Bounds) {
        use std::mem::size_of;
        use windows::Win32::{
            Graphics::Gdi::{DeleteObject, HGDIOBJ},
            UI::WindowsAndMessaging::{
                CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetIconInfo,
                GetSystemMetrics, ICONINFO, SM_CXCURSOR, SM_CYCURSOR,
            },
        };
        let mut cursor = CURSORINFO {
            cbSize: size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetCursorInfo(&mut cursor) }.is_err()
            || cursor.flags != CURSOR_SHOWING
            || cursor.hCursor.0.is_null()
        {
            return;
        }
        let mut icon = ICONINFO::default();
        if unsafe { GetIconInfo(cursor.hCursor.into(), &mut icon) }.is_err() {
            return;
        }
        let scale_x = self.width as f64 / source.width as f64;
        let scale_y = self.height as f64 / source.height as f64;
        let x = ((cursor.ptScreenPos.x - source.left) as f64 * scale_x).round() as i32
            - (icon.xHotspot as f64 * scale_x).round() as i32;
        let y = ((cursor.ptScreenPos.y - source.top) as f64 * scale_y).round() as i32
            - (icon.yHotspot as f64 * scale_y).round() as i32;
        let width = (unsafe { GetSystemMetrics(SM_CXCURSOR) } as f64 * scale_x)
            .round()
            .max(1.0) as i32;
        let height = (unsafe { GetSystemMetrics(SM_CYCURSOR) } as f64 * scale_y)
            .round()
            .max(1.0) as i32;
        let _ = unsafe {
            DrawIconEx(
                self.memory_dc,
                x,
                y,
                cursor.hCursor.into(),
                width,
                height,
                0,
                None,
                DI_NORMAL,
            )
        };
        if !icon.hbmMask.0.is_null() {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(icon.hbmMask.0));
            }
        }
        if !icon.hbmColor.0.is_null() {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(icon.hbmColor.0));
            }
        }
    }

    fn draw_click_highlight(&self, source: Bounds) {
        use windows::Win32::{
            Foundation::COLORREF,
            Graphics::Gdi::{
                CreatePen, DeleteObject, Ellipse, GetStockObject, HGDIOBJ, NULL_BRUSH, PS_SOLID,
                SelectObject,
            },
            UI::{
                Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON},
                WindowsAndMessaging::{CURSORINFO, GetCursorInfo},
            },
        };
        if unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } >= 0 {
            return;
        }
        let mut cursor = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetCursorInfo(&mut cursor) }.is_err() {
            return;
        }
        let scale_x = self.width as f64 / source.width as f64;
        let scale_y = self.height as f64 / source.height as f64;
        let x = ((cursor.ptScreenPos.x - source.left) as f64 * scale_x).round() as i32;
        let y = ((cursor.ptScreenPos.y - source.top) as f64 * scale_y).round() as i32;
        let radius = (18.0 * scale_x.min(scale_y)).round().max(8.0) as i32;
        let pen = unsafe { CreatePen(PS_SOLID, 4, COLORREF(0x00505cff)) };
        if pen.0.is_null() {
            return;
        }
        let old_pen = unsafe { SelectObject(self.memory_dc, pen.into()) };
        let old_brush = unsafe { SelectObject(self.memory_dc, GetStockObject(NULL_BRUSH)) };
        unsafe {
            let _ = Ellipse(
                self.memory_dc,
                x - radius,
                y - radius,
                x + radius,
                y + radius,
            );
            SelectObject(self.memory_dc, old_brush);
            SelectObject(self.memory_dc, old_pen);
            let _ = DeleteObject(HGDIOBJ(pen.0));
        }
    }
}

#[cfg(windows)]
impl Drop for FrameGrabber {
    fn drop(&mut self) {
        use windows::Win32::Graphics::Gdi::{DeleteDC, DeleteObject, ReleaseDC, SelectObject};
        unsafe {
            SelectObject(self.memory_dc, self.previous);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.memory_dc);
            let _ = ReleaseDC(None, self.screen_dc);
        }
    }
}

#[cfg(windows)]
fn bitmap_info(width: i32, height: i32) -> windows::Win32::Graphics::Gdi::BITMAPINFO {
    use std::mem::size_of;
    use windows::Win32::Graphics::Gdi::{BI_RGB, BITMAPINFO, BITMAPINFOHEADER};
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Media Foundation receives a positive MF_MT_DEFAULT_STRIDE, which
            // denotes top-down RGB rows. A negative DIB height uses the same order.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::bitmap_info;
    use super::output_size;
    use crate::target::Bounds;

    #[test]
    fn output_size_preserves_ratio_and_even_dimensions() {
        let source = Bounds {
            left: 0,
            top: 0,
            width: 2560,
            height: 1440,
        };
        assert_eq!(output_size(source, 1), (1280, 720));
        assert_eq!(output_size(source, 2), (1920, 1080));
        assert_eq!(output_size(source, 3), (2560, 1440));
        let odd = Bounds {
            width: 801,
            height: 601,
            ..source
        };
        let size = output_size(odd, 3);
        assert_eq!(size.0 % 2, 0);
        assert_eq!(size.1 % 2, 0);
    }

    #[cfg(windows)]
    #[test]
    fn capture_bitmap_is_top_down_like_media_foundation_input() {
        let info = bitmap_info(1920, 1080);
        assert_eq!(info.bmiHeader.biWidth, 1920);
        assert_eq!(info.bmiHeader.biHeight, -1080);
    }
}
