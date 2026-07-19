use anyhow::{Result, anyhow};

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
            return Err(anyhow!("录制区域至少需要 16 × 16 像素"));
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
                .ok_or_else(|| anyhow!("所选窗口已关闭、隐藏或最小化"))?
                .validate(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WindowCandidate {
    pub hwnd: isize,
    pub bounds: Bounds,
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
                enumeration
                    .values
                    .push(WindowCandidate { hwnd: raw, bounds });
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
        Err(anyhow!("窗口选择仅支持 Windows"))
    }

    pub fn target_at(&self, x: i32, y: i32) -> Option<WindowCandidate> {
        self.values
            .iter()
            .copied()
            .find(|candidate| candidate.bounds.contains(x, y))
    }

    pub fn exclude(&mut self, hwnd: isize) {
        self.values.retain(|candidate| candidate.hwnd != hwnd);
    }
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
    Err(anyhow!("屏幕录制仅支持 Windows"))
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
    Err(anyhow!("屏幕录制仅支持 Windows"))
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
    use super::Bounds;

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
}
