use std::path::Path;

use anyhow::{Context, Result, anyhow};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

use crate::i18n;

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
            return Err(anyhow!(i18n::text(
                "图像像素尺寸无效",
                "Invalid image pixel dimensions"
            )));
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
            .with_context(|| {
                format!(
                    "{}: {}",
                    i18n::text("无法读取图像", "Failed to read image"),
                    path.display()
                )
            })?
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

    #[cfg(windows)]
    pub fn draw_text(
        &mut self,
        position: (u32, u32),
        text: &str,
        font_size: u32,
        rgba: [u8; 4],
    ) -> Result<()> {
        let mask = crate::platform::windows::text::render_text_mask(
            self.width(),
            self.height(),
            position,
            text,
            font_size,
        )?;
        for (pixel, coverage) in self.pixels.make_mut_slice().iter_mut().zip(mask) {
            let alpha = coverage as u16 * rgba[3] as u16 / 255;
            if alpha == 0 {
                continue;
            }
            let inverse = 255 - alpha;
            pixel.r = ((rgba[0] as u16 * alpha + pixel.r as u16 * inverse) / 255) as u8;
            pixel.g = ((rgba[1] as u16 * alpha + pixel.g as u16 * inverse) / 255) as u8;
            pixel.b = ((rgba[2] as u16 * alpha + pixel.b as u16 * inverse) / 255) as u8;
            pixel.a = (alpha + pixel.a as u16 * inverse / 255).min(255) as u8;
        }
        Ok(())
    }

    #[cfg(not(windows))]
    pub fn draw_text(
        &mut self,
        _position: (u32, u32),
        _text: &str,
        _font_size: u32,
        _rgba: [u8; 4],
    ) -> Result<()> {
        Err(anyhow!(i18n::text(
            "当前平台尚未实现文字标注",
            "Text annotation is not implemented on this platform"
        )))
    }

    pub fn pixelate_stroke(&mut self, points: &[(u32, u32)], radius: u32, block_size: u32) {
        if points.is_empty() || radius == 0 || block_size == 0 {
            return;
        }

        let width = self.width() as usize;
        let height = self.height() as usize;
        let radius_usize = radius as usize;
        let left = points
            .iter()
            .map(|point| point.0 as usize)
            .min()
            .unwrap_or(0)
            .saturating_sub(radius_usize);
        let right = points
            .iter()
            .map(|point| point.0 as usize)
            .max()
            .unwrap_or(0)
            .saturating_add(radius_usize)
            .min(width - 1);
        let top = points
            .iter()
            .map(|point| point.1 as usize)
            .min()
            .unwrap_or(0)
            .saturating_sub(radius_usize);
        let bottom = points
            .iter()
            .map(|point| point.1 as usize)
            .max()
            .unwrap_or(0)
            .saturating_add(radius_usize)
            .min(height - 1);
        let mask_width = right - left + 1;
        let mask_height = bottom - top + 1;
        let mut mask = vec![false; mask_width * mask_height];
        let local_point = |point: (u32, u32)| {
            (
                point.0.saturating_sub(left as u32),
                point.1.saturating_sub(top as u32),
            )
        };
        if points.len() == 1 {
            mark_pixelation_disk(
                &mut mask,
                mask_width,
                mask_height,
                local_point(points[0]),
                radius,
            );
        } else {
            for pair in points.windows(2) {
                let from = pair[0];
                let to = pair[1];
                let dx = to.0 as i64 - from.0 as i64;
                let dy = to.1 as i64 - from.1 as i64;
                let steps = dx.unsigned_abs().max(dy.unsigned_abs()).max(1);
                for step in 0..=steps {
                    let center = (
                        (from.0 as i64 + dx * step as i64 / steps as i64).max(0) as u32,
                        (from.1 as i64 + dy * step as i64 / steps as i64).max(0) as u32,
                    );
                    mark_pixelation_disk(
                        &mut mask,
                        mask_width,
                        mask_height,
                        local_point(center),
                        radius,
                    );
                }
            }
        }

        let block_size = block_size as usize;
        let pixels = self.pixels.make_mut_slice();
        let first_block_x = left / block_size * block_size;
        let first_block_y = top / block_size * block_size;
        for block_y in (first_block_y..=bottom).step_by(block_size) {
            let block_bottom = (block_y + block_size).min(height);
            for block_x in (first_block_x..=right).step_by(block_size) {
                let block_right = (block_x + block_size).min(width);
                let contains_stroke = (block_y..block_bottom).any(|y| {
                    (block_x..block_right).any(|x| {
                        x >= left
                            && x <= right
                            && y >= top
                            && y <= bottom
                            && mask[(y - top) * mask_width + (x - left)]
                    })
                });
                if !contains_stroke {
                    continue;
                }

                let mut sum = [0u64; 4];
                let mut count = 0u64;
                for y in block_y..block_bottom {
                    for x in block_x..block_right {
                        let pixel = pixels[y * width + x];
                        sum[0] += pixel.r as u64;
                        sum[1] += pixel.g as u64;
                        sum[2] += pixel.b as u64;
                        sum[3] += pixel.a as u64;
                        count += 1;
                    }
                }
                let average = Rgba8Pixel {
                    r: (sum[0] / count) as u8,
                    g: (sum[1] / count) as u8,
                    b: (sum[2] / count) as u8,
                    a: (sum[3] / count) as u8,
                };
                for y in block_y..block_bottom {
                    for x in block_x..block_right {
                        if x >= left
                            && x <= right
                            && y >= top
                            && y <= bottom
                            && mask[(y - top) * mask_width + (x - left)]
                        {
                            pixels[y * width + x] = average;
                        }
                    }
                }
            }
        }
    }

    pub fn with_origin(mut self, left: i32, top: i32) -> Self {
        self.bounds.left = left;
        self.bounds.top = top;
        self
    }

    pub fn slint_image(&self) -> Image {
        Image::from_rgba8(self.pixels.clone())
    }

    pub fn crop(&self, left: u32, top: u32, width: u32, height: u32) -> Result<Self> {
        let right = left.checked_add(width).ok_or_else(|| {
            anyhow!(i18n::text(
                "图像裁剪坐标溢出",
                "Image crop coordinates overflow"
            ))
        })?;
        let bottom = top.checked_add(height).ok_or_else(|| {
            anyhow!(i18n::text(
                "图像裁剪坐标溢出",
                "Image crop coordinates overflow"
            ))
        })?;
        if width == 0 || height == 0 || right > self.width() || bottom > self.height() {
            return Err(anyhow!(i18n::text(
                "图像裁剪区域无效",
                "Invalid image crop area"
            )));
        }

        let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
        let source = self.pixels.as_slice();
        let target = pixels.make_mut_slice();
        let source_width = self.width() as usize;
        let target_width = width as usize;
        for row in 0..height as usize {
            let source_start = (top as usize + row) * source_width + left as usize;
            let target_start = row * target_width;
            target[target_start..target_start + target_width]
                .copy_from_slice(&source[source_start..source_start + target_width]);
        }

        Ok(Self {
            bounds: DesktopBounds {
                left: self.bounds.left + left as i32,
                top: self.bounds.top + top as i32,
                width: width as i32,
                height: height as i32,
            },
            pixels,
        })
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

fn mark_pixelation_disk(
    mask: &mut [bool],
    width: usize,
    height: usize,
    center: (u32, u32),
    radius: u32,
) {
    let center_x = center.0 as i64;
    let center_y = center.1 as i64;
    let radius = radius as i64;
    let left = (center_x - radius).max(0) as usize;
    let right = (center_x + radius).min(width.saturating_sub(1) as i64) as usize;
    let top = (center_y - radius).max(0) as usize;
    let bottom = (center_y + radius).min(height.saturating_sub(1) as i64) as usize;
    let radius_squared = radius * radius;
    for y in top..=bottom {
        for x in left..=right {
            let dx = x as i64 - center_x;
            let dy = y as i64 - center_y;
            if dx * dx + dy * dy <= radius_squared {
                mask[y * width + x] = true;
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::{CapturedImage, arrow_head};

    #[test]
    fn arrow_head_requires_a_visible_segment() {
        assert!(arrow_head((10, 10), (10, 10)).is_none());
        assert!(arrow_head((10, 10), (30, 20)).is_some());
    }

    #[test]
    fn crop_preserves_pixels_and_updates_origin() {
        let source = CapturedImage::from_rgba(
            -10,
            20,
            3,
            2,
            &[
                1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 13, 14, 15, 255, 16, 17,
                18, 255,
            ],
        )
        .unwrap();

        let cropped = source.crop(1, 0, 2, 2).unwrap();

        assert_eq!(cropped.bounds.left, -9);
        assert_eq!(cropped.bounds.top, 20);
        assert_eq!(cropped.width(), 2);
        assert_eq!(cropped.height(), 2);
        assert_eq!(
            cropped.rgba_bytes(),
            vec![4, 5, 6, 255, 7, 8, 9, 255, 13, 14, 15, 255, 16, 17, 18, 255]
        );
        assert!(source.crop(2, 1, 2, 1).is_err());
    }

    #[test]
    fn pixelation_uses_a_shared_color_inside_each_block() {
        let rgba = (0u8..16)
            .flat_map(|value| [value * 10, 0, 0, 255])
            .collect::<Vec<_>>();
        let mut image = CapturedImage::from_rgba(0, 0, 4, 4, &rgba).unwrap();

        image.pixelate_stroke(&[(1, 1)], 2, 2);

        let pixels = image.rgba_bytes();
        for index in [0usize, 1, 4, 5] {
            assert_eq!(pixels[index * 4], 25);
        }
    }

    #[cfg(windows)]
    #[test]
    fn text_rendering_changes_the_target_pixels() {
        let mut image = CapturedImage::from_rgba(0, 0, 80, 40, &[0; 80 * 40 * 4]).unwrap();
        let original = image.rgba_bytes();

        image
            .draw_text((2, 2), "Test", 20, [255, 255, 255, 255])
            .unwrap();

        assert_ne!(image.rgba_bytes(), original);
    }
}
