use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::platform::clock::LocalTime;

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
