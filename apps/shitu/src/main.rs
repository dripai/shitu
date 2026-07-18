#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod capture;
mod config;
mod hotkey;
mod image;
mod output;
mod platform;

pub use shi_foundation::{i18n, logging};

fn main() -> Result<(), slint::PlatformError> {
    #[cfg(windows)]
    if let Some(exit_code) = platform::ocr::worker_exit_code() {
        std::process::exit(exit_code);
    }

    app::run()
}
