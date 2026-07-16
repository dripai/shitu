use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

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
pub enum CompletionAction {
    #[default]
    Copy,
    Save,
    CopyAndSave,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    #[default]
    Png,
    Jpeg,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    pub completion_action: CompletionAction,
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
            completion_action: CompletionAction::Copy,
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
    pub launch_at_startup: bool,
    pub hotkey: Option<String>,
    pub capture: CaptureConfig,
    pub pin: PinConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            appearance: AppearanceMode::System,
            launch_at_startup: false,
            hotkey: None,
            capture: CaptureConfig::default(),
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
                return Err(error).with_context(|| format!("读取配置失败：{}", path.display()));
            }
        };
        let mut config: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("配置文件格式无效：{}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let mut config = self.clone();
        config.validate()?;

        let path = Self::path();
        let parent = path.parent().ok_or_else(|| anyhow!("配置路径没有父目录"))?;
        fs::create_dir_all(parent)
            .with_context(|| format!("创建配置目录失败：{}", parent.display()))?;

        let temp_path = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(&config)?;
        let write_result = (|| -> Result<()> {
            let mut file = fs::File::create(&temp_path)
                .with_context(|| format!("创建临时配置失败：{}", temp_path.display()))?;
            file.write_all(&json)
                .with_context(|| format!("写入临时配置失败：{}", temp_path.display()))?;
            file.sync_all()
                .with_context(|| format!("同步临时配置失败：{}", temp_path.display()))?;
            atomic_replace(&temp_path, &path)
        })();

        if write_result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        write_result
    }

    pub fn validate(&mut self) -> Result<()> {
        self.capture.jpeg_quality = self.capture.jpeg_quality.clamp(1, 100);
        self.pin.default_opacity = self.pin.default_opacity.clamp(25, 100);
        self.pin.zoom_step = self.pin.zoom_step.clamp(5, 100);

        self.hotkey = self
            .hotkey
            .take()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());

        self.capture.filename_template = self.capture.filename_template.trim().to_owned();
        if self.capture.filename_template.is_empty() {
            return Err(anyhow!("文件名模板不能为空"));
        }
        if self
            .capture
            .filename_template
            .chars()
            .any(|ch| matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
        {
            return Err(anyhow!("文件名模板包含 Windows 不允许的字符"));
        }

        if self.capture.save_directory.as_os_str().is_empty() {
            self.capture.save_directory = default_picture_directory();
        }
        Ok(())
    }

    pub fn path() -> PathBuf {
        app_data_directory().join("config.json")
    }

    pub fn log_directory() -> PathBuf {
        app_data_directory().join("logs")
    }

    pub fn third_party_licenses_path() -> PathBuf {
        app_data_directory().join("THIRD_PARTY_LICENSES.md")
    }
}

pub fn app_data_directory() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("GridStart")
}

pub fn default_picture_directory() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Pictures")
}

#[cfg(windows)]
fn atomic_replace(source: &Path, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::Storage::FileSystem::{
            MOVE_FILE_FLAGS, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        core::PCWSTR,
    };

    let source: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let target: Vec<u16> = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let flags = MOVE_FILE_FLAGS(MOVEFILE_REPLACE_EXISTING.0 | MOVEFILE_WRITE_THROUGH.0);
    unsafe { MoveFileExW(PCWSTR(source.as_ptr()), PCWSTR(target.as_ptr()), flags) }
        .with_context(|| format!("替换配置文件失败：{}", Config::path().display()))
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, target: &Path) -> Result<()> {
    fs::rename(source, target).with_context(|| format!("替换配置文件失败：{}", target.display()))
}

#[cfg(test)]
mod tests {
    use super::{Config, default_picture_directory};

    #[test]
    fn defaults_match_product_specification() {
        let config = Config::default();
        assert!(config.hotkey.is_none());
        assert_eq!(config.capture.jpeg_quality, 90);
        assert_eq!(config.capture.save_directory, default_picture_directory());
        assert_eq!(config.pin.default_opacity, 100);
        assert_eq!(config.pin.zoom_step, 15);
    }

    #[test]
    fn validation_normalizes_ranges_and_rejects_invalid_names() {
        let mut config = Config::default();
        config.capture.jpeg_quality = 0;
        config.pin.default_opacity = 1;
        config.pin.zoom_step = 255;
        config.validate().unwrap();
        assert_eq!(config.capture.jpeg_quality, 1);
        assert_eq!(config.pin.default_opacity, 25);
        assert_eq!(config.pin.zoom_step, 100);

        config.capture.filename_template = "bad/name".to_owned();
        assert!(config.validate().is_err());
    }
}
