use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow};
use shi_foundation::i18n;

pub struct OutputPaths {
    pub partial: PathBuf,
    pub final_path: PathBuf,
}

pub fn prepare(directory: &Path) -> Result<OutputPaths> {
    fs::create_dir_all(directory).with_context(|| {
        format!(
            "{}: {}",
            i18n::text("创建保存目录失败", "Failed to create the save folder"),
            directory.display()
        )
    })?;
    let timestamp = timestamp();
    for suffix in 0..10_000_u32 {
        let stem = if suffix == 0 {
            format!("Recording_{timestamp}")
        } else {
            format!("Recording_{timestamp}_{suffix}")
        };
        let final_path = directory.join(format!("{stem}.mp4"));
        let partial = directory.join(format!(".{stem}.partial.mp4"));
        if !final_path.exists() && !partial.exists() {
            return Ok(OutputPaths {
                partial,
                final_path,
            });
        }
    }
    Err(anyhow!(i18n::text(
        "无法生成不重复的录制文件名",
        "Could not generate a unique recording filename"
    )))
}

fn timestamp() -> String {
    crate::platform::local_timestamp()
}

pub fn commit(paths: &OutputPaths) -> Result<()> {
    fs::rename(&paths.partial, &paths.final_path).with_context(|| {
        format!(
            "{}: {} -> {}",
            i18n::text("完成录制文件失败", "Failed to finalize the recorded file"),
            paths.partial.display(),
            paths.final_path.display()
        )
    })
}

pub fn discard_partial(paths: &OutputPaths) {
    let _ = fs::remove_file(&paths.partial);
}

#[cfg(test)]
mod tests {
    use super::prepare;

    #[test]
    fn output_uses_mp4_and_hidden_partial_file() {
        let directory =
            std::env::temp_dir().join(format!("shiping-output-test-{}", std::process::id()));
        let paths = prepare(&directory).unwrap();
        assert_eq!(
            paths
                .final_path
                .extension()
                .and_then(|value| value.to_str()),
            Some("mp4")
        );
        assert!(
            paths
                .partial
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.starts_with('.') && value.ends_with(".partial.mp4"))
        );
        let _ = std::fs::remove_dir(directory);
    }
}
