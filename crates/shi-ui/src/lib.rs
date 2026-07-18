use std::path::PathBuf;

/// Returns the Slint library directory exposed to application build scripts.
pub fn slint_library_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui")
}
