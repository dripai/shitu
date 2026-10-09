use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum LanguageMode {
    #[default]
    System,
    Chinese,
    English,
    Japanese,
    Korean,
    French,
    German,
    Spanish,
    Portuguese,
    Russian,
    Hindi,
}

impl LanguageMode {
    pub const ALL: [Self; 11] = [
        Self::System,
        Self::Chinese,
        Self::English,
        Self::Japanese,
        Self::Korean,
        Self::French,
        Self::German,
        Self::Spanish,
        Self::Portuguese,
        Self::Russian,
        Self::Hindi,
    ];

    pub fn bundle_code(self) -> &'static str {
        match self {
            Self::System => panic!("resolve the system language before selecting a catalog"),
            Self::Chinese => "zh",
            Self::English => "en",
            Self::Japanese => "ja",
            Self::Korean => "ko",
            Self::French => "fr",
            Self::German => "de",
            Self::Spanish => "es",
            Self::Portuguese => "pt",
            Self::Russian => "ru",
            Self::Hindi => "hi",
        }
    }

    pub fn from_locale(locale: &str) -> Self {
        let language = locale
            .split(['-', '_', '.', '@'])
            .next()
            .unwrap_or_default();
        match language.to_ascii_lowercase().as_str() {
            "zh" => Self::Chinese,
            "ja" => Self::Japanese,
            "ko" => Self::Korean,
            "fr" => Self::French,
            "de" => Self::German,
            "es" => Self::Spanish,
            "pt" => Self::Portuguese,
            "ru" => Self::Russian,
            "hi" => Self::Hindi,
            // Keep the existing product rule: unsupported system languages use English.
            _ => Self::English,
        }
    }
}

static LANGUAGE: AtomicU8 = AtomicU8::new(LanguageMode::Chinese as u8);

pub fn prepare(mode: LanguageMode) {
    LANGUAGE.store(resolve(mode) as u8, Ordering::Relaxed);
}

fn resolve(mode: LanguageMode) -> LanguageMode {
    match mode {
        LanguageMode::System => system_locale()
            .as_deref()
            .map(LanguageMode::from_locale)
            .unwrap_or(LanguageMode::English),
        language => language,
    }
}

pub fn current_language() -> LanguageMode {
    LanguageMode::ALL[LANGUAGE.load(Ordering::Relaxed) as usize]
}

#[cfg(windows)]
fn system_locale() -> Option<String> {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;

    let mut buffer = [0u16; 85];
    let length = unsafe { GetUserDefaultLocaleName(&mut buffer) };
    (length > 1).then(|| String::from_utf16_lossy(&buffer[..length as usize - 1]))
}

#[cfg(not(windows))]
fn system_locale() -> Option<String> {
    std::env::var("LC_ALL")
        .ok()
        .or_else(|| std::env::var("LANG").ok())
}

#[cfg(test)]
mod tests {
    use super::LanguageMode;

    #[test]
    fn system_language_detection_accepts_regional_windows_and_posix_forms() {
        let cases = [
            ("zh-CN", LanguageMode::Chinese),
            ("zh_TW.UTF-8", LanguageMode::Chinese),
            ("ZH", LanguageMode::Chinese),
            ("en-US", LanguageMode::English),
            ("ja-JP", LanguageMode::Japanese),
            ("ko_KR.UTF-8", LanguageMode::Korean),
            ("fr-CA", LanguageMode::French),
            ("de_DE@euro", LanguageMode::German),
            ("es-MX", LanguageMode::Spanish),
            ("pt-BR", LanguageMode::Portuguese),
            ("pt_PT.UTF-8", LanguageMode::Portuguese),
            ("ru-RU", LanguageMode::Russian),
            ("HI_IN.UTF-8", LanguageMode::Hindi),
        ];
        for (locale, expected) in cases {
            assert_eq!(LanguageMode::from_locale(locale), expected, "{locale}");
        }
    }

    #[test]
    fn unsupported_system_languages_keep_the_english_default() {
        for locale in ["", "C", "C.UTF-8", "it-IT", "japanese", "zhong", "rupee"] {
            assert_eq!(LanguageMode::from_locale(locale), LanguageMode::English);
        }
    }

    #[test]
    fn catalog_codes_match_the_selected_language() {
        let codes = ["zh", "en", "ja", "ko", "fr", "de", "es", "pt", "ru", "hi"];
        for (mode, code) in LanguageMode::ALL[1..].iter().zip(codes) {
            assert_eq!(mode.bundle_code(), code);
        }
    }
}
