use std::{mem::size_of, os::windows::ffi::OsStrExt, path::Path};

use anyhow::{Context, Result};
use windows::{
    Win32::{
        Foundation::ERROR_FILE_NOT_FOUND,
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
            RegCreateKeyExW, RegDeleteValueW, RegSetValueExW,
        },
    },
    core::PCWSTR,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "GridStartCapture";
const START_MINIMIZED_ARGUMENT: &str = "--minimized";

pub fn set_enabled(enabled: bool, start_minimized: bool) -> Result<()> {
    let mut key = HKEY::default();
    let subkey = wide(RUN_KEY);
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    status.ok().context("打开开机启动注册表失败")?;

    let result = if enabled {
        let executable = std::env::current_exe().context("无法获取当前程序路径")?;
        let command = startup_command(&executable, start_minimized);
        let wide_command: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = unsafe {
            std::slice::from_raw_parts(
                wide_command.as_ptr().cast::<u8>(),
                wide_command.len() * size_of::<u16>(),
            )
        };
        let name = wide(VALUE_NAME);
        unsafe { RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(bytes)) }
            .ok()
            .context("写入开机启动设置失败")
    } else {
        let name = wide(VALUE_NAME);
        let status = unsafe { RegDeleteValueW(key, PCWSTR(name.as_ptr())) };
        if status == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            status.ok().context("删除开机启动设置失败")
        }
    };
    unsafe {
        let _ = RegCloseKey(key);
    }
    result
}

pub fn start_minimized_requested() -> bool {
    std::env::args_os()
        .skip(1)
        .any(|argument| argument == std::ffi::OsStr::new(START_MINIMIZED_ARGUMENT))
}

fn startup_command(executable: &Path, start_minimized: bool) -> String {
    let mut command = format!("\"{}\"", executable.display());
    if start_minimized {
        command.push(' ');
        command.push_str(START_MINIMIZED_ARGUMENT);
    }
    command
}

fn wide(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::startup_command;

    #[test]
    fn startup_command_only_adds_minimized_argument_when_enabled() {
        let executable = Path::new(r"C:\Program Files\ShiTu\ShiTu.exe");
        assert_eq!(
            startup_command(executable, false),
            r#""C:\Program Files\ShiTu\ShiTu.exe""#
        );
        assert_eq!(
            startup_command(executable, true),
            r#""C:\Program Files\ShiTu\ShiTu.exe" --minimized"#
        );
    }
}
