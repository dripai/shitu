#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod capture;
mod config;
mod hotkey;
mod i18n;
mod image;
mod locale;
mod logging;
mod output;
mod paths;
mod platform;
mod settings;

fn main() -> anyhow::Result<()> {
    #[cfg(windows)]
    if let Some(exit_code) = platform::ocr::worker_exit_code() {
        std::process::exit(exit_code);
    }

    let result = app::run(platform::windows::startup::start_minimized_requested());
    if let Err(error) = &result {
        logging::error(format!("Startup failed: {error:#}"));
        // Release builds have no console: report startup failure explicitly.
        rfd::MessageDialog::new()
            .set_title(i18n::text("拾图"))
            .set_level(rfd::MessageLevel::Error)
            .set_description(format!("{}: {error:#}", i18n::text("操作失败")))
            .show();
    }
    result
}
