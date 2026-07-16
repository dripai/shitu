use std::{
    fs,
    fs::File,
    io::BufWriter,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use image::{
    ExtendedColorType, ImageEncoder,
    codecs::{jpeg::JpegEncoder, png::PngEncoder},
};

use crate::{
    capture::CapturedImage,
    config::{CaptureConfig, ImageFormat},
};

pub fn save_quick(image: &CapturedImage, config: &CaptureConfig) -> Result<PathBuf> {
    fs::create_dir_all(&config.save_directory)
        .with_context(|| format!("创建截图保存目录失败：{}", config.save_directory.display()))?;
    let stem = render_filename(&config.filename_template);
    let extension = extension(config.format);
    let path = unique_path(&config.save_directory, &stem, extension);
    save_to_path(image, &path, config.format, config.jpeg_quality)?;
    Ok(path)
}

pub fn save_as_dialog(
    image: &CapturedImage,
    initial_directory: &Path,
    format: ImageFormat,
    jpeg_quality: u8,
) -> Result<Option<PathBuf>> {
    let default_name = format!(
        "{}.{}",
        render_filename("Screenshot_{yyyy-MM-dd_HH-mm-ss}"),
        extension(format)
    );
    let dialog = rfd::FileDialog::new()
        .set_directory(initial_directory)
        .set_file_name(default_name)
        .add_filter("PNG 图像", &["png"])
        .add_filter("JPEG 图像", &["jpg", "jpeg"]);
    let Some(mut path) = dialog.save_file() else {
        return Ok(None);
    };
    if path.extension().is_none() {
        path.set_extension(extension(format));
    }
    save_to_path(
        image,
        &path,
        format_from_path(&path).unwrap_or(format),
        jpeg_quality,
    )?;
    Ok(Some(path))
}

pub fn save_to_path(
    image: &CapturedImage,
    path: &Path,
    format: ImageFormat,
    jpeg_quality: u8,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("创建目录失败：{}", parent.display()))?;
    }
    let file =
        File::create(path).with_context(|| format!("创建图像文件失败：{}", path.display()))?;
    let mut writer = BufWriter::new(file);
    let rgba = image.rgba_bytes();

    match format {
        ImageFormat::Png => PngEncoder::new(&mut writer)
            .write_image(
                &rgba,
                image.width(),
                image.height(),
                ExtendedColorType::Rgba8,
            )
            .context("PNG 编码失败")?,
        ImageFormat::Jpeg => {
            let mut rgb = Vec::with_capacity(image.width() as usize * image.height() as usize * 3);
            for pixel in rgba.chunks_exact(4) {
                let alpha = pixel[3] as u16;
                for channel in &pixel[..3] {
                    let value = (*channel as u16 * alpha + 255 * (255 - alpha)) / 255;
                    rgb.push(value as u8);
                }
            }
            JpegEncoder::new_with_quality(&mut writer, jpeg_quality.clamp(1, 100))
                .encode(&rgb, image.width(), image.height(), ExtendedColorType::Rgb8)
                .context("JPEG 编码失败")?;
        }
    }
    Ok(())
}

pub fn format_from_path(path: &Path) -> Option<ImageFormat> {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => Some(ImageFormat::Png),
        Some("jpg" | "jpeg") => Some(ImageFormat::Jpeg),
        _ => None,
    }
}

pub fn render_filename(template: &str) -> String {
    let now = local_time();
    template
        .replace("{yyyy}", &format!("{:04}", now.year))
        .replace("{MM}", &format!("{:02}", now.month))
        .replace("{dd}", &format!("{:02}", now.day))
        .replace("{HH}", &format!("{:02}", now.hour))
        .replace("{mm}", &format!("{:02}", now.minute))
        .replace("{ss}", &format!("{:02}", now.second))
        .trim()
        .to_owned()
}

fn unique_path(directory: &Path, stem: &str, extension: &str) -> PathBuf {
    let base = directory.join(format!("{stem}.{extension}"));
    if !base.exists() {
        return base;
    }
    for index in 1..10_000 {
        let candidate = directory.join(format!("{stem}_{index}.{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    directory.join(format!("{stem}_{}.{}", std::process::id(), extension))
}

fn extension(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
    }
}

#[derive(Clone, Copy)]
struct LocalTime {
    year: u16,
    month: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
}

#[cfg(windows)]
fn local_time() -> LocalTime {
    use windows::Win32::System::SystemInformation::GetLocalTime;

    let value = unsafe { GetLocalTime() };
    LocalTime {
        year: value.wYear,
        month: value.wMonth,
        day: value.wDay,
        hour: value.wHour,
        minute: value.wMinute,
        second: value.wSecond,
    }
}

#[cfg(not(windows))]
fn local_time() -> LocalTime {
    LocalTime {
        year: 1970,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{render_filename, save_to_path};
    use crate::{capture::CapturedImage, config::ImageFormat};

    #[test]
    fn filename_template_replaces_all_supported_tokens() {
        let name = render_filename("shot_{yyyy}-{MM}-{dd}_{HH}-{mm}-{ss}");
        assert!(!name.contains('{'));
        assert!(name.starts_with("shot_"));
    }

    #[test]
    fn png_and_jpeg_outputs_keep_original_dimensions() {
        let image = CapturedImage::from_rgba(
            0,
            0,
            2,
            2,
            &[
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
        )
        .unwrap();
        let directory = std::env::temp_dir().join(format!("gridstart-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let png = directory.join("output.png");
        let jpeg = directory.join("output.jpg");
        save_to_path(&image, &png, ImageFormat::Png, 90).unwrap();
        save_to_path(&image, &jpeg, ImageFormat::Jpeg, 90).unwrap();
        assert_eq!(image::image_dimensions(&png).unwrap(), (2, 2));
        assert_eq!(image::image_dimensions(&jpeg).unwrap(), (2, 2));
        let _ = std::fs::remove_dir_all(directory);
    }
}
