//! Filesystem-backed gallery. Entries are references, never imported copies.
mod removal;
pub use removal::{RemovalReport, permanently_delete, recycle};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    #[default]
    Thumbnails,
    List,
}
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sort {
    #[default]
    Newest,
    Name,
    Type,
    Size,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub folders: Vec<PathBuf>,
    pub current: Option<PathBuf>,
    pub view: ViewMode,
    pub sort: Sort,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            folders: Vec::new(),
            current: None,
            view: ViewMode::Thumbnails,
            sort: Sort::Newest,
        }
    }
}
impl Preferences {
    pub fn load() -> Result<Self> {
        match fs::read(crate::config::Config::directory().join("gallery.json")) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save(&self) -> Result<()> {
        self.save_to(&crate::config::Config::directory())
    }
    fn save_to(&self, directory: &Path) -> Result<()> {
        fs::create_dir_all(directory)?;
        let temp = directory.join("gallery.json.tmp");
        let result = (|| {
            let mut file = fs::File::create(&temp)?;
            file.write_all(&serde_json::to_vec_pretty(self)?)?;
            file.sync_all()?;
            drop(file);
            crate::platform::replace_file(&temp, &directory.join("gallery.json"))
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
}

#[derive(Clone)]
pub struct Picture {
    pub path: PathBuf,
    pub name: String,
    pub bytes: u64,
    pub modified: SystemTime,
}

pub fn is_picture(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg"))
}

pub fn scan(directory: &Path) -> Result<(Vec<PathBuf>, Vec<Picture>)> {
    let mut folders = Vec::new();
    let mut pictures = Vec::new();
    for entry in fs::read_dir(directory).with_context(|| directory.display().to_string())? {
        let entry = entry?;
        // Do not follow symlinks/junctions into another tree or a cycle.
        let metadata = entry.metadata()?;
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            continue;
        }
        if metadata.is_dir() {
            folders.push(entry.path());
        } else if metadata.is_file() && is_picture(&entry.path()) {
            pictures.push(Picture {
                path: entry.path(),
                name: entry.file_name().to_string_lossy().into_owned(),
                bytes: metadata.len(),
                modified: metadata.modified()?,
            });
        }
    }
    folders.sort_by_key(|path| path.file_name().map(|n| n.to_ascii_lowercase()));
    Ok((folders, pictures))
}

pub fn sort_pictures(pictures: &mut [Picture], order: Sort) {
    pictures.sort_by(|a, b| {
        let primary = match order {
            Sort::Newest => b.modified.cmp(&a.modified),
            Sort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Sort::Type => a
                .path
                .extension()
                .map(|ext| ext.to_ascii_lowercase())
                .cmp(&b.path.extension().map(|ext| ext.to_ascii_lowercase())),
            Sort::Size => b.bytes.cmp(&a.bytes),
        };
        primary.then_with(|| a.name.cmp(&b.name))
    });
}

pub fn rename_target(source: &Path, stem: &str) -> Result<PathBuf> {
    let stem = stem.trim();
    ensure!(
        !stem.is_empty() && !stem.ends_with(['.', ' ']) && stem.encode_utf16().count() <= 220,
        "{}",
        crate::i18n::text("文件名无效")
    );
    ensure!(
        !stem
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c)),
        "{}",
        crate::i18n::text("文件名无效")
    );
    let reserved = stem
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    ensure!(
        !matches!(reserved.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            && !(reserved.len() == 4
                && (reserved.starts_with("COM") || reserved.starts_with("LPT"))
                && matches!(reserved.as_bytes()[3], b'1'..=b'9')),
        "{}",
        crate::i18n::text("文件名无效")
    );
    let extension = source.extension().context("Image extension is missing")?;
    let mut name = std::ffi::OsString::from(stem);
    name.push(".");
    name.push(extension);
    Ok(source.with_file_name(name))
}

pub fn move_picture(source: &Path, target: &Path) -> Result<()> {
    ensure!(
        source.is_file() && is_picture(source),
        "{}",
        crate::i18n::text("图片不存在或不受支持")
    );
    if source == target {
        return Ok(());
    }
    ensure!(
        !target.try_exists()?,
        "{}",
        crate::i18n::text("目标文件已存在")
    );
    // MoveFileEx without REPLACE_EXISTING also protects against a target created
    // after the check. COPY_ALLOWED handles cross-volume moves. Windows may
    // report success if source deletion failed; detect and roll that copy back.
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::Storage::FileSystem::{MOVEFILE_COPY_ALLOWED, MoveFileExW},
        core::PCWSTR,
    };
    let src: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let dst: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        MoveFileExW(
            PCWSTR(src.as_ptr()),
            PCWSTR(dst.as_ptr()),
            MOVEFILE_COPY_ALLOWED,
        )
    }
    .with_context(|| format!("{} → {}", source.display(), target.display()))?;
    if source.try_exists()? {
        fs::remove_file(target)
            .context("Move did not remove source; failed to roll back destination copy")?;
        anyhow::bail!("Move did not remove source; destination copy rolled back");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sorting_respects_each_field_and_groups_extension_case_insensitively() {
        let pictures = [("b.PNG", 10, 30), ("c.jpg", 30, 10), ("a.png", 20, 20)].map(
            |(name, bytes, seconds)| Picture {
                path: PathBuf::from(name),
                name: name.to_owned(),
                bytes,
                modified: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(seconds),
            },
        );
        for (order, expected) in [
            (Sort::Newest, ["b.PNG", "a.png", "c.jpg"]),
            (Sort::Name, ["a.png", "b.PNG", "c.jpg"]),
            (Sort::Type, ["c.jpg", "a.png", "b.PNG"]),
            (Sort::Size, ["c.jpg", "a.png", "b.PNG"]),
        ] {
            let mut sorted = pictures.clone();
            sort_pictures(&mut sorted, order);
            assert_eq!(sorted.map(|picture| picture.name), expected);
        }
    }

    #[test]
    fn names_keep_extension_and_reject_paths_and_devices() {
        let source = Path::new("D:/images/old.PNG");
        assert_eq!(
            rename_target(source, "新图").unwrap(),
            Path::new("D:/images/新图.PNG")
        );
        for name in ["", "../escape", "a/b", "a:b", "CON", "lpt1.txt", "a."] {
            assert!(rename_target(source, name).is_err(), "{name}");
        }
    }
    #[test]
    fn files_are_scanned_nonrecursively_and_moved_without_overwrite() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "shitu-gallery-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir_all(root.join("child"))?;
        let result = (|| -> Result<()> {
            fs::write(root.join("one.PNG"), b"source")?;
            fs::write(root.join("ignored.txt"), b"ignore")?;
            fs::write(root.join("child/two.jpg"), b"target")?;
            let (folders, pictures) = scan(&root)?;
            assert_eq!(folders.len(), 1);
            assert_eq!(pictures.len(), 1);
            assert!(move_picture(&root.join("one.PNG"), &root.join("child/two.jpg")).is_err());
            assert_eq!(fs::read(root.join("child/two.jpg"))?, b"target");
            assert_eq!(fs::read(root.join("one.PNG"))?, b"source");
            move_picture(&root.join("one.PNG"), &root.join("child/one.PNG"))?;
            assert!(!root.join("one.PNG").exists());
            assert_eq!(fs::read(root.join("child/one.PNG"))?, b"source");
            use std::os::windows::fs::OpenOptionsExt;
            let locked = fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(root.join("child/one.PNG"))?;
            assert!(move_picture(&root.join("child/one.PNG"), &root.join("locked.PNG")).is_err());
            assert!(!root.join("locked.PNG").exists());
            assert_eq!(fs::read(root.join("child/one.PNG"))?, b"source");
            drop(locked);
            let prefs = Preferences {
                folders: vec![root.join("child")],
                ..Default::default()
            };
            prefs.save_to(&root)?;
            let saved: Preferences = serde_json::from_slice(&fs::read(root.join("gallery.json"))?)?;
            assert_eq!(saved.folders, prefs.folders);
            // Removing a library reference only changes preferences.
            Preferences::default().save_to(&root)?;
            assert_eq!(fs::read(root.join("child/one.PNG"))?, b"source");
            Ok(())
        })();
        fs::remove_dir_all(&root)?;
        result
    }
}
