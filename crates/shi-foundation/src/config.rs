use std::path::PathBuf;

/// Returns the roaming application-data directory for one product in the suite.
pub fn roaming_app_data_directory(product_directory: &str) -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(product_directory)
}

/// Returns the user's conventional Pictures directory.
pub fn default_picture_directory() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Pictures")
}
