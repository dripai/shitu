#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod capture;
mod config;
mod hotkey;
mod image;
mod logging;
mod output;
mod platform;

fn main() -> Result<(), slint::PlatformError> {
    app::run()
}
