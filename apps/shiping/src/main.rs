#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod application;
mod config;
mod domain;
mod output;
mod platform;
mod ports;
mod ui;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    ui::run()
}
