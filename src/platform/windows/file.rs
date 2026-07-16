use std::{os::windows::ffi::OsStrExt, path::Path};

use anyhow::{Context, Result};
use windows::{
    Win32::Storage::FileSystem::{
        MOVE_FILE_FLAGS, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    },
    core::PCWSTR,
};

pub fn replace(source: &Path, target: &Path) -> Result<()> {
    let target_display = target.display().to_string();
    let source_wide: Vec<u16> = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let target_wide: Vec<u16> = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let flags = MOVE_FILE_FLAGS(MOVEFILE_REPLACE_EXISTING.0 | MOVEFILE_WRITE_THROUGH.0);
    unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(target_wide.as_ptr()),
            flags,
        )
    }
    .with_context(|| format!("替换文件失败：{target_display}"))
}
