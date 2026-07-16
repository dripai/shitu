use std::{ffi::c_void, mem::size_of, path::Path, ptr};

use anyhow::{Context, Result, anyhow};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use windows::Win32::{
    Foundation::{GlobalFree, HANDLE, HGLOBAL},
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC,
        CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, ReleaseDC, SRCCOPY,
        SelectObject,
    },
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
            OpenClipboard, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
    },
    UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    },
};

const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;

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
    pub fn from_rgba(left: i32, top: i32, width: u32, height: u32, rgba: &[u8]) -> Result<Self> {
        let expected = width as usize * height as usize * 4;
        if width == 0 || height == 0 || rgba.len() != expected {
            return Err(anyhow!("图像像素尺寸无效"));
        }
        let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
        for (source, target) in rgba.chunks_exact(4).zip(pixels.make_mut_slice()) {
            *target = Rgba8Pixel {
                r: source[0],
                g: source[1],
                b: source[2],
                a: source[3],
            };
        }
        Ok(Self {
            bounds: DesktopBounds {
                left,
                top,
                width: width as i32,
                height: height as i32,
            },
            pixels,
        })
    }

    pub fn from_file(path: &Path, left: i32, top: i32) -> Result<Self> {
        let image = image::open(path)
            .with_context(|| format!("无法读取图像：{}", path.display()))?
            .to_rgba8();
        Self::from_rgba(left, top, image.width(), image.height(), image.as_raw())
    }

    pub fn width(&self) -> u32 {
        self.bounds.width as u32
    }

    pub fn height(&self) -> u32 {
        self.bounds.height as u32
    }

    pub fn rgba_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.pixels.as_slice().len() * 4);
        for pixel in self.pixels.as_slice() {
            bytes.extend_from_slice(&[pixel.r, pixel.g, pixel.b, pixel.a]);
        }
        bytes
    }

    pub fn with_origin(mut self, left: i32, top: i32) -> Self {
        self.bounds.left = left;
        self.bounds.top = top;
        self
    }

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

    pub fn rotate_left(&self) -> Self {
        let source_width = self.width();
        let source_height = self.height();
        let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(source_height, source_width);
        let source = self.pixels.as_slice();
        let target = pixels.make_mut_slice();

        for y in 0..source_height {
            for x in 0..source_width {
                let target_x = y;
                let target_y = source_width - 1 - x;
                target[(target_y * source_height + target_x) as usize] =
                    source[(y * source_width + x) as usize];
            }
        }
        Self {
            bounds: DesktopBounds {
                left: self.bounds.left,
                top: self.bounds.top,
                width: source_height as i32,
                height: source_width as i32,
            },
            pixels,
        }
    }

    pub fn rotate_right(&self) -> Self {
        let source_width = self.width();
        let source_height = self.height();
        let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(source_height, source_width);
        let source = self.pixels.as_slice();
        let target = pixels.make_mut_slice();

        for y in 0..source_height {
            for x in 0..source_width {
                let target_x = source_height - 1 - y;
                let target_y = x;
                target[(target_y * source_height + target_x) as usize] =
                    source[(y * source_width + x) as usize];
            }
        }
        Self {
            bounds: DesktopBounds {
                left: self.bounds.left,
                top: self.bounds.top,
                width: source_height as i32,
                height: source_width as i32,
            },
            pixels,
        }
    }

    pub fn flip_horizontal(&self) -> Self {
        let width = self.width();
        let height = self.height();
        let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
        let source = self.pixels.as_slice();
        let target = pixels.make_mut_slice();
        for y in 0..height {
            for x in 0..width {
                target[(y * width + x) as usize] = source[(y * width + (width - 1 - x)) as usize];
            }
        }
        Self {
            bounds: self.bounds,
            pixels,
        }
    }

    pub fn flip_vertical(&self) -> Self {
        let width = self.width();
        let height = self.height();
        let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
        let source = self.pixels.as_slice();
        let target = pixels.make_mut_slice();
        for y in 0..height {
            let source_y = height - 1 - y;
            let target_start = (y * width) as usize;
            let source_start = (source_y * width) as usize;
            target[target_start..target_start + width as usize]
                .copy_from_slice(&source[source_start..source_start + width as usize]);
        }
        Self {
            bounds: self.bounds,
            pixels,
        }
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

pub fn copy_text_to_clipboard(text: &str) -> Result<()> {
    unsafe { write_text_clipboard(text) }
}

pub fn image_from_clipboard(left: i32, top: i32) -> Result<CapturedImage> {
    unsafe { read_clipboard_image(left, top) }
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

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn write_text_clipboard(text: &str) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;

    let wide: Vec<u16> = std::ffi::OsStr::new(text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let byte_len = wide.len() * size_of::<u16>();
    let allocation = GlobalAlloc(GMEM_MOVEABLE, byte_len)?;
    let target = GlobalLock(allocation);
    if target.is_null() {
        let _ = GlobalFree(Some(allocation));
        return Err(anyhow!("GlobalLock failed"));
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
unsafe fn read_clipboard_image(left: i32, top: i32) -> Result<CapturedImage> {
    if IsClipboardFormatAvailable(CF_DIB).is_err() {
        return Err(anyhow!("剪贴板中没有可用图像"));
    }
    OpenClipboard(None)?;
    let result = (|| -> Result<CapturedImage> {
        let handle = GetClipboardData(CF_DIB)?;
        let global = HGLOBAL(handle.0);
        let size = GlobalSize(global);
        let data = GlobalLock(global);
        if data.is_null() || size < size_of::<BITMAPINFOHEADER>() {
            return Err(anyhow!("剪贴板图像数据无效"));
        }

        let bytes = std::slice::from_raw_parts(data.cast::<u8>(), size);
        let header = ptr::read_unaligned(bytes.as_ptr().cast::<BITMAPINFOHEADER>());
        let width = header.biWidth.unsigned_abs();
        let height = header.biHeight.unsigned_abs();
        if width == 0 || height == 0 || !matches!(header.biBitCount, 24 | 32) {
            let _ = GlobalUnlock(global);
            return Err(anyhow!("暂不支持该剪贴板图像格式"));
        }

        let header_size = header.biSize as usize;
        let stride = (width as usize * header.biBitCount as usize).div_ceil(32) * 4;
        let required = header_size.saturating_add(stride.saturating_mul(height as usize));
        if header_size < size_of::<BITMAPINFOHEADER>() || required > bytes.len() {
            let _ = GlobalUnlock(global);
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
        let _ = GlobalUnlock(global);
        CapturedImage::from_rgba(left, top, width, height, &rgba)
    })();
    let _ = CloseClipboard();
    result
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
