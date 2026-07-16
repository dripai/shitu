use std::mem::size_of;

use anyhow::Result;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, POINT, RECT},
        Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
        UI::WindowsAndMessaging::{
            EnumWindows, GWL_EXSTYLE, GetCursorPos, GetWindowLongW, GetWindowRect, IsIconic,
            IsWindowVisible, WS_EX_TRANSPARENT,
        },
    },
    core::BOOL,
};

use crate::image::DesktopBounds;

pub struct WindowTargets {
    bounds: Vec<DesktopBounds>,
}

impl WindowTargets {
    pub fn snapshot(desktop: DesktopBounds) -> Result<Self> {
        let mut enumeration = Enumeration {
            desktop,
            bounds: Vec::new(),
        };
        unsafe {
            EnumWindows(
                Some(enum_window),
                LPARAM((&mut enumeration as *mut Enumeration) as isize),
            )?;
        }
        Ok(Self {
            bounds: enumeration.bounds,
        })
    }

    pub fn target_at(&self, x: i32, y: i32) -> Option<DesktopBounds> {
        self.bounds
            .iter()
            .copied()
            .find(|bounds| contains(*bounds, x, y))
    }

    pub fn target_at_cursor(&self) -> Option<DesktopBounds> {
        let mut point = POINT::default();
        unsafe { GetCursorPos(&mut point) }
            .ok()
            .and_then(|_| self.target_at(point.x, point.y))
    }
}

struct Enumeration {
    desktop: DesktopBounds,
    bounds: Vec<DesktopBounds>,
}

unsafe extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let enumeration = unsafe { &mut *(lparam.0 as *mut Enumeration) };
    if let Some(bounds) = window_bounds(hwnd, enumeration.desktop) {
        enumeration.bounds.push(bounds);
    }
    BOOL(1)
}

fn window_bounds(hwnd: HWND, desktop: DesktopBounds) -> Option<DesktopBounds> {
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() || unsafe { IsIconic(hwnd) }.as_bool() {
        return None;
    }

    let extended_style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
    if extended_style & WS_EX_TRANSPARENT.0 != 0 {
        return None;
    }

    let mut cloaked = 0_u32;
    if unsafe {
        DwmGetWindowAttribute(
            hwnd,
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

    clipped_bounds(rect, desktop)
}

fn clipped_bounds(rect: RECT, desktop: DesktopBounds) -> Option<DesktopBounds> {
    let left = rect.left.max(desktop.left);
    let top = rect.top.max(desktop.top);
    let right = rect.right.min(desktop.left + desktop.width);
    let bottom = rect.bottom.min(desktop.top + desktop.height);
    let width = right.saturating_sub(left);
    let height = bottom.saturating_sub(top);
    (width >= 24 && height >= 24).then_some(DesktopBounds {
        left,
        top,
        width,
        height,
    })
}

fn contains(bounds: DesktopBounds, x: i32, y: i32) -> bool {
    x >= bounds.left
        && y >= bounds.top
        && x < bounds.left + bounds.width
        && y < bounds.top + bounds.height
}

#[cfg(test)]
mod tests {
    use super::{WindowTargets, clipped_bounds};
    use crate::image::DesktopBounds;
    use windows::Win32::Foundation::RECT;

    #[test]
    fn targets_use_z_order_and_clip_to_desktop() {
        let targets = WindowTargets {
            bounds: vec![
                DesktopBounds {
                    left: 100,
                    top: 100,
                    width: 200,
                    height: 200,
                },
                DesktopBounds {
                    left: 0,
                    top: 0,
                    width: 500,
                    height: 500,
                },
            ],
        };
        assert_eq!(targets.target_at(150, 150), Some(targets.bounds[0]));

        assert_eq!(
            clipped_bounds(
                RECT {
                    left: -50,
                    top: -20,
                    right: 120,
                    bottom: 80,
                },
                DesktopBounds {
                    left: 0,
                    top: 0,
                    width: 100,
                    height: 100,
                },
            ),
            Some(DesktopBounds {
                left: 0,
                top: 0,
                width: 100,
                height: 80,
            })
        );
    }
}
