#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod application;
mod config;
mod output;
mod platform;
mod ui;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    ui::run()
}
