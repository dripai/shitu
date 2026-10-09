use std::path::PathBuf;

/// Returns the platform configuration directory for one product in the suite.
pub fn roaming_app_data_directory(product_directory: &str) -> PathBuf {
    platform_config_root().join(product_directory)
}

/// Returns the user's conventional Pictures directory.
pub fn default_picture_directory() -> PathBuf {
    home_directory().join("Pictures")
}

#[cfg(target_os = "windows")]
fn platform_config_root() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(target_os = "macos")]
fn platform_config_root() -> PathBuf {
    home_directory().join("Library").join("Application Support")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_config_root() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_directory().join(".config"))
}

#[cfg(not(any(windows, unix)))]
fn platform_config_root() -> PathBuf {
    PathBuf::from(".")
}

#[cfg(target_os = "windows")]
fn home_directory() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(not(target_os = "windows"))]
fn home_directory() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
