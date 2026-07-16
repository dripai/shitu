use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use walkdir::WalkDir;
use windows::{
    Win32::{
        Foundation::RPC_E_CHANGED_MODE,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize, IPersistFile, STGM_READ,
        },
        UI::Shell::{IShellLinkW, SLGP_UNCPRIORITY, ShellLink},
    },
    core::{Interface, PCWSTR},
};

use crate::model::{AppEntry, AppSource};

pub fn scan_apps() -> Vec<AppEntry> {
    let mut seen = HashSet::new();
    let mut apps = Vec::new();

    for (root, source) in scan_roots() {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
        {
            let shortcut_path = entry.path();
            let Some(target_path) = executable_target(shortcut_path) else {
                continue;
            };
            let id = stable_id(&target_path);
            if !seen.insert(id.clone()) {
                continue;
            }
            let name = app_name(shortcut_path);
            let group = group_name(&root, shortcut_path, source);
            apps.push(AppEntry {
                id,
                name,
                group,
                launch_path: target_path,
                source,
            });
        }
    }

    apps.sort_by(|a, b| {
        a.group
            .cmp(&b.group)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    apps
}

fn scan_roots() -> Vec<(PathBuf, AppSource)> {
    let mut roots = Vec::new();
    if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
        roots.push((
            PathBuf::from(program_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
            AppSource::StartMenu,
        ));
    }
    if let Some(app_data) = std::env::var_os("APPDATA") {
        roots.push((
            PathBuf::from(app_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
            AppSource::StartMenu,
        ));
    }
    if let Some(public) = std::env::var_os("PUBLIC") {
        roots.push((PathBuf::from(public).join("Desktop"), AppSource::Desktop));
    }
    if let Some(user_profile) = std::env::var_os("USERPROFILE") {
        roots.push((
            PathBuf::from(user_profile).join("Desktop"),
            AppSource::Desktop,
        ));
    }
    roots
}

fn executable_target(path: &Path) -> Option<PathBuf> {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("exe") => Some(path.to_path_buf()),
        Some("lnk") => resolve_shortcut(path).filter(|target| is_exe(target)),
        _ => None,
    }
}

fn is_exe(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

fn resolve_shortcut(path: &Path) -> Option<PathBuf> {
    let com_scope = ComScope::new()?;
    let link: IShellLinkW =
        unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.ok()?;
    let persist: IPersistFile = link.cast().ok()?;
    let shortcut = wide(path.as_os_str());
    unsafe {
        persist.Load(PCWSTR(shortcut.as_ptr()), STGM_READ).ok()?;
    }

    let mut target = vec![0_u16; 32768];
    unsafe {
        link.GetPath(&mut target, std::ptr::null_mut(), SLGP_UNCPRIORITY.0 as u32)
            .ok()?;
    }
    drop(com_scope);
    let len = target
        .iter()
        .position(|ch| *ch == 0)
        .unwrap_or(target.len());
    if len == 0 {
        return None;
    }
    Some(PathBuf::from(String::from_utf16_lossy(&target[..len])))
}

fn app_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("Application")
        .trim()
        .to_owned()
}

fn group_name(root: &Path, path: &Path, source: AppSource) -> String {
    if source == AppSource::Desktop {
        return "Desktop".to_owned();
    }
    let Ok(relative) = path.strip_prefix(root) else {
        return "Apps".to_owned();
    };
    relative
        .parent()
        .and_then(|parent| parent.components().next())
        .and_then(|component| component.as_os_str().to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Apps")
        .to_owned()
}

fn stable_id(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

struct ComScope {
    should_uninitialize: bool,
}

impl ComScope {
    fn new() -> Option<Self> {
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result.is_ok() {
            return Some(Self {
                should_uninitialize: true,
            });
        }
        (result == RPC_E_CHANGED_MODE).then_some(Self {
            should_uninitialize: false,
        })
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.should_uninitialize {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.as_ref().encode_wide().chain(Some(0)).collect()
}
