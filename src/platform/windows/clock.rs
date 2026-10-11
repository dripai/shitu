use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::platform::clock::LocalTime;

pub fn format_timestamp(seconds: u64) -> anyhow::Result<String> {
    use windows::Win32::{
        Foundation::{FILETIME, SYSTEMTIME},
        System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime},
    };
    let ticks = seconds
        .checked_add(11_644_473_600)
        .and_then(|v| v.checked_mul(10_000_000))
        .ok_or_else(|| anyhow::anyhow!("Invalid file timestamp"))?;
    let file = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    unsafe {
        FileTimeToSystemTime(&file, &mut utc)?;
        SystemTimeToTzSpecificLocalTime(None, &utc, &mut local)?;
    }
    Ok(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute
    ))
}

pub fn local_time() -> LocalTime {
    let value = unsafe { GetLocalTime() };
    LocalTime {
        year: value.wYear,
        month: value.wMonth,
        day: value.wDay,
        hour: value.wHour,
        minute: value.wMinute,
        second: value.wSecond,
    }
}
