use std::thread;

use anyhow::{Context, Result};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey},
};

use crate::MainWindow;

pub(super) struct RecordingHotkeys {
    _manager: GlobalHotKeyManager,
    start_id: u32,
    pause_id: u32,
    stop_id: u32,
}

impl RecordingHotkeys {
    pub(super) fn register() -> Result<Self> {
        let manager = GlobalHotKeyManager::new().context("无法创建全局快捷键管理器")?;
        let start = HotKey::new(None, Code::F10);
        let pause = HotKey::new(None, Code::F11);
        let stop = HotKey::new(None, Code::F12);
        let hotkeys = [start, pause, stop];

        for (index, hotkey) in hotkeys.iter().copied().enumerate() {
            if let Err(error) = manager.register(hotkey) {
                for registered in hotkeys[..index].iter().copied() {
                    let _ = manager.unregister(registered);
                }
                return Err(error).context(format!(
                    "无法注册 {} 快捷键",
                    match hotkey.key {
                        Code::F10 => "F10",
                        Code::F11 => "F11",
                        Code::F12 => "F12",
                        _ => "录制",
                    }
                ));
            }
        }

        Ok(Self {
            _manager: manager,
            start_id: start.id(),
            pause_id: pause.id(),
            stop_id: stop.id(),
        })
    }

    pub(super) fn bind_events(&self, main: slint::Weak<MainWindow>) {
        let start_id = self.start_id;
        let pause_id = self.pause_id;
        let stop_id = self.stop_id;
        thread::spawn(move || {
            while let Ok(event) = GlobalHotKeyEvent::receiver().recv() {
                if event.state != HotKeyState::Pressed {
                    continue;
                }
                let event_id = event.id;
                if main
                    .upgrade_in_event_loop(move |main| {
                        if event_id == start_id {
                            main.invoke_start_recording();
                        } else if event_id == pause_id {
                            main.invoke_pause_recording();
                        } else if event_id == stop_id {
                            main.invoke_stop_recording();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
    }
}
