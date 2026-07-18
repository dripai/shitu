use std::{fs, io::Write, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
pub use shi_foundation::LanguageMode;
pub use shi_foundation::config::default_picture_directory;

use crate::i18n;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    #[default]
    Png,
    Jpeg,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrEngineKind {
    #[default]
    System,
    WindowsAi,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OcrConfig {
    pub engine: OcrEngineKind,
    /// Application-side filter for Windows AI `RecognizedWord.MatchConfidence`.
    pub minimum_confidence: u8,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            engine: OcrEngineKind::System,
            minimum_confidence: 60,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    pub format: ImageFormat,
    pub jpeg_quality: u8,
    pub save_directory: PathBuf,
    pub filename_template: String,
    pub auto_save: bool,
    pub save_notification: bool,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            format: ImageFormat::Png,
            jpeg_quality: 90,
            save_directory: default_picture_directory(),
            filename_template: "Screenshot_{yyyy-MM-dd_HH-mm-ss}".to_owned(),
            auto_save: false,
            save_notification: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PinConfig {
    pub default_opacity: u8,
    pub shadow: bool,
    pub always_on_top: bool,
    pub wheel_zoom: bool,
    pub zoom_step: u8,
    pub double_click_close: bool,
}

impl Default for PinConfig {
    fn default() -> Self {
        Self {
            default_opacity: 100,
            shadow: true,
            always_on_top: true,
            wheel_zoom: true,
            zoom_step: 15,
            double_click_close: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub appearance: AppearanceMode,
    pub language: LanguageMode,
    pub launch_at_startup: bool,
    pub hotkey: Option<String>,
    pub capture: CaptureConfig,
    pub ocr: OcrConfig,
    pub pin: PinConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            appearance: AppearanceMode::System,
            language: LanguageMode::System,
            launch_at_startup: false,
            hotkey: Some("Ctrl+Alt+C".to_owned()),
            capture: CaptureConfig::default(),
            ocr: OcrConfig::default(),
            pin: PinConfig::default(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = Self::path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "{}: {}",
                        i18n::text("读取配置失败", "Failed to read settings"),
                        path.display()
                    )
                });
            }
        };
        let mut config: Self = serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "{}: {}",
                i18n::text("配置文件格式无效", "Invalid settings file"),
                path.display()
            )
        })?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let mut config = self.clone();
        config.validate()?;

        let path = Self::path();
        let parent = path.parent().ok_or_else(|| {
            anyhow!(i18n::text(
                "配置路径没有父目录",
                "The settings path has no parent directory"
            ))
        })?;
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "{}: {}",
                i18n::text("创建配置目录失败", "Failed to create settings folder"),
                parent.display()
            )
        })?;

        let temp_path = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(&config)?;
        let write_result = (|| -> Result<()> {
            let mut file = fs::File::create(&temp_path).with_context(|| {
                format!(
                    "{}: {}",
                    i18n::text(
                        "创建临时配置失败",
                        "Failed to create temporary settings file"
                    ),
                    temp_path.display()
                )
            })?;
            file.write_all(&json).with_context(|| {
                format!(
                    "{}: {}",
                    i18n::text(
                        "写入临时配置失败",
                        "Failed to write temporary settings file"
                    ),
                    temp_path.display()
                )
            })?;
            file.sync_all().with_context(|| {
                format!(
                    "{}: {}",
                    i18n::text(
                        "同步临时配置失败",
                        "Failed to flush temporary settings file"
                    ),
                    temp_path.display()
                )
            })?;
            crate::platform::replace_file(&temp_path, &path)
        })();

        if write_result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        write_result
    }

    pub fn validate(&mut self) -> Result<()> {
        self.capture.jpeg_quality = self.capture.jpeg_quality.clamp(1, 100);
        self.ocr.minimum_confidence = self.ocr.minimum_confidence.clamp(0, 100);
        self.pin.default_opacity = self.pin.default_opacity.clamp(25, 100);
        self.pin.zoom_step = self.pin.zoom_step.clamp(5, 100);

        self.hotkey = self
            .hotkey
            .take()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());

        self.capture.filename_template = self.capture.filename_template.trim().to_owned();
        if self.capture.filename_template.is_empty() {
            return Err(anyhow!(i18n::text(
                "文件名模板不能为空",
                "Filename template cannot be empty"
            )));
        }
        if self
            .capture
            .filename_template
            .chars()
            .any(|ch| matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
        {
            return Err(anyhow!(i18n::text(
                "文件名模板包含 Windows 不允许的字符",
                "Filename template contains characters that Windows does not allow"
            )));
        }

        if self.capture.save_directory.as_os_str().is_empty() {
            self.capture.save_directory = default_picture_directory();
        }
        Ok(())
    }

    pub fn path() -> PathBuf {
        Self::directory().join("config.json")
    }

    pub fn directory() -> PathBuf {
        app_data_directory()
    }

    pub fn log_directory() -> PathBuf {
        Self::directory().join("logs")
    }
}

pub fn app_data_directory() -> PathBuf {
    shi_foundation::config::roaming_app_data_directory("GridStart")
}

#[cfg(test)]
mod tests {
    use super::{Config, OcrEngineKind, default_picture_directory};

    #[test]
    fn defaults_match_product_specification() {
        let config = Config::default();
        assert_eq!(config.hotkey.as_deref(), Some("Ctrl+Alt+C"));
        assert_eq!(config.capture.jpeg_quality, 90);
        assert_eq!(config.capture.save_directory, default_picture_directory());
        assert_eq!(config.ocr.engine, OcrEngineKind::System);
        assert_eq!(config.ocr.minimum_confidence, 60);
        assert_eq!(config.pin.default_opacity, 100);
        assert_eq!(config.pin.zoom_step, 15);
    }

    #[test]
    fn validation_normalizes_ranges_and_rejects_invalid_names() {
        let mut config = Config::default();
        config.capture.jpeg_quality = 0;
        config.ocr.minimum_confidence = 255;
        config.pin.default_opacity = 1;
        config.pin.zoom_step = 255;
        config.validate().unwrap();
        assert_eq!(config.capture.jpeg_quality, 1);
        assert_eq!(config.ocr.minimum_confidence, 100);
        assert_eq!(config.pin.default_opacity, 25);
        assert_eq!(config.pin.zoom_step, 100);

        config.capture.filename_template = "bad/name".to_owned();
        assert!(config.validate().is_err());
    }
}
