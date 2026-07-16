#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod capture;
mod config;
mod hotkey;
mod pin;

fn main() -> Result<(), slint::PlatformError> {
    app::run()
}
