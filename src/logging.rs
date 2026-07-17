use std::{
    fs::{self, OpenOptions},
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::config::Config;

pub fn initialize() {
    std::panic::set_hook(Box::new(|panic| {
        let location = panic
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()))
            .unwrap_or_else(|| "unknown location".to_owned());
        let message = if let Some(message) = panic.payload().downcast_ref::<&str>() {
            (*message).to_owned()
        } else if let Some(message) = panic.payload().downcast_ref::<String>() {
            message.clone()
        } else {
            "unknown panic payload".to_owned()
        };
        event("PANIC", &format!("{location}: {message}"));
    }));
    event("INFO", "application started");
}

pub fn info(message: impl AsRef<str>) {
    event("INFO", message.as_ref());
}

pub fn error(message: impl AsRef<str>) {
    event("ERROR", message.as_ref());
}

fn event(level: &str, message: &str) {
    let directory = Config::log_directory();
    if fs::create_dir_all(&directory).is_err() {
        return;
    }
    let path = directory.join("gridstart.log");
    let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) else {
        return;
    };
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default();
    let sanitized = message.replace(['\r', '\n'], " ");
    let _ = writeln!(file, "{timestamp} {level} {sanitized}");
}
