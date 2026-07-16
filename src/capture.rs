use std::{ffi::c_void, mem::size_of, ptr};

use anyhow::{Result, anyhow};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use windows::Win32::{
    Foundation::{GlobalFree, HANDLE},
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC,
        CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC, SRCCOPY,
        SelectObject,
    },
    System::{
        DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
    },
    UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    },
};

const CF_DIB: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DesktopBounds {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone)]
pub struct CapturedImage {
    pub bounds: DesktopBounds,
    pixels: SharedPixelBuffer<Rgba8Pixel>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawStyle {
    pub rgba: [u8; 4],
    pub radius: i32,
}

impl CapturedImage {
    pub fn crop(&self, left: u32, top: u32, width: u32, height: u32) -> Option<Self> {
        if width == 0
            || height == 0
            || left.checked_add(width)? > self.bounds.width as u32
            || top.checked_add(height)? > self.bounds.height as u32
        {
            return None;
        }

        let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
        let source = self.pixels.as_slice();
        let target = pixels.make_mut_slice();
        let source_stride = self.bounds.width as usize;
        let target_stride = width as usize;

        for row in 0..height as usize {
            let source_offset = (top as usize + row) * source_stride + left as usize;
            let target_offset = row * target_stride;
            target[target_offset..target_offset + target_stride]
                .copy_from_slice(&source[source_offset..source_offset + target_stride]);
        }

        Some(Self {
            bounds: DesktopBounds {
                left: self.bounds.left + left as i32,
                top: self.bounds.top + top as i32,
                width: width as i32,
                height: height as i32,
            },
            pixels,
        })
    }

    pub fn slint_image(&self) -> Image {
        Image::from_rgba8(self.pixels.clone())
    }

    pub fn draw_line(&mut self, from: (u32, u32), to: (u32, u32), style: DrawStyle) {
        let (mut x0, mut y0) = (from.0 as i32, from.1 as i32);
        let (x1, y1) = (to.0 as i32, to.1 as i32);
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut error = dx + dy;

        loop {
            self.paint_dot(x0, y0, style);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let twice_error = error * 2;
            if twice_error >= dy {
                error += dy;
                x0 += sx;
            }
            if twice_error <= dx {
                error += dx;
                y0 += sy;
            }
        }
    }

    pub fn draw_rectangle(&mut self, start: (u32, u32), end: (u32, u32), style: DrawStyle) {
        let left = start.0.min(end.0);
        let right = start.0.max(end.0);
        let top = start.1.min(end.1);
        let bottom = start.1.max(end.1);
        self.draw_line((left, top), (right, top), style);
        self.draw_line((right, top), (right, bottom), style);
        self.draw_line((right, bottom), (left, bottom), style);
        self.draw_line((left, bottom), (left, top), style);
    }

    pub fn draw_arrow(&mut self, start: (u32, u32), end: (u32, u32), style: DrawStyle) {
        self.draw_line(start, end, style);
        let Some((left, right)) = arrow_head(start, end) else {
            return;
        };
        self.draw_line(end, left, style);
        self.draw_line(end, right, style);
    }

    fn paint_dot(&mut self, center_x: i32, center_y: i32, style: DrawStyle) {
        let width = self.bounds.width;
        let height = self.bounds.height;
        let pixels = self.pixels.make_mut_slice();

        for y in center_y - style.radius..=center_y + style.radius {
            for x in center_x - style.radius..=center_x + style.radius {
                if x < 0
                    || y < 0
                    || x >= width
                    || y >= height
                    || (x - center_x).pow(2) + (y - center_y).pow(2) > style.radius.pow(2)
                {
                    continue;
                }
                pixels[y as usize * width as usize + x as usize] = Rgba8Pixel {
                    r: style.rgba[0],
                    g: style.rgba[1],
                    b: style.rgba[2],
                    a: style.rgba[3],
                };
            }
        }
    }
}

pub fn capture_virtual_desktop() -> Result<CapturedImage> {
    let bounds = DesktopBounds {
        left: unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) },
        top: unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) },
        width: unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) },
        height: unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) },
    };
    if bounds.width <= 0 || bounds.height <= 0 {
        return Err(anyhow!("virtual desktop has no visible pixels"));
    }
    unsafe { capture(bounds) }
}

pub fn copy_to_clipboard(image: &CapturedImage) -> Result<()> {
    unsafe { write_clipboard(image) }
}

pub fn arrow_head(start: (u32, u32), end: (u32, u32)) -> Option<((u32, u32), (u32, u32))> {
    let dx = end.0 as f32 - start.0 as f32;
    let dy = end.1 as f32 - start.1 as f32;
    let length = (dx * dx + dy * dy).sqrt();
    if length < 2.0 {
        return None;
    }

    let unit_x = dx / length;
    let unit_y = dy / length;
    let side = 14.0_f32.min(length * 0.45);
    let left = (
        (end.0 as f32 - unit_x * side - unit_y * side * 0.55).max(0.0) as u32,
        (end.1 as f32 - unit_y * side + unit_x * side * 0.55).max(0.0) as u32,
    );
    let right = (
        (end.0 as f32 - unit_x * side + unit_y * side * 0.55).max(0.0) as u32,
        (end.1 as f32 - unit_y * side - unit_x * side * 0.55).max(0.0) as u32,
    );
    Some((left, right))
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn capture(bounds: DesktopBounds) -> Result<CapturedImage> {
    let screen_dc = GetDC(None);
    if screen_dc.0.is_null() {
        return Err(anyhow!("GetDC failed"));
    }
    let memory_dc = CreateCompatibleDC(Some(screen_dc));
    if memory_dc.0.is_null() {
        let _ = ReleaseDC(None, screen_dc);
        return Err(anyhow!("CreateCompatibleDC failed"));
    }

    let bitmap_info = bitmap_info(bounds.width, -bounds.height);
    let mut source_pixels: *mut c_void = ptr::null_mut();
    let bitmap = match CreateDIBSection(
        Some(screen_dc),
        &bitmap_info,
        DIB_RGB_COLORS,
        &mut source_pixels,
        None,
        0,
    ) {
        Ok(bitmap) => bitmap,
        Err(error) => {
            let _ = DeleteDC(memory_dc);
            let _ = ReleaseDC(None, screen_dc);
            return Err(error.into());
        }
    };

    let previous = SelectObject(memory_dc, bitmap.into());
    let copied = BitBlt(
        memory_dc,
        0,
        0,
        bounds.width,
        bounds.height,
        Some(screen_dc),
        bounds.left,
        bounds.top,
        SRCCOPY | CAPTUREBLT,
    );
    let _ = SelectObject(memory_dc, previous);
    let _ = DeleteDC(memory_dc);
    let _ = ReleaseDC(None, screen_dc);

    if copied.is_err() {
        let _ = DeleteObject(bitmap.into());
        return Err(anyhow!("BitBlt failed"));
    }

    let pixel_count = bounds.width as usize * bounds.height as usize;
    let bgra = std::slice::from_raw_parts(source_pixels.cast::<u8>(), pixel_count * 4);
    let mut pixels =
        SharedPixelBuffer::<Rgba8Pixel>::new(bounds.width as u32, bounds.height as u32);
    for (source, target) in bgra.chunks_exact(4).zip(pixels.make_mut_slice()) {
        *target = Rgba8Pixel {
            r: source[2],
            g: source[1],
            b: source[0],
            a: 255,
        };
    }
    let _ = DeleteObject(bitmap.into());

    Ok(CapturedImage { bounds, pixels })
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn write_clipboard(image: &CapturedImage) -> Result<()> {
    let bitmap_info = bitmap_info(image.bounds.width, -image.bounds.height);
    let pixel_bytes = image.pixels.as_slice().len() * 4;
    let dib_bytes = size_of::<BITMAPINFOHEADER>() + pixel_bytes;
    let allocation = GlobalAlloc(GMEM_MOVEABLE, dib_bytes)?;
    let target = GlobalLock(allocation);
    if target.is_null() {
        let _ = GlobalFree(Some(allocation));
        return Err(anyhow!("GlobalLock failed"));
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
    for (source, target) in image
        .pixels
        .as_slice()
        .iter()
        .zip(target_pixels.chunks_exact_mut(4))
    {
        target.copy_from_slice(&[source.b, source.g, source.r, source.a]);
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

#[cfg(test)]
mod tests {
    use super::arrow_head;

    #[test]
    fn arrow_head_requires_a_visible_segment() {
        assert!(arrow_head((10, 10), (10, 10)).is_none());
        assert!(arrow_head((10, 10), (30, 20)).is_some());
    }
}
