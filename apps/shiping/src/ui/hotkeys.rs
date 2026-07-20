use std::{
    fmt,
    sync::{Arc, RwLock},
    thread,
};

use anyhow::{Context, Result};
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};

use crate::MainWindow;

const ACTION_COUNT: usize = 3;
const START_ACTION: usize = 0;
const PAUSE_ACTION: usize = 1;
const STOP_ACTION: usize = 2;

type HotkeySet = [Option<HotKey>; ACTION_COUNT];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ShortcutIssue {
    pub action: Option<usize>,
    pub message: String,
}

impl ShortcutIssue {
    fn action(action: usize, message: impl Into<String>) -> Self {
        Self {
            action: Some(action),
            message: message.into(),
        }
    }

    fn general(message: impl Into<String>) -> Self {
        Self {
            action: None,
            message: message.into(),
        }
    }
}

impl fmt::Display for ShortcutIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ShortcutIssue {}

trait HotkeyRegistrar {
    fn register(&self, hotkey: HotKey) -> std::result::Result<(), RegistrationFailure>;
    fn unregister(&self, hotkey: HotKey) -> std::result::Result<(), String>;
}

#[derive(Debug)]
enum RegistrationFailure {
    Conflict,
    Other(String),
}

impl HotkeyRegistrar for GlobalHotKeyManager {
    fn register(&self, hotkey: HotKey) -> std::result::Result<(), RegistrationFailure> {
        GlobalHotKeyManager::register(self, hotkey).map_err(|error| match error {
            global_hotkey::Error::AlreadyRegistered(_) => RegistrationFailure::Conflict,
            error => RegistrationFailure::Other(error.to_string()),
        })
    }

    fn unregister(&self, hotkey: HotKey) -> std::result::Result<(), String> {
        GlobalHotKeyManager::unregister(self, hotkey).map_err(|error| error.to_string())
    }
}

struct ReplaceFailure {
    issue: ShortcutIssue,
    restored_old: bool,
}

pub(super) struct RecordingHotkeys {
    manager: GlobalHotKeyManager,
    registered: HotkeySet,
    action_ids: Arc<RwLock<[Option<u32>; ACTION_COUNT]>>,
}

impl RecordingHotkeys {
    pub(super) fn new() -> Result<Self> {
        Ok(Self {
            manager: GlobalHotKeyManager::new().context("无法创建全局快捷键管理器")?,
            registered: [None; ACTION_COUNT],
            action_ids: Arc::new(RwLock::new([None; ACTION_COUNT])),
        })
    }

    pub(super) fn reconfigure(
        &mut self,
        values: [Option<String>; ACTION_COUNT],
    ) -> std::result::Result<[Option<String>; ACTION_COUNT], ShortcutIssue> {
        let (hotkeys, canonical) = parse_shortcuts(values)?;
        if hotkeys == self.registered {
            return Ok(canonical);
        }

        let old = self.registered;
        match replace_registered(&self.manager, old, hotkeys) {
            Ok(()) => {
                self.registered = hotkeys;
                self.publish_ids(hotkeys);
                Ok(canonical)
            }
            Err(failure) => {
                if failure.restored_old {
                    self.registered = old;
                    self.publish_ids(old);
                } else {
                    self.registered = [None; ACTION_COUNT];
                    self.publish_ids([None; ACTION_COUNT]);
                }
                Err(failure.issue)
            }
        }
    }

    pub(super) fn bind_events(&self, main: slint::Weak<MainWindow>) {
        let action_ids = Arc::clone(&self.action_ids);
        thread::spawn(move || {
            while let Ok(event) = GlobalHotKeyEvent::receiver().recv() {
                if event.state != HotKeyState::Pressed {
                    continue;
                }
                let action = action_ids
                    .read()
                    .ok()
                    .and_then(|ids| ids.iter().position(|id| *id == Some(event.id)));
                let Some(action) = action else { continue };
                if main
                    .upgrade_in_event_loop(move |main| match action {
                        START_ACTION => main.invoke_start_recording(),
                        PAUSE_ACTION => main.invoke_pause_recording(),
                        STOP_ACTION => main.invoke_stop_recording(),
                        _ => {}
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
    }

    fn publish_ids(&self, hotkeys: HotkeySet) {
        if let Ok(mut ids) = self.action_ids.write() {
            *ids = hotkeys.map(|hotkey| hotkey.map(|value| value.id()));
        }
    }
}

pub(super) fn shortcut_from_key_event(
    text: &str,
    control: bool,
    alt: bool,
    shift: bool,
    meta: bool,
) -> std::result::Result<String, ShortcutIssue> {
    let key = key_name_from_event(text)
        .ok_or_else(|| ShortcutIssue::general("这个按键暂不支持作为全局快捷键"))?;
    let mut parts = Vec::with_capacity(5);
    if control {
        parts.push("control".to_owned());
    }
    if alt {
        parts.push("alt".to_owned());
    }
    if shift {
        parts.push("shift".to_owned());
    }
    if meta {
        parts.push("super".to_owned());
    }
    parts.push(key);
    let hotkey = parts
        .join("+")
        .parse::<HotKey>()
        .map_err(|error| ShortcutIssue::general(format!("无法识别这个快捷键：{error}")))?;
    validate_hotkey(hotkey).map_err(ShortcutIssue::general)?;
    Ok(hotkey.to_string())
}

pub(super) fn display_shortcut(value: Option<&str>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let Ok(hotkey) = value.parse::<HotKey>() else {
        return value.to_owned();
    };
    let mut parts = Vec::with_capacity(5);
    if hotkey.mods.contains(Modifiers::CONTROL) {
        parts.push("Ctrl".to_owned());
    }
    if hotkey.mods.contains(Modifiers::ALT) {
        parts.push("Alt".to_owned());
    }
    if hotkey.mods.contains(Modifiers::SHIFT) {
        parts.push("Shift".to_owned());
    }
    if hotkey.mods.contains(Modifiers::SUPER) {
        parts.push("Win".to_owned());
    }
    let key = hotkey.key.to_string();
    parts.push(
        key.strip_prefix("Key")
            .or_else(|| key.strip_prefix("Digit"))
            .unwrap_or(&key)
            .to_owned(),
    );
    parts.join(" + ")
}

fn parse_shortcuts(
    values: [Option<String>; ACTION_COUNT],
) -> std::result::Result<(HotkeySet, [Option<String>; ACTION_COUNT]), ShortcutIssue> {
    let mut hotkeys = [None; ACTION_COUNT];
    let mut canonical: [Option<String>; ACTION_COUNT] = [None, None, None];
    for (index, value) in values.into_iter().enumerate() {
        let Some(value) = value else { continue };
        let value = value.trim();
        if value.is_empty() {
            return Err(ShortcutIssue::action(index, "启用后必须设置快捷键"));
        }
        let hotkey = value
            .parse::<HotKey>()
            .map_err(|error| ShortcutIssue::action(index, format!("快捷键格式无效：{error}")))?;
        validate_hotkey(hotkey).map_err(|message| ShortcutIssue::action(index, message))?;
        if let Some(previous) = hotkeys[..index]
            .iter()
            .position(|candidate| *candidate == Some(hotkey))
        {
            return Err(ShortcutIssue::action(
                index,
                format!("与{}快捷键重复", action_label(previous)),
            ));
        }
        hotkeys[index] = Some(hotkey);
        canonical[index] = Some(hotkey.to_string());
    }
    Ok((hotkeys, canonical))
}

fn validate_hotkey(hotkey: HotKey) -> std::result::Result<(), String> {
    if is_unsupported_function_key(hotkey.key) {
        return Err("功能键仅支持 F1–F12".to_owned());
    }
    if hotkey.mods.is_empty() && !is_supported_function_key(hotkey.key) {
        return Err("普通按键必须同时按下 Ctrl、Alt、Shift 或 Win".to_owned());
    }
    Ok(())
}

fn is_supported_function_key(key: Code) -> bool {
    matches!(
        key,
        Code::F1
            | Code::F2
            | Code::F3
            | Code::F4
            | Code::F5
            | Code::F6
            | Code::F7
            | Code::F8
            | Code::F9
            | Code::F10
            | Code::F11
            | Code::F12
    )
}

fn is_unsupported_function_key(key: Code) -> bool {
    matches!(
        key,
        Code::F13
            | Code::F14
            | Code::F15
            | Code::F16
            | Code::F17
            | Code::F18
            | Code::F19
            | Code::F20
            | Code::F21
            | Code::F22
            | Code::F23
            | Code::F24
    )
}

fn key_name_from_event(text: &str) -> Option<String> {
    let mut chars = text.chars();
    let key = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let name = match key {
        'a'..='z' | 'A'..='Z' => key.to_ascii_uppercase().to_string(),
        '0'..='9' => key.to_string(),
        '`' | '\\' | '[' | ']' | ',' | '=' | '-' | '.' | '\'' | ';' | '/' => key.to_string(),
        '\u{0008}' => "Backspace".to_owned(),
        '\u{0009}' => "Tab".to_owned(),
        '\u{000a}' | '\r' => "Enter".to_owned(),
        '\u{001b}' => "Escape".to_owned(),
        '\u{0020}' => "Space".to_owned(),
        '\u{007f}' => "Delete".to_owned(),
        '\u{F700}' => "ArrowUp".to_owned(),
        '\u{F701}' => "ArrowDown".to_owned(),
        '\u{F702}' => "ArrowLeft".to_owned(),
        '\u{F703}' => "ArrowRight".to_owned(),
        '\u{F704}'..='\u{F71B}' => format!("F{}", key as u32 - 0xF704 + 1),
        '\u{F727}' => "Insert".to_owned(),
        '\u{F729}' => "Home".to_owned(),
        '\u{F72B}' => "End".to_owned(),
        '\u{F72C}' => "PageUp".to_owned(),
        '\u{F72D}' => "PageDown".to_owned(),
        '\u{F72F}' => "ScrollLock".to_owned(),
        '\u{F730}' => "Pause".to_owned(),
        _ => return None,
    };
    Some(name)
}

fn action_label(index: usize) -> &'static str {
    match index {
        START_ACTION => "开始录制",
        PAUSE_ACTION => "暂停/继续",
        STOP_ACTION => "停止录制",
        _ => "录制",
    }
}

fn replace_registered<R: HotkeyRegistrar>(
    registrar: &R,
    old: HotkeySet,
    new: HotkeySet,
) -> std::result::Result<(), ReplaceFailure> {
    unregister_set(registrar, old)?;
    if let Err(issue) = register_set(registrar, new) {
        return match register_set(registrar, old) {
            Ok(()) => Err(ReplaceFailure {
                issue,
                restored_old: true,
            }),
            Err(restore_issue) => Err(ReplaceFailure {
                issue: ShortcutIssue::general(format!(
                    "{}；旧快捷键也未能恢复：{}",
                    issue.message, restore_issue.message
                )),
                restored_old: false,
            }),
        };
    }
    Ok(())
}

fn register_set<R: HotkeyRegistrar>(
    registrar: &R,
    hotkeys: HotkeySet,
) -> std::result::Result<(), ShortcutIssue> {
    let mut registered = Vec::new();
    for (index, hotkey) in hotkeys.into_iter().enumerate() {
        let Some(hotkey) = hotkey else { continue };
        if let Err(error) = registrar.register(hotkey) {
            for registered_hotkey in registered {
                let _ = registrar.unregister(registered_hotkey);
            }
            let message = match error {
                RegistrationFailure::Conflict => {
                    format!("{}快捷键已被其他程序占用", action_label(index))
                }
                RegistrationFailure::Other(error) => {
                    format!("{}快捷键注册失败：{error}", action_label(index))
                }
            };
            return Err(ShortcutIssue::action(index, message));
        }
        registered.push(hotkey);
    }
    Ok(())
}

fn unregister_set<R: HotkeyRegistrar>(
    registrar: &R,
    hotkeys: HotkeySet,
) -> std::result::Result<(), ReplaceFailure> {
    let mut unregistered = [None; ACTION_COUNT];
    for (index, hotkey) in hotkeys.into_iter().enumerate() {
        let Some(hotkey) = hotkey else { continue };
        if let Err(error) = registrar.unregister(hotkey) {
            let issue = ShortcutIssue::action(
                index,
                format!("无法更新{}快捷键：{error}", action_label(index)),
            );
            return match register_set(registrar, unregistered) {
                Ok(()) => Err(ReplaceFailure {
                    issue,
                    restored_old: true,
                }),
                Err(restore_issue) => {
                    for old_hotkey in hotkeys.into_iter().flatten() {
                        let _ = registrar.unregister(old_hotkey);
                    }
                    Err(ReplaceFailure {
                        issue: ShortcutIssue::general(format!(
                            "{}；旧快捷键也未能恢复：{}",
                            issue.message, restore_issue.message
                        )),
                        restored_old: false,
                    })
                }
            };
        }
        unregistered[index] = Some(hotkey);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashSet};

    use global_hotkey::hotkey::HotKey;

    use super::{
        HotkeyRegistrar, display_shortcut, parse_shortcuts, replace_registered,
        shortcut_from_key_event, validate_hotkey,
    };

    #[derive(Default)]
    struct FakeRegistrar {
        registered: RefCell<HashSet<HotKey>>,
        rejected: RefCell<HashSet<HotKey>>,
    }

    impl HotkeyRegistrar for FakeRegistrar {
        fn register(&self, hotkey: HotKey) -> Result<(), super::RegistrationFailure> {
            if self.rejected.borrow().contains(&hotkey) {
                return Err(super::RegistrationFailure::Conflict);
            }
            self.registered.borrow_mut().insert(hotkey);
            Ok(())
        }

        fn unregister(&self, hotkey: HotKey) -> Result<(), String> {
            if self.registered.borrow_mut().remove(&hotkey) {
                Ok(())
            } else {
                Err("尚未注册".to_owned())
            }
        }
    }

    #[test]
    fn captures_function_and_modified_letter_shortcuts() {
        assert_eq!(
            shortcut_from_key_event("\u{F70F}", false, false, false, false).unwrap(),
            "F12"
        );
        let letter = shortcut_from_key_event("a", true, false, true, false).unwrap();
        assert_eq!(display_shortcut(Some(&letter)), "Ctrl + Shift + A");
        assert!(shortcut_from_key_event("a", false, false, false, false).is_err());
    }

    #[test]
    fn rejects_function_keys_above_f12() {
        let captured = shortcut_from_key_event("\u{F710}", false, false, false, false).unwrap_err();
        assert!(captured.message.contains("F1–F12"));

        let f13 = "F13".parse::<HotKey>().unwrap();
        assert!(validate_hotkey(f13).is_err());
        let modified_f13 = "control+F13".parse::<HotKey>().unwrap();
        assert!(validate_hotkey(modified_f13).is_err());

        let configured = parse_shortcuts([Some("F13".to_owned()), None, None]).unwrap_err();
        assert_eq!(configured.action, Some(0));
        assert!(configured.message.contains("F1–F12"));
    }

    #[test]
    fn duplicate_shortcuts_are_rejected_before_registration() {
        let issue =
            parse_shortcuts([Some("F10".to_owned()), Some("F10".to_owned()), None]).unwrap_err();
        assert_eq!(issue.action, Some(1));
        assert!(issue.message.contains("重复"));
    }

    #[test]
    fn failed_replacement_restores_the_previous_shortcuts() {
        let registrar = FakeRegistrar::default();
        let old_start = "F10".parse::<HotKey>().unwrap();
        let old_pause = "F11".parse::<HotKey>().unwrap();
        let rejected = "F12".parse::<HotKey>().unwrap();
        registrar.register(old_start).unwrap();
        registrar.register(old_pause).unwrap();
        registrar.rejected.borrow_mut().insert(rejected);

        let result = replace_registered(
            &registrar,
            [Some(old_start), Some(old_pause), None],
            [Some(old_start), Some(old_pause), Some(rejected)],
        );

        let failure = result.unwrap_err();
        assert!(failure.restored_old);
        let registered = registrar.registered.borrow();
        assert!(registered.contains(&old_start));
        assert!(registered.contains(&old_pause));
        assert!(!registered.contains(&rejected));
    }
}
