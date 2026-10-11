use super::*;
use std::{collections::HashSet, os::windows::ffi::OsStrExt};
use windows::{
    Win32::{
        System::Com::{
            CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
        },
        UI::Shell::{
            FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING, FOFX_ADDUNDORECORD,
            FOFX_EARLYFAILURE, FOFX_RECYCLEONDELETE, FileOperation, IFileOperation, IShellItem,
            SHCreateItemFromParsingName,
        },
    },
    core::PCWSTR,
};

pub struct RemovalReport {
    pub removed: Vec<PathBuf>,
    pub remaining: Vec<PathBuf>,
    pub error: Option<String>,
}

// IFileOperation is not transactional. Validate/queue the entire selection
// first, then report actual remaining paths even if PerformOperations fails.
// Keep WANTNUKEWARNING: unavailable recycle support must not silently turn
// into permanent deletion. No filesystem-delete fallback is provided.
pub fn recycle(paths: &[PathBuf]) -> Result<RemovalReport> {
    let paths = validate(paths)?;
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        struct Com;
        impl Drop for Com {
            fn drop(&mut self) {
                unsafe { CoUninitialize() }
            }
        }
        let _com = Com;
        let operation: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
        operation.SetOperationFlags(
            FOFX_RECYCLEONDELETE
                | FOFX_ADDUNDORECORD
                | FOFX_EARLYFAILURE
                | FOF_NOERRORUI
                | FOF_NOCONFIRMATION
                | FOF_SILENT
                | FOF_WANTNUKEWARNING,
        )?;
        for path in &paths {
            let absolute = std::path::absolute(path)?;
            let path_wide: Vec<u16> = absolute.as_os_str().encode_wide().chain(Some(0)).collect();
            let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(path_wide.as_ptr()), None)
                .with_context(|| path.display().to_string())?;
            operation
                .DeleteItem(&item, None)
                .with_context(|| path.display().to_string())?;
        }
        let mut error = operation.PerformOperations().err().map(|e| e.to_string());
        match operation.GetAnyOperationsAborted() {
            Ok(aborted) if aborted.as_bool() && error.is_none() => {
                error = Some(crate::i18n::text("删除未完成").into())
            }
            Err(e) => error = Some(e.to_string()),
            _ => {}
        }
        let mut report = RemovalReport {
            removed: Vec::new(),
            remaining: Vec::new(),
            error,
        };
        for path in paths {
            match path.try_exists() {
                Ok(false) => report.removed.push(path),
                Ok(true) => report.remaining.push(path),
                Err(e) => {
                    report.error = Some(format!("{}: {e}", path.display()));
                    report.remaining.push(path);
                }
            }
        }
        if !report.remaining.is_empty() && report.error.is_none() {
            report.error = Some(crate::i18n::text("删除未完成").into());
        }
        Ok(report)
    }
}

// Explicit permanent-delete operation, never a fallback for recycling.
// Preflight the entire group, then stop on the first failure and report the
// irreversible partial result. remove_file cannot recursively delete a folder.
pub fn permanently_delete(paths: &[PathBuf]) -> Result<RemovalReport> {
    let paths = validate(paths)?;
    let mut report = RemovalReport {
        removed: Vec::new(),
        remaining: Vec::new(),
        error: None,
    };
    for (index, path) in paths.iter().enumerate() {
        match fs::remove_file(path) {
            Ok(()) => report.removed.push(path.clone()),
            Err(error) => {
                report.error = Some(format!("{}: {error}", path.display()));
                report.remaining.extend_from_slice(&paths[index..]);
                break;
            }
        }
    }
    Ok(report)
}

fn validate(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    ensure!(!paths.is_empty(), "No pictures selected");
    let mut unique = HashSet::new();
    let mut result = Vec::new();
    for path in paths {
        ensure!(
            path.is_file() && is_picture(path),
            "{}: {}",
            crate::i18n::text("图片不存在或不受支持"),
            path.display()
        );
        let absolute = std::path::absolute(path)?;
        if unique.insert(absolute.clone()) {
            result.push(path.clone());
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{permanently_delete, recycle, validate};
    use anyhow::Result;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    #[test]
    fn permanent_delete_removes_only_selected_files_and_deduplicates() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "shitu-delete-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir(&root)?;
        let selected = root.join("selected.png");
        let untouched = root.join("untouched.jpg");
        fs::write(&selected, b"selected fixture")?;
        fs::write(&untouched, b"untouched fixture")?;
        let report = permanently_delete(&[selected.clone(), selected.clone()])?;
        assert_eq!(report.removed, vec![selected.clone()]);
        assert!(report.error.is_none() && report.remaining.is_empty());
        assert!(!selected.exists());
        assert_eq!(fs::read(&untouched)?, b"untouched fixture");
        fs::remove_file(untouched)?;
        fs::remove_dir(root)?;
        Ok(())
    }

    #[test]
    fn permanent_delete_preflights_whole_selection_and_rejects_directories() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "shitu-delete-preflight-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir(&root)?;
        let selected = root.join("selected.png");
        let folder = root.join("folder.png");
        let unsupported = root.join("keep.txt");
        fs::write(&selected, b"fixture")?;
        fs::write(&unsupported, b"keep")?;
        fs::create_dir(&folder)?;
        for invalid in [
            root.join("missing.png"),
            folder.clone(),
            unsupported.clone(),
        ] {
            assert!(permanently_delete(&[selected.clone(), invalid]).is_err());
            assert_eq!(fs::read(&selected)?, b"fixture");
        }
        assert!(permanently_delete(&[]).is_err());
        fs::remove_file(selected)?;
        fs::remove_file(unsupported)?;
        fs::remove_dir(folder)?;
        fs::remove_dir(root)?;
        Ok(())
    }

    #[test]
    fn permanent_delete_reports_partial_failure_and_can_retry_remaining() -> Result<()> {
        use std::os::windows::fs::OpenOptionsExt;
        let root = std::env::temp_dir().join(format!(
            "shitu-delete-partial-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir(&root)?;
        let paths = [
            root.join("first.png"),
            root.join("locked.png"),
            root.join("last.png"),
        ];
        for path in &paths {
            fs::write(path, b"fixture")?;
        }
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&paths[1])?;
        let report = permanently_delete(&paths)?;
        assert_eq!(report.removed, paths[..1]);
        assert_eq!(report.remaining, paths[1..]);
        assert!(report.error.is_some());
        assert!(!paths[0].exists());
        assert!(paths[1].exists() && paths[2].exists());
        drop(locked);
        let retried = permanently_delete(&report.remaining)?;
        assert!(retried.error.is_none() && retried.remaining.is_empty());
        assert_eq!(retried.removed, paths[1..]);
        fs::remove_dir(root)?;
        Ok(())
    }
    #[test]
    fn batch_preflight_keeps_all_files_when_an_input_is_missing() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "shitu-recycle-preflight-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir(&root)?;
        let file = root.join("test.png");
        fs::write(&file, b"fixture")?;
        let result = (|| -> Result<()> {
            assert_eq!(validate(&[file.clone(), file.clone()])?.len(), 1);
            assert!(recycle(&[file.clone(), root.join("missing.png")]).is_err());
            assert_eq!(fs::read(&file)?, b"fixture");
            assert!(recycle(&[]).is_err());
            Ok(())
        })();
        fs::remove_dir_all(root)?;
        result
    }

    #[test]
    #[ignore = "Moves only freshly created fixtures to the real Windows Recycle Bin"]
    fn windows_batch_recycle_moves_only_its_test_fixtures() -> Result<()> {
        let root = std::env::current_dir()?.join(".codex-tmp").join(format!(
            "recycle-test-{}",
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir_all(&root)?;
        let paths = [root.join("one.png"), root.join("two.png")];
        for path in &paths {
            image::RgbaImage::new(2, 2).save(path)?;
        }
        let report = recycle(&paths)?;
        assert!(report.error.is_none(), "{:?}", report.error);
        assert!(report.remaining.is_empty());
        assert_eq!(report.removed.len(), 2);
        assert!(paths.iter().all(|p| !p.exists()));

        // A sharing violation must surface as remaining items, never an
        // unconditional success. Shell may stop before or after the first item.
        for path in &paths {
            image::RgbaImage::new(2, 2).save(path)?;
        }
        use std::os::windows::fs::OpenOptionsExt;
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&paths[1])?;
        let report = recycle(&paths)?;
        assert!(report.error.is_some());
        assert!(report.remaining.contains(&paths[1]));
        for path in &paths {
            assert_eq!(report.remaining.contains(path), path.exists());
            assert_eq!(report.removed.contains(path), !path.exists());
        }
        drop(locked);
        let cleanup = recycle(&report.remaining)?;
        assert!(cleanup.error.is_none() && cleanup.remaining.is_empty());
        fs::remove_dir(root)?;
        Ok(())
    }
}
