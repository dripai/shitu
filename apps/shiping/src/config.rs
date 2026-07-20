use std::{fs, io::Write, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
pub use shi_foundation::LanguageMode;

use shi_foundation::i18n;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub language: LanguageMode,
    pub source_mode: u8,
    pub quality_preset: u8,
    pub frame_rate: u8,
    pub system_audio: bool,
    pub microphone: bool,
    pub show_cursor: bool,
    pub highlight_clicks: bool,
    pub countdown_seconds: u8,
    pub auto_minimize_after_start: bool,
    pub open_directory_after_stop: bool,
    pub start_hotkey: Option<String>,
    pub pause_hotkey: Option<String>,
    pub stop_hotkey: Option<String>,
    pub save_directory: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: LanguageMode::English,
            source_mode: 0,
            quality_preset: 2,
            frame_rate: 0,
            system_audio: true,
            microphone: false,
            show_cursor: true,
            highlight_clicks: false,
            countdown_seconds: 3,
            auto_minimize_after_start: false,
            open_directory_after_stop: false,
            start_hotkey: Some("F10".to_owned()),
            pause_hotkey: Some("F11".to_owned()),
            stop_hotkey: Some("F12".to_owned()),
            save_directory: default_video_directory(),
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
        config.validate();
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let mut config = self.clone();
        config.validate();
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
                i18n::text(
                    "创建配置目录失败",
                    "Failed to create the settings directory"
                ),
                parent.display()
            )
        })?;

        let temporary = path.with_extension("json.tmp");
        let result = (|| -> Result<()> {
            let bytes = serde_json::to_vec_pretty(&config)?;
            let mut file = fs::File::create(&temporary).with_context(|| {
                format!(
                    "{}: {}",
                    i18n::text(
                        "创建临时配置失败",
                        "Failed to create the temporary settings file"
                    ),
                    temporary.display()
                )
            })?;
            file.write_all(&bytes).with_context(|| {
                format!(
                    "{}: {}",
                    i18n::text(
                        "写入临时配置失败",
                        "Failed to write the temporary settings file"
                    ),
                    temporary.display()
                )
            })?;
            file.sync_all().with_context(|| {
                format!(
                    "{}: {}",
                    i18n::text(
                        "同步临时配置失败",
                        "Failed to sync the temporary settings file"
                    ),
                    temporary.display()
                )
            })?;
            replace_file(&temporary, &path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    pub fn path() -> PathBuf {
        shi_foundation::config::roaming_app_data_directory("ShiPing").join("config.json")
    }

    fn validate(&mut self) {
        self.source_mode = self.source_mode.min(2);
        self.quality_preset = self.quality_preset.min(3);
        self.frame_rate = self.frame_rate.min(1);
        self.countdown_seconds = self.countdown_seconds.min(10);
        if self.save_directory.as_os_str().is_empty() {
            self.save_directory = default_video_directory();
        }
    }

    pub fn hotkeys(&self) -> [Option<String>; 3] {
        [
            self.start_hotkey.clone(),
            self.pause_hotkey.clone(),
            self.stop_hotkey.clone(),
        ]
    }

    pub fn set_hotkeys(&mut self, hotkeys: [Option<String>; 3]) {
        [self.start_hotkey, self.pause_hotkey, self.stop_hotkey] = hotkeys;
    }
}

pub fn default_video_directory() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Videos")
        .join("ShiPing")
}

fn replace_file(source: &std::path::Path, target: &std::path::Path) -> Result<()> {
    crate::platform::replace_file(source, target)
}

#[cfg(test)]
mod tests {
    use super::{Config, LanguageMode, default_video_directory};

    #[test]
    fn defaults_match_product_design() {
        let config = Config::default();
        assert_eq!(config.language, LanguageMode::English);
        assert_eq!(config.source_mode, 0);
        assert_eq!(config.quality_preset, 2);
        assert_eq!(config.frame_rate, 0);
        assert!(config.system_audio);
        assert!(!config.microphone);
        assert!(config.show_cursor);
        assert_eq!(config.countdown_seconds, 3);
        assert!(!config.auto_minimize_after_start);
        assert!(!config.open_directory_after_stop);
        assert_eq!(config.start_hotkey.as_deref(), Some("F10"));
        assert_eq!(config.pause_hotkey.as_deref(), Some("F11"));
        assert_eq!(config.stop_hotkey.as_deref(), Some("F12"));
        assert_eq!(config.save_directory, default_video_directory());
    }

    #[test]
    fn old_or_invalid_values_are_normalized() {
        let mut config: Config = serde_json::from_str(
            r#"{"source_mode":9,"quality_preset":9,"frame_rate":9,"countdown_seconds":99}"#,
        )
        .unwrap();
        config.validate();
        assert_eq!(config.language, LanguageMode::English);
        assert_eq!(config.source_mode, 2);
        assert_eq!(config.quality_preset, 3);
        assert_eq!(config.frame_rate, 1);
        assert_eq!(config.countdown_seconds, 10);
        assert_eq!(config.start_hotkey.as_deref(), Some("F10"));
        assert_eq!(config.pause_hotkey.as_deref(), Some("F11"));
        assert_eq!(config.stop_hotkey.as_deref(), Some("F12"));
    }

    #[test]
    fn selected_language_is_serialized() {
        let config = Config {
            language: LanguageMode::Chinese,
            ..Config::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"language\":\"chinese\""));

        let restored: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.language, LanguageMode::Chinese);
    }
}
