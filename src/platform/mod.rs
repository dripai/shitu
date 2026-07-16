use std::path::Path;

#[cfg(not(windows))]
use anyhow::Context;
use anyhow::Result;

pub mod clock;
pub mod ocr;

#[cfg(windows)]
pub mod windows;

#[cfg(windows)]
pub fn replace_file(source: &Path, target: &Path) -> Result<()> {
    windows::file::replace(source, target)
}

#[cfg(not(windows))]
pub fn replace_file(source: &Path, target: &Path) -> Result<()> {
    std::fs::rename(source, target).with_context(|| format!("替换文件失败：{}", target.display()))
}
