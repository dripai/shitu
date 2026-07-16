use std::{collections::HashMap, path::Path};

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use windows::{
    Win32::{
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
            DeleteObject, GetDIBits, HBITMAP, HDC, SelectObject,
        },
        Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES,
        UI::{
            Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW},
            WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx},
        },
    },
    core::PCWSTR,
};

use crate::model::AppEntry;

pub struct IconCache {
    images: HashMap<String, Image>,
}

impl IconCache {
    pub fn new() -> Self {
        Self {
            images: HashMap::new(),
        }
    }

    pub fn image(&mut self, app: &AppEntry) -> Image {
        if let Some(image) = self.images.get(&app.id) {
            return image.clone();
        }
        let image = extract_icon(&app.launch_path).unwrap_or_else(|| placeholder_icon(&app.name));
        self.images.insert(app.id.clone(), image.clone());
        image
    }
}

fn extract_icon(path: &Path) -> Option<Image> {
    let wide = wide(path.as_os_str());
    let mut info = SHFILEINFOW::default();
    let ok = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if ok == 0 || info.hIcon.is_invalid() {
        return None;
    }
    let image = hicon_to_image(info.hIcon);
    unsafe {
        let _ = DestroyIcon(info.hIcon);
    }
    image
}

fn hicon_to_image(icon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<Image> {
    const SIZE: i32 = 32;
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.is_invalid() {
        return None;
    }

    let Some(bitmap) = create_32bit_bitmap(dc, SIZE, SIZE) else {
        let _ = unsafe { DeleteDC(dc) };
        return None;
    };
    let old = unsafe { SelectObject(dc, bitmap.into()) };
    let _ = unsafe { DrawIconEx(dc, 0, 0, icon, SIZE, SIZE, 0, None, DI_NORMAL) };

    let mut pixels = vec![0_u8; (SIZE * SIZE * 4) as usize];
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: SIZE,
            biHeight: -SIZE,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let rows = unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            SIZE as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut bmi,
            DIB_RGB_COLORS,
        )
    };

    let _ = unsafe { SelectObject(dc, old) };
    let _ = unsafe { DeleteObject(bitmap.into()) };
    let _ = unsafe { DeleteDC(dc) };

    if rows == 0 {
        return None;
    }

    let rgba: Vec<u8> = pixels
        .chunks_exact(4)
        .flat_map(|bgra| [bgra[2], bgra[1], bgra[0], bgra[3]])
        .collect();
    Some(Image::from_rgba8(
        SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, SIZE as u32, SIZE as u32),
    ))
}

fn create_32bit_bitmap(dc: HDC, width: i32, height: i32) -> Option<HBITMAP> {
    use windows::Win32::Graphics::Gdi::CreateDIBSection;

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let bitmap =
        unsafe { CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) }.ok()?;
    if bitmap.is_invalid() {
        None
    } else {
        Some(bitmap)
    }
}

fn placeholder_icon(name: &str) -> Image {
    let hash = name.bytes().fold(0_u8, |acc, byte| acc.wrapping_add(byte));
    let mut pixels = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32 {
        for x in 0..32 {
            let inside = (4..28).contains(&x) && (4..28).contains(&y);
            if inside {
                pixels.extend_from_slice(&[70 + hash % 80, 100 + hash % 70, 140 + hash % 60, 255]);
            } else {
                pixels.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        &pixels, 32, 32,
    ))
}

fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.as_ref().encode_wide().chain(Some(0)).collect()
}
