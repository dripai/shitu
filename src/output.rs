use std::{
    fs,
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Context, Result};
use image::{
    ExtendedColorType, ImageEncoder,
    codecs::{jpeg::JpegEncoder, png::PngEncoder},
};

use crate::{
    config::{CaptureConfig, ImageFormat},
    i18n,
    image::CapturedImage,
};

pub fn save_quick(image: &CapturedImage, config: &CaptureConfig) -> Result<PathBuf> {
    fs::create_dir_all(&config.save_directory).with_context(|| {
        format!(
            "{}: {}",
            i18n::text("创建截图保存目录失败", "Failed to create screenshot folder"),
            config.save_directory.display()
        )
    })?;
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
        .add_filter(i18n::text("PNG 图像", "PNG image"), &["png"])
        .add_filter(i18n::text("JPEG 图像", "JPEG image"), &["jpg", "jpeg"]);
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
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "{}: {}",
                i18n::text("创建目录失败", "Failed to create folder"),
                parent.display()
            )
        })?;
    }
    let temp_path = temporary_path(path);
    let result = encode_to_path(image, &temp_path, format, jpeg_quality)
        .and_then(|_| crate::platform::replace_file(&temp_path, path));
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn encode_to_path(
    image: &CapturedImage,
    path: &Path,
    format: ImageFormat,
    jpeg_quality: u8,
) -> Result<()> {
    let file = File::create(path).with_context(|| {
        format!(
            "{}: {}",
            i18n::text("创建图像文件失败", "Failed to create image file"),
            path.display()
        )
    })?;
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
            .context(i18n::text("PNG 编码失败", "PNG encoding failed"))?,
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
                .context(i18n::text("JPEG 编码失败", "JPEG encoding failed"))?;
        }
    }
    writer
        .flush()
        .context(i18n::text("写入图像文件失败", "Failed to write image file"))?;
    writer
        .get_ref()
        .sync_all()
        .context(i18n::text("同步图像文件失败", "Failed to flush image file"))?;
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .map(|value| value.to_string_lossy())
        .unwrap_or_else(|| "image".into());
    path.with_file_name(format!(".{file_name}.{}.{}.tmp", std::process::id(), id))
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
    let now = crate::platform::clock::local_time();
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

#[cfg(test)]
mod tests {
    use super::{render_filename, save_to_path};
    use crate::{config::ImageFormat, image::CapturedImage};

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
