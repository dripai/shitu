use std::{fs, io::Write, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub source_mode: u8,
    pub quality_preset: u8,
    pub frame_rate: u8,
    pub system_audio: bool,
    pub microphone: bool,
    pub show_cursor: bool,
    pub highlight_clicks: bool,
    pub countdown_seconds: u8,
    pub save_directory: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            source_mode: 0,
            quality_preset: 2,
            frame_rate: 0,
            system_audio: true,
            microphone: false,
            show_cursor: true,
            highlight_clicks: false,
            countdown_seconds: 3,
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
                return Err(error).with_context(|| format!("读取配置失败：{}", path.display()));
            }
        };
        let mut config: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("配置文件格式无效：{}", path.display()))?;
        config.validate();
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let mut config = self.clone();
        config.validate();
        let path = Self::path();
        let parent = path.parent().ok_or_else(|| anyhow!("配置路径没有父目录"))?;
        fs::create_dir_all(parent)
            .with_context(|| format!("创建配置目录失败：{}", parent.display()))?;

        let temporary = path.with_extension("json.tmp");
        let result = (|| -> Result<()> {
            let bytes = serde_json::to_vec_pretty(&config)?;
            let mut file = fs::File::create(&temporary)
                .with_context(|| format!("创建临时配置失败：{}", temporary.display()))?;
            file.write_all(&bytes)
                .with_context(|| format!("写入临时配置失败：{}", temporary.display()))?;
            file.sync_all()
                .with_context(|| format!("同步临时配置失败：{}", temporary.display()))?;
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
    use super::{Config, default_video_directory};

    #[test]
    fn defaults_match_product_design() {
        let config = Config::default();
        assert_eq!(config.source_mode, 0);
        assert_eq!(config.quality_preset, 2);
        assert_eq!(config.frame_rate, 0);
        assert!(config.system_audio);
        assert!(!config.microphone);
        assert!(config.show_cursor);
        assert_eq!(config.countdown_seconds, 3);
        assert_eq!(config.save_directory, default_video_directory());
    }

    #[test]
    fn old_or_invalid_values_are_normalized() {
        let mut config: Config = serde_json::from_str(
            r#"{"source_mode":9,"quality_preset":9,"frame_rate":9,"countdown_seconds":99}"#,
        )
        .unwrap();
        config.validate();
        assert_eq!(config.source_mode, 2);
        assert_eq!(config.quality_preset, 3);
        assert_eq!(config.frame_rate, 1);
        assert_eq!(config.countdown_seconds, 10);
    }
}
