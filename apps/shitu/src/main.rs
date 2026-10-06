#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod capture;
mod config;
mod hotkey;
mod i18n;
mod image;
mod output;
mod platform;

pub use shi_foundation::logging;

fn main() -> Result<(), slint::PlatformError> {
    #[cfg(windows)]
    if let Some(exit_code) = platform::ocr::worker_exit_code() {
        std::process::exit(exit_code);
    }

    app::run(platform::windows::startup::start_minimized_requested())
}
