//! Native tooltip control for the small screenshot toolbar. GPUI Component
//! 0.7.1 TooltipOverlay is confined to its window. The Windows control owns
//! hover timers, positioning, mouse leave/click dismissal and screen clamping.
//! https://learn.microsoft.com/windows/win32/controls/tooltip-controls
use std::{collections::HashMap, mem::size_of};

use anyhow::{Context, Result, ensure};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT, WPARAM},
        UI::{
            Controls::{
                ICC_WIN95_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, TOOLTIPS_CLASSW,
                TTDT_AUTOPOP, TTDT_INITIAL, TTDT_RESHOW, TTF_SUBCLASS, TTM_ADDTOOLW, TTM_DELTOOLW,
                TTM_NEWTOOLRECTW, TTM_POP, TTM_SETDELAYTIME, TTS_ALWAYSTIP, TTS_NOPREFIX,
                TTTOOLINFOW,
            },
            WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, HWND_TOPMOST, IsWindow, SWP_NOACTIVATE, SWP_NOMOVE,
                SWP_NOSIZE, SendMessageW, SetWindowPos, WINDOW_STYLE, WS_EX_NOACTIVATE,
                WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
    core::{PCWSTR, PWSTR},
};

pub struct TipRegion {
    pub id: usize,
    pub text: &'static str,
    pub rect: RECT,
}

struct Tool {
    text: &'static str,
    // The control may retain this pointer until DELTOOL or destruction.
    wide: Vec<u16>,
    rect: RECT,
}

pub struct NativeTooltips {
    hwnd: HWND,
    owner: HWND,
    tools: HashMap<usize, Tool>,
}

impl NativeTooltips {
    pub fn new(owner: HWND) -> Result<Self> {
        unsafe {
            ensure!(
                InitCommonControlsEx(&INITCOMMONCONTROLSEX {
                    dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
                    dwICC: ICC_WIN95_CLASSES,
                })
                .as_bool(),
                "Initialize tooltip controls"
            );
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                TOOLTIPS_CLASSW,
                PCWSTR::null(),
                WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP | TTS_NOPREFIX),
                0,
                0,
                0,
                0,
                Some(owner),
                None,
                None,
                None,
            )
            .context("Create toolbar tooltip")?;
            let tooltip = Self {
                hwnd,
                owner,
                tools: HashMap::new(),
            };
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .context("Position toolbar tooltip")?;
            // No instant reshow when moving between neighboring buttons.
            for (kind, delay) in [
                (TTDT_INITIAL, 600),
                (TTDT_RESHOW, 600),
                (TTDT_AUTOPOP, 5000),
            ] {
                SendMessageW(
                    hwnd,
                    TTM_SETDELAYTIME,
                    Some(WPARAM(kind as usize)),
                    Some(LPARAM(delay)),
                );
            }
            Ok(tooltip)
        }
    }

    fn info(&self, id: usize) -> TTTOOLINFOW {
        TTTOOLINFOW {
            cbSize: size_of::<TTTOOLINFOW>() as u32,
            uFlags: TTF_SUBCLASS,
            hwnd: self.owner,
            uId: id,
            ..Default::default()
        }
    }

    pub fn dismiss(&self) {
        unsafe {
            SendMessageW(self.hwnd, TTM_POP, None, None);
        }
    }

    /// Reconcile actual painted button rectangles; do not restart timers on
    /// unchanged frames. Removed/hidden buttons cannot leave stale hit regions.
    pub fn sync(&mut self, regions: &[TipRegion]) -> Result<()> {
        // TTM_NEWTOOLRECTW updates hit testing, but does not dismiss a visible
        // tip. Pop it before a layout/label change so it cannot linger over a
        // different control. Unchanged frames retain the native hover timers.
        if self.tools.len() != regions.len()
            || regions.iter().any(|region| {
                self.tools
                    .get(&region.id)
                    .is_none_or(|tool| tool.text != region.text || tool.rect != region.rect)
            })
        {
            self.dismiss();
        }
        let removed: Vec<_> = self
            .tools
            .iter()
            .filter_map(|(&id, old)| {
                (!regions.iter().any(|r| r.id == id && r.text == old.text)).then_some(id)
            })
            .collect();
        for id in removed {
            let info = self.info(id);
            unsafe {
                SendMessageW(
                    self.hwnd,
                    TTM_DELTOOLW,
                    None,
                    Some(LPARAM((&info as *const TTTOOLINFOW) as isize)),
                );
            }
            self.tools.remove(&id);
        }
        for region in regions {
            let mut info = self.info(region.id);
            info.rect = region.rect;
            if let Some(tool) = self.tools.get_mut(&region.id) {
                if tool.rect != region.rect {
                    tool.rect = region.rect;
                    unsafe {
                        SendMessageW(
                            self.hwnd,
                            TTM_NEWTOOLRECTW,
                            None,
                            Some(LPARAM((&info as *const TTTOOLINFOW) as isize)),
                        );
                    }
                }
            } else {
                let mut tool = Tool {
                    text: region.text,
                    wide: region.text.encode_utf16().chain(Some(0)).collect(),
                    rect: region.rect,
                };
                info.lpszText = PWSTR(tool.wide.as_mut_ptr());
                ensure!(
                    unsafe {
                        SendMessageW(
                            self.hwnd,
                            TTM_ADDTOOLW,
                            None,
                            Some(LPARAM((&info as *const TTTOOLINFOW) as isize)),
                        )
                    }
                    .0 != 0,
                    "Register toolbar tooltip {}",
                    region.id
                );
                self.tools.insert(region.id, tool);
            }
        }
        Ok(())
    }
}

impl Drop for NativeTooltips {
    fn drop(&mut self) {
        // The owner may already have destroyed this owned native control.
        unsafe {
            if IsWindow(Some(self.hwnd)).as_bool() {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::{
        Foundation::POINT,
        UI::Controls::{
            TTHITTESTINFOW, TTM_GETDELAYTIME, TTM_GETTOOLCOUNT, TTM_GETTOOLINFOW, TTM_HITTESTW,
        },
    };

    #[test]
    fn native_control_delays_reshow_and_reconciles_moved_and_removed_tools() -> Result<()> {
        // Hidden test window only; no desktop input, activation or screenshots.
        struct Owner(HWND);
        impl Drop for Owner {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }
        let owner = Owner(unsafe {
            CreateWindowExW(
                Default::default(),
                windows::core::w!("STATIC"),
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                400,
                100,
                None,
                None,
                None,
                None,
            )?
        });
        let mut tips = NativeTooltips::new(owner.0)?;
        for delay in [TTDT_INITIAL, TTDT_RESHOW] {
            assert_eq!(
                unsafe {
                    SendMessageW(
                        tips.hwnd,
                        TTM_GETDELAYTIME,
                        Some(WPARAM(delay as usize)),
                        None,
                    )
                }
                .0,
                600
            );
        }
        let first = RECT {
            left: 0,
            top: 0,
            right: 28,
            bottom: 28,
        };
        let moved = RECT {
            left: 40,
            top: 32,
            right: 68,
            bottom: 60,
        };
        tips.sync(&[
            TipRegion {
                id: 1,
                text: "Pen",
                rect: first,
            },
            TipRegion {
                id: 2,
                text: "Color",
                rect: moved,
            },
        ])?;
        assert_eq!(
            unsafe { SendMessageW(tips.hwnd, TTM_GETTOOLCOUNT, None, None) }.0,
            2
        );
        tips.sync(&[TipRegion {
            id: 1,
            text: "Pen",
            rect: moved,
        }])?;
        let mut info = tips.info(1);
        assert_ne!(
            unsafe {
                SendMessageW(
                    tips.hwnd,
                    TTM_GETTOOLINFOW,
                    None,
                    Some(LPARAM((&mut info as *mut TTTOOLINFOW) as isize)),
                )
            }
            .0,
            0
        );
        assert_eq!(info.rect, moved);
        // After a mode/layout change, the old location must not trigger a tip;
        // only the new button rectangle may match the native control.
        for (point, expected) in [
            (POINT { x: 10, y: 10 }, false),
            (POINT { x: 50, y: 40 }, true),
        ] {
            let mut hit = TTHITTESTINFOW {
                hwnd: owner.0,
                pt: point,
                ti: tips.info(1),
            };
            let found = unsafe {
                SendMessageW(
                    tips.hwnd,
                    TTM_HITTESTW,
                    None,
                    Some(LPARAM((&mut hit as *mut TTHITTESTINFOW) as isize)),
                )
            }
            .0 != 0;
            assert_eq!(found, expected);
            if found {
                assert_eq!(hit.ti.uId, 1);
            }
        }
        assert_eq!(
            unsafe { SendMessageW(tips.hwnd, TTM_GETTOOLCOUNT, None, None) }.0,
            1
        );
        // The same slot can have another label when switching toolbar modes.
        tips.sync(&[TipRegion {
            id: 1,
            text: "Width",
            rect: moved,
        }])?;
        assert_eq!(
            unsafe { SendMessageW(tips.hwnd, TTM_GETTOOLCOUNT, None, None) }.0,
            1
        );
        tips.sync(&[])?;
        assert_eq!(
            unsafe { SendMessageW(tips.hwnd, TTM_GETTOOLCOUNT, None, None) }.0,
            0
        );
        let handle = tips.hwnd;
        drop(tips);
        assert!(!unsafe { IsWindow(Some(handle)) }.as_bool());
        Ok(())
    }
}
