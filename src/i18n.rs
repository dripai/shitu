use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::LanguageMode;

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn apply(mode: LanguageMode) -> Result<(), slint::SelectBundledTranslationError> {
    let english = uses_english(mode);
    slint::select_bundled_translation(if english { "en" } else { "" })?;
    ENGLISH.store(english, Ordering::Relaxed);
    Ok(())
}

pub fn prepare(mode: LanguageMode) {
    ENGLISH.store(uses_english(mode), Ordering::Relaxed);
}

fn uses_english(mode: LanguageMode) -> bool {
    match mode {
        LanguageMode::Chinese => false,
        LanguageMode::English => true,
        LanguageMode::System => !system_locale().is_some_and(|locale| is_chinese_locale(&locale)),
    }
}

fn is_chinese_locale(locale: &str) -> bool {
    locale.eq_ignore_ascii_case("zh")
        || locale.get(..3).is_some_and(|prefix| {
            prefix.eq_ignore_ascii_case("zh-") || prefix.eq_ignore_ascii_case("zh_")
        })
}

pub fn text<'a>(chinese: &'a str, english: &'a str) -> &'a str {
    if ENGLISH.load(Ordering::Relaxed) {
        english
    } else {
        chinese
    }
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
    use super::is_chinese_locale;

    #[test]
    fn chinese_locale_detection_accepts_common_windows_and_posix_forms() {
        assert!(is_chinese_locale("zh-CN"));
        assert!(is_chinese_locale("zh_TW.UTF-8"));
        assert!(is_chinese_locale("ZH"));
        assert!(!is_chinese_locale("en-US"));
        assert!(!is_chinese_locale("ja-JP"));
    }
}
