use std::{ffi::c_void, ptr};

use anyhow::{Result, anyhow};
use windows::Win32::{
    Graphics::Gdi::{
        BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
        DeleteObject, GetDC, ReleaseDC, SRCCOPY, SelectObject,
    },
    UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    },
};

use crate::image::{CapturedImage, DesktopBounds};

pub fn virtual_desktop_bounds() -> Result<DesktopBounds> {
    let bounds = DesktopBounds {
        left: unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) },
        top: unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) },
        width: unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) },
        height: unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) },
    };
    if bounds.width <= 0 || bounds.height <= 0 {
        return Err(anyhow!("虚拟桌面没有可见像素"));
    }
    Ok(bounds)
}

pub fn capture_region(bounds: DesktopBounds) -> Result<CapturedImage> {
    if bounds.width <= 0 || bounds.height <= 0 {
        return Err(anyhow!("截图区域没有可见像素"));
    }
    let desktop = virtual_desktop_bounds()?;
    let right = bounds
        .left
        .checked_add(bounds.width)
        .ok_or_else(|| anyhow!("截图区域坐标溢出"))?;
    let bottom = bounds
        .top
        .checked_add(bounds.height)
        .ok_or_else(|| anyhow!("截图区域坐标溢出"))?;
    let desktop_right = desktop.left + desktop.width;
    let desktop_bottom = desktop.top + desktop.height;
    if bounds.left < desktop.left
        || bounds.top < desktop.top
        || right > desktop_right
        || bottom > desktop_bottom
    {
        return Err(anyhow!("截图区域超出虚拟桌面"));
    }
    unsafe { capture(bounds) }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn capture(bounds: DesktopBounds) -> Result<CapturedImage> {
    let screen_dc = GetDC(None);
    if screen_dc.0.is_null() {
        return Err(anyhow!("GetDC 失败"));
    }
    let memory_dc = CreateCompatibleDC(Some(screen_dc));
    if memory_dc.0.is_null() {
        let _ = ReleaseDC(None, screen_dc);
        return Err(anyhow!("CreateCompatibleDC 失败"));
    }

    let bitmap_info = super::bitmap_info(bounds.width, -bounds.height);
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
        return Err(anyhow!("BitBlt 失败"));
    }

    let pixel_count = bounds.width as usize * bounds.height as usize;
    let bgra = std::slice::from_raw_parts(source_pixels.cast::<u8>(), pixel_count * 4);
    let mut rgba = Vec::with_capacity(pixel_count * 4);
    for source in bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[source[2], source[1], source[0], 255]);
    }
    let _ = DeleteObject(bitmap.into());

    CapturedImage::from_rgba(
        bounds.left,
        bounds.top,
        bounds.width as u32,
        bounds.height as u32,
        &rgba,
    )
}
