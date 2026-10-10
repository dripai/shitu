use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use global_hotkey::{
    GlobalHotKeyManager,
    hotkey::{Code, HotKey, Modifiers},
};

use crate::i18n;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HotkeyFailure {
    Invalid,
    Occupied,
    SystemRejected,
}

impl HotkeyFailure {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Invalid => i18n::text("组合键无效"),
            Self::Occupied => i18n::text("已被占用"),
            Self::SystemRejected => i18n::text("系统拒绝注册"),
        }
    }
}

pub struct HotkeyState {
    manager: Option<GlobalHotKeyManager>,
    registered: Option<HotKey>,
    binding: Option<String>,
    error: Option<HotkeyFailure>,
    active_id: Arc<AtomicU32>,
}

impl HotkeyState {
    pub fn new(binding: Option<&str>) -> Self {
        let manager = GlobalHotKeyManager::new().ok();
        let mut state = Self {
            manager,
            registered: None,
            binding: None,
            error: None,
            active_id: Arc::new(AtomicU32::new(0)),
        };
        if let Err(error) = state.set_binding(binding) {
            state.error = Some(error);
        }
        state
    }

    pub fn set_binding(&mut self, binding: Option<&str>) -> Result<(), HotkeyFailure> {
        let normalized = binding
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        if normalized == self.binding {
            self.error = None;
            return Ok(());
        }

        let candidate = match normalized.as_deref() {
            Some(value) => Some(parse_hotkey(value).ok_or(HotkeyFailure::Invalid)?),
            None => None,
        };
        // Recording normalizes spelling/order. The same physical combination
        // must not be registered twice just because its display text changed.
        if candidate == self.registered && candidate.is_some() {
            self.binding = normalized;
            self.error = None;
            return Ok(());
        }
        if candidate.is_none() && self.registered.is_none() {
            self.binding = None;
            self.error = None;
            self.active_id.store(0, Ordering::Relaxed);
            return Ok(());
        }
        let manager = self.manager.as_ref().ok_or(HotkeyFailure::SystemRejected)?;

        if let Some(candidate) = candidate {
            manager
                .register(candidate)
                .map_err(classify_registration_error)?;
        }
        if let Some(previous) = self.registered
            && let Err(error) = manager.unregister(previous)
        {
            if let Some(candidate) = candidate {
                let _ = manager.unregister(candidate);
            }
            return Err(classify_registration_error(error));
        }

        self.registered = candidate;
        self.binding = normalized;
        self.error = None;
        self.active_id.store(
            candidate.map(|hotkey| hotkey.id()).unwrap_or(0),
            Ordering::Relaxed,
        );
        Ok(())
    }

    pub fn error(&self) -> Option<&HotkeyFailure> {
        self.error.as_ref()
    }

    pub fn active_id_handle(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.active_id)
    }

    pub fn binding(&self) -> Option<&str> {
        self.binding.as_deref()
    }
}

fn classify_registration_error(error: impl ToString) -> HotkeyFailure {
    let message = error.to_string().to_ascii_lowercase();
    if message.contains("already")
        || message.contains("registered")
        || message.contains("occupied")
        || message.contains("占用")
    {
        HotkeyFailure::Occupied
    } else {
        HotkeyFailure::SystemRejected
    }
}

pub fn validate_binding(value: &str) -> Result<(), HotkeyFailure> {
    if value.trim().is_empty() || parse_hotkey(value).is_some() {
        Ok(())
    } else {
        Err(HotkeyFailure::Invalid)
    }
}

/// GPUI's Windows keyboard mapper restores Shift+digit from shifted symbols.
/// Keep recorded values inside the same supported grammar as saved bindings.
pub fn recorded_binding(stroke: &gpui_kit::KeybindingKeystroke) -> Option<String> {
    let modifiers = stroke.modifiers();
    if modifiers.function {
        return None;
    }
    let mut parts = Vec::new();
    for (enabled, label) in [
        (modifiers.control, "Ctrl"),
        (modifiers.alt, "Alt"),
        (modifiers.shift, "Shift"),
        (modifiers.platform, "Win"),
    ] {
        if enabled {
            parts.push(label.to_owned());
        }
    }
    parts.push(if stroke.key() == "space" {
        "Space".to_owned()
    } else {
        stroke.key().to_ascii_uppercase()
    });
    let binding = parts.join("+");
    parse_hotkey(&binding).map(|_| binding)
}

pub fn parse_hotkey(value: &str) -> Option<HotKey> {
    let mut modifiers = Modifiers::empty();
    let mut code = None;
    for part in value
        .split('+')
        .map(|part| part.trim().to_ascii_lowercase())
    {
        match part.as_str() {
            "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
            "alt" => modifiers |= Modifiers::ALT,
            "shift" => modifiers |= Modifiers::SHIFT,
            "win" | "super" | "meta" => modifiers |= Modifiers::SUPER,
            key if key.len() == 1 => {
                if code.is_some() {
                    return None;
                }
                let ch = key.chars().next()?;
                code = match ch {
                    'a' => Some(Code::KeyA),
                    'b' => Some(Code::KeyB),
                    'c' => Some(Code::KeyC),
                    'd' => Some(Code::KeyD),
                    'e' => Some(Code::KeyE),
                    'f' => Some(Code::KeyF),
                    'g' => Some(Code::KeyG),
                    'h' => Some(Code::KeyH),
                    'i' => Some(Code::KeyI),
                    'j' => Some(Code::KeyJ),
                    'k' => Some(Code::KeyK),
                    'l' => Some(Code::KeyL),
                    'm' => Some(Code::KeyM),
                    'n' => Some(Code::KeyN),
                    'o' => Some(Code::KeyO),
                    'p' => Some(Code::KeyP),
                    'q' => Some(Code::KeyQ),
                    'r' => Some(Code::KeyR),
                    's' => Some(Code::KeyS),
                    't' => Some(Code::KeyT),
                    'u' => Some(Code::KeyU),
                    'v' => Some(Code::KeyV),
                    'w' => Some(Code::KeyW),
                    'x' => Some(Code::KeyX),
                    'y' => Some(Code::KeyY),
                    'z' => Some(Code::KeyZ),
                    '0' => Some(Code::Digit0),
                    '1' => Some(Code::Digit1),
                    '2' => Some(Code::Digit2),
                    '3' => Some(Code::Digit3),
                    '4' => Some(Code::Digit4),
                    '5' => Some(Code::Digit5),
                    '6' => Some(Code::Digit6),
                    '7' => Some(Code::Digit7),
                    '8' => Some(Code::Digit8),
                    '9' => Some(Code::Digit9),
                    _ => None,
                };
            }
            "space" if code.is_none() => code = Some(Code::Space),
            "f1" if code.is_none() => code = Some(Code::F1),
            "f2" if code.is_none() => code = Some(Code::F2),
            "f3" if code.is_none() => code = Some(Code::F3),
            "f4" if code.is_none() => code = Some(Code::F4),
            "f5" if code.is_none() => code = Some(Code::F5),
            "f6" if code.is_none() => code = Some(Code::F6),
            "f7" if code.is_none() => code = Some(Code::F7),
            "f8" if code.is_none() => code = Some(Code::F8),
            "f9" if code.is_none() => code = Some(Code::F9),
            "f10" if code.is_none() => code = Some(Code::F10),
            "f11" if code.is_none() => code = Some(Code::F11),
            "f12" if code.is_none() => code = Some(Code::F12),
            _ => return None,
        }
    }
    if modifiers.is_empty() {
        return None;
    }
    code.map(|code| HotKey::new(Some(modifiers), code))
}

#[cfg(test)]
mod tests {
    use super::{HotkeyState, parse_hotkey, recorded_binding};
    use gpui_kit::{KeybindingKeystroke, Keystroke, Modifiers};
    use std::sync::{Arc, atomic::AtomicU32};

    #[test]
    fn recorded_combinations_round_trip_to_supported_hotkeys() {
        for (keys, expected) in [
            ("ctrl-alt-c", "Ctrl+Alt+C"),
            ("ctrl-shift-2", "Ctrl+Shift+2"),
            ("win-shift-k", "Shift+Win+K"),
            ("alt-space", "Alt+Space"),
            ("ctrl-f12", "Ctrl+F12"),
        ] {
            let stroke = KeybindingKeystroke::from_keystroke(Keystroke::parse(keys).unwrap());
            assert_eq!(recorded_binding(&stroke).as_deref(), Some(expected));
            assert!(parse_hotkey(expected).is_some());
        }
        for keys in ["c", "ctrl-escape", "ctrl-f13", "ctrl-;", "fn-ctrl-c"] {
            let stroke = KeybindingKeystroke::from_keystroke(Keystroke::parse(keys).unwrap());
            assert!(recorded_binding(&stroke).is_none(), "{keys}");
        }
        // Windows maps Ctrl+@ back to Ctrl+Shift+2 for display. Recording must
        // use that mapping rather than the inner shifted character.
        let mapped = KeybindingKeystroke::new(
            Keystroke::parse("ctrl-@").unwrap(),
            Modifiers {
                control: true,
                shift: true,
                ..Default::default()
            },
            "2".into(),
        );
        assert_eq!(recorded_binding(&mapped).as_deref(), Some("Ctrl+Shift+2"));
    }

    #[test]
    fn equivalent_recorded_binding_does_not_reregister() {
        let key = parse_hotkey("alt+control+c").unwrap();
        // No native manager: a needless registration would fail this test.
        let mut state = HotkeyState {
            manager: None,
            registered: Some(key),
            binding: Some("alt+control+c".into()),
            error: None,
            active_id: Arc::new(AtomicU32::new(key.id())),
        };
        assert!(state.set_binding(Some("Ctrl+Alt+C")).is_ok());
        assert_eq!(state.binding(), Some("Ctrl+Alt+C"));
        assert_eq!(state.registered, Some(key));
        assert!(state.set_binding(Some("Ctrl+Alt+D")).is_err());
        assert_eq!(state.binding(), Some("Ctrl+Alt+C"));
    }

    #[test]
    fn parses_modifier_key_binding() {
        assert!(parse_hotkey("Ctrl+Alt+C").is_some());
        assert!(parse_hotkey("Ctrl+Alt+Space").is_some());
        assert!(parse_hotkey("Win+Shift+K").is_some());
        assert!(parse_hotkey("Ctrl+F3").is_some());
    }

    #[test]
    fn rejects_unknown_or_unmodified_key_binding() {
        assert!(parse_hotkey("Ctrl+Alt+Unknown").is_none());
        assert!(parse_hotkey("Ctrl++").is_none());
        assert!(parse_hotkey("Ctrl+A+B").is_none());
        assert!(parse_hotkey("Ctrl+Space+F1").is_none());
        assert!(parse_hotkey("A").is_none());
    }
}
