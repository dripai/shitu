use global_hotkey::{
    GlobalHotKeyManager,
    hotkey::{Code, HotKey, Modifiers},
};

pub struct HotkeyState {
    manager: Option<GlobalHotKeyManager>,
    registered: Option<HotKey>,
    error: Option<String>,
}

impl HotkeyState {
    pub fn new(binding: Option<&str>) -> Self {
        let manager = GlobalHotKeyManager::new().ok();
        let mut state = Self {
            manager,
            registered: None,
            error: None,
        };
        state.set_binding(binding);
        state
    }

    pub fn set_binding(&mut self, binding: Option<&str>) {
        if let (Some(manager), Some(hotkey)) = (&self.manager, self.registered) {
            let _ = manager.unregister(hotkey);
        }
        self.registered = None;
        self.error = None;

        let Some(binding) = binding.filter(|value| !value.trim().is_empty()) else {
            return;
        };
        let Some(hotkey) = parse_hotkey(binding) else {
            self.error = Some("Invalid hotkey".to_owned());
            return;
        };
        let Some(manager) = &self.manager else {
            self.error = Some("Global hotkey manager unavailable".to_owned());
            return;
        };
        if let Err(err) = manager.register(hotkey) {
            self.error = Some(err.to_string());
            return;
        }
        self.registered = Some(hotkey);
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
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
            "space" => code = Some(Code::Space),
            _ => return None,
        }
    }
    code.map(|code| HotKey::new(Some(modifiers), code))
}

#[cfg(test)]
mod tests {
    use super::parse_hotkey;

    #[test]
    fn parses_modifier_key_binding() {
        assert!(parse_hotkey("Ctrl+Alt+Space").is_some());
        assert!(parse_hotkey("Win+Shift+K").is_some());
    }

    #[test]
    fn rejects_unknown_key_binding() {
        assert!(parse_hotkey("Ctrl+Alt+Unknown").is_none());
        assert!(parse_hotkey("Ctrl++").is_none());
    }
}
