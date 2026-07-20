use anyhow::{Result, anyhow};
use shi_foundation::i18n;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

impl Bounds {
    pub fn validate(self) -> Result<Self> {
        if self.width < 16 || self.height < 16 {
            return Err(anyhow!(i18n::text(
                "录制区域至少需要 16 × 16 像素",
                "The recording region must be at least 16 × 16 pixels"
            )));
        }
        Ok(self)
    }

    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left
            && y >= self.top
            && x < self.left.saturating_add(self.width)
            && y < self.top.saturating_add(self.height)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordingTarget {
    Screen(Bounds),
    Window { hwnd: isize, initial_bounds: Bounds },
    Region(Bounds),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MonitorCandidate {
    pub bounds: Bounds,
    pub primary: bool,
}

pub struct MonitorCandidates {
    values: Vec<MonitorCandidate>,
}

impl MonitorCandidates {
    #[cfg(windows)]
    pub fn snapshot() -> Result<Self> {
        use std::mem::size_of;
        use windows::Win32::{
            Foundation::LPARAM,
            Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO},
            UI::WindowsAndMessaging::MONITORINFOF_PRIMARY,
        };

        unsafe extern "system" fn enumerate(
            monitor: HMONITOR,
            _device_context: HDC,
            _bounds: *mut windows::Win32::Foundation::RECT,
            parameter: LPARAM,
        ) -> windows::core::BOOL {
            let values = unsafe { &mut *(parameter.0 as *mut Vec<MonitorCandidate>) };
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
                values.push(MonitorCandidate {
                    bounds: Bounds {
                        left: info.rcMonitor.left,
                        top: info.rcMonitor.top,
                        width: info.rcMonitor.right.saturating_sub(info.rcMonitor.left),
                        height: info.rcMonitor.bottom.saturating_sub(info.rcMonitor.top),
                    },
                    primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
                });
            }
            windows::core::BOOL(1)
        }

        let mut values: Vec<MonitorCandidate> = Vec::new();
        unsafe {
            EnumDisplayMonitors(
                None,
                None,
                Some(enumerate),
                LPARAM((&mut values as *mut Vec<MonitorCandidate>) as isize),
            )
            .ok()?;
        }
        values.retain(|monitor| monitor.bounds.validate().is_ok());
        values.sort_by_key(|monitor| (!monitor.primary, monitor.bounds.top, monitor.bounds.left));
        if values.is_empty() {
            return Err(anyhow!(i18n::text(
                "未检测到可录制的显示器",
                "No recordable display was detected"
            )));
        }
        Ok(Self { values })
    }

    #[cfg(not(windows))]
    pub fn snapshot() -> Result<Self> {
        Err(anyhow!(i18n::text(
            "显示器选择仅支持 Windows",
            "Display selection is only supported on Windows"
        )))
    }

    pub fn get(&self, index: usize) -> Option<MonitorCandidate> {
        self.values.get(index).copied()
    }

    pub fn primary_index(&self) -> usize {
        self.values
            .iter()
            .position(|monitor| monitor.primary)
            .unwrap_or(0)
    }

    pub fn index_of(&self, bounds: Bounds) -> Option<usize> {
        self.values
            .iter()
            .position(|monitor| monitor.bounds == bounds)
    }

    pub fn labels(&self) -> Vec<String> {
        self.values
            .iter()
            .enumerate()
            .map(|(index, monitor)| {
                format!(
                    "{} {} · {} × {}{}",
                    i18n::text("显示器", "Display"),
                    index + 1,
                    monitor.bounds.width,
                    monitor.bounds.height,
                    if monitor.primary {
                        i18n::text("（主显示器）", " (primary)")
                    } else {
                        ""
                    }
                )
            })
            .collect()
    }
}

impl RecordingTarget {
    pub fn initial_bounds(self) -> Bounds {
        match self {
            Self::Screen(bounds) | Self::Region(bounds) => bounds,
            Self::Window { initial_bounds, .. } => initial_bounds,
        }
    }

    pub fn current_bounds(self) -> Result<Bounds> {
        match self {
            Self::Screen(bounds) | Self::Region(bounds) => bounds.validate(),
            Self::Window { hwnd, .. } => window_bounds(hwnd)
                .ok_or_else(|| {
                    anyhow!(i18n::text(
                        "所选窗口已关闭、隐藏或最小化",
                        "The selected window was closed, hidden, or minimized"
                    ))
                })?
                .validate(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowCandidate {
    pub hwnd: isize,
    pub bounds: Bounds,
    pub title: String,
}

pub struct WindowCandidates {
    values: Vec<WindowCandidate>,
}

impl WindowCandidates {
    #[cfg(windows)]
    pub fn snapshot(desktop: Bounds) -> Result<Self> {
        use windows::Win32::{Foundation::LPARAM, UI::WindowsAndMessaging::EnumWindows};

        struct Enumeration {
            desktop: Bounds,
            values: Vec<WindowCandidate>,
        }

        unsafe extern "system" fn enumerate(
            hwnd: windows::Win32::Foundation::HWND,
            parameter: LPARAM,
        ) -> windows::core::BOOL {
            let enumeration = unsafe { &mut *(parameter.0 as *mut Enumeration) };
            let raw = hwnd.0 as isize;
            if let Some(bounds) = clipped_window_bounds(raw, enumeration.desktop) {
                enumeration.values.push(WindowCandidate {
                    hwnd: raw,
                    bounds,
                    title: window_title(raw),
                });
            }
            windows::core::BOOL(1)
        }

        let mut enumeration = Enumeration {
            desktop,
            values: Vec::new(),
        };
        unsafe {
            EnumWindows(
                Some(enumerate),
                LPARAM((&mut enumeration as *mut Enumeration) as isize),
            )?;
        }
        Ok(Self {
            values: enumeration.values,
        })
    }

    #[cfg(not(windows))]
    pub fn snapshot(_desktop: Bounds) -> Result<Self> {
        Err(anyhow!(i18n::text(
            "窗口选择仅支持 Windows",
            "Window selection is only supported on Windows"
        )))
    }

    pub fn target_at(&self, x: i32, y: i32) -> Option<WindowCandidate> {
        self.values
            .iter()
            .find(|candidate| candidate.bounds.contains(x, y))
            .cloned()
    }

    pub fn exclude(&mut self, hwnd: isize) {
        self.values.retain(|candidate| candidate.hwnd != hwnd);
    }
}

#[cfg(windows)]
fn window_title(hwnd: isize) -> String {
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW},
    };

    let handle = HWND(hwnd as *mut _);
    let length = unsafe { GetWindowTextLengthW(handle) };
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0_u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(handle, &mut buffer) };
    if copied <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..copied as usize])
        .trim()
        .to_owned()
}

#[cfg(windows)]
pub fn virtual_desktop_bounds() -> Result<Bounds> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    Bounds {
        left: unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) },
        top: unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) },
        width: unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) },
        height: unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) },
    }
    .validate()
}

#[cfg(not(windows))]
pub fn virtual_desktop_bounds() -> Result<Bounds> {
    Err(anyhow!(i18n::text(
        "屏幕录制仅支持 Windows",
        "Screen recording is only supported on Windows"
    )))
}

#[cfg(windows)]
pub fn primary_screen_bounds() -> Result<Bounds> {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    Bounds {
        left: 0,
        top: 0,
        width: unsafe { GetSystemMetrics(SM_CXSCREEN) },
        height: unsafe { GetSystemMetrics(SM_CYSCREEN) },
    }
    .validate()
}

#[cfg(not(windows))]
pub fn primary_screen_bounds() -> Result<Bounds> {
    Err(anyhow!(i18n::text(
        "屏幕录制仅支持 Windows",
        "Screen recording is only supported on Windows"
    )))
}

#[cfg(windows)]
fn window_bounds(hwnd: isize) -> Option<Bounds> {
    use std::mem::size_of;
    use windows::Win32::{
        Foundation::{HWND, RECT},
        Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
        UI::WindowsAndMessaging::{GetWindowRect, IsIconic, IsWindowVisible},
    };
    let hwnd = HWND(hwnd as *mut _);
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() || unsafe { IsIconic(hwnd) }.as_bool() {
        return None;
    }
    let mut rect = RECT::default();
    if unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut rect as *mut RECT).cast(),
            size_of::<RECT>() as u32,
        )
    }
    .is_err()
        && unsafe { GetWindowRect(hwnd, &mut rect) }.is_err()
    {
        return None;
    }
    Some(Bounds {
        left: rect.left,
        top: rect.top,
        width: rect.right.saturating_sub(rect.left),
        height: rect.bottom.saturating_sub(rect.top),
    })
}

#[cfg(not(windows))]
fn window_bounds(_hwnd: isize) -> Option<Bounds> {
    None
}

#[cfg(windows)]
fn clipped_window_bounds(hwnd: isize, desktop: Bounds) -> Option<Bounds> {
    use std::mem::size_of;
    use windows::Win32::{
        Foundation::HWND,
        Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
        UI::WindowsAndMessaging::{GWL_EXSTYLE, GetWindowLongW, WS_EX_TRANSPARENT},
    };
    let handle = HWND(hwnd as *mut _);
    let extended_style = unsafe { GetWindowLongW(handle, GWL_EXSTYLE) } as u32;
    if extended_style & WS_EX_TRANSPARENT.0 != 0 {
        return None;
    }
    let mut cloaked = 0_u32;
    if unsafe {
        DwmGetWindowAttribute(
            handle,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut u32).cast(),
            size_of::<u32>() as u32,
        )
    }
    .is_ok()
        && cloaked != 0
    {
        return None;
    }
    let bounds = window_bounds(hwnd)?;
    let left = bounds.left.max(desktop.left);
    let top = bounds.top.max(desktop.top);
    let right = (bounds.left + bounds.width).min(desktop.left + desktop.width);
    let bottom = (bounds.top + bounds.height).min(desktop.top + desktop.height);
    let clipped = Bounds {
        left,
        top,
        width: right.saturating_sub(left),
        height: bottom.saturating_sub(top),
    };
    (clipped.width >= 24 && clipped.height >= 24).then_some(clipped)
}

#[cfg(test)]
mod tests {
    use super::{Bounds, MonitorCandidate, MonitorCandidates, WindowCandidate, WindowCandidates};

    #[test]
    fn bounds_validate_and_contain_points() {
        let bounds = Bounds {
            left: -20,
            top: 10,
            width: 100,
            height: 60,
        };
        assert!(bounds.validate().is_ok());
        assert!(bounds.contains(-20, 10));
        assert!(bounds.contains(79, 69));
        assert!(!bounds.contains(80, 70));
    }

    #[test]
    fn monitor_candidates_identify_primary_and_format_labels() {
        let primary = Bounds {
            left: 0,
            top: 0,
            width: 1920,
            height: 1080,
        };
        let secondary = Bounds {
            left: -2560,
            top: 0,
            width: 2560,
            height: 1440,
        };
        let monitors = MonitorCandidates {
            values: vec![
                MonitorCandidate {
                    bounds: primary,
                    primary: true,
                },
                MonitorCandidate {
                    bounds: secondary,
                    primary: false,
                },
            ],
        };

        assert_eq!(monitors.primary_index(), 0);
        assert_eq!(monitors.index_of(secondary), Some(1));
        assert_eq!(
            monitors.labels(),
            vec![
                "显示器 1 · 1920 × 1080（主显示器）",
                "显示器 2 · 2560 × 1440",
            ]
        );
    }

    #[test]
    fn window_candidates_use_z_order_and_keep_titles() {
        let front = WindowCandidate {
            hwnd: 1,
            bounds: Bounds {
                left: 100,
                top: 100,
                width: 200,
                height: 200,
            },
            title: "Front window".to_owned(),
        };
        let back = WindowCandidate {
            hwnd: 2,
            bounds: Bounds {
                left: 0,
                top: 0,
                width: 500,
                height: 500,
            },
            title: "Back window".to_owned(),
        };
        let candidates = WindowCandidates {
            values: vec![front.clone(), back],
        };

        assert_eq!(candidates.target_at(150, 150), Some(front));
        assert!(candidates.target_at(600, 600).is_none());
    }
}
