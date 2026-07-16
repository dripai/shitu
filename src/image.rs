use std::path::Path;

use anyhow::{Context, Result, anyhow};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

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
    use super::arrow_head;

    #[test]
    fn arrow_head_requires_a_visible_segment() {
        assert!(arrow_head((10, 10), (10, 10)).is_none());
        assert!(arrow_head((10, 10), (30, 20)).is_some());
    }
}
