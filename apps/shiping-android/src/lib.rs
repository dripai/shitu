mod permission;

#[cfg(target_os = "android")]
mod android;

slint::include_modules!();

pub use permission::PermissionState;
