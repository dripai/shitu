use shi_foundation::LanguageMode;
pub use shi_foundation::i18n::{apply, current_language, prepare};

// Slint 1.17.0 bundles @tr calls, but exposes no public Rust lookup by msgid
// for that bundle. build.rs compiles the same PO catalogs for worker-thread
// messages; no private Slint API or second set of translations is needed.
include!(concat!(env!("OUT_DIR"), "/runtime_translations.rs"));

pub fn text(key: &str) -> &'static str {
    catalog_text(current_language(), key)
}

#[cfg(test)]
mod tests {
    use super::{CATALOG_KEYS, LanguageMode, catalog_text};

    #[test]
    fn every_message_has_a_translation_in_every_supported_language() {
        for language in LanguageMode::ALL[1..].iter().copied() {
            for key in CATALOG_KEYS {
                assert!(
                    !catalog_text(language, key).trim().is_empty(),
                    "{language:?}: {key}"
                );
            }
        }
    }

    #[test]
    fn runtime_messages_use_each_selected_catalog() {
        let cases = [
            (LanguageMode::Chinese, "保存", "快捷键已注册"),
            (LanguageMode::English, "Save", "Hotkey registered"),
            (LanguageMode::Japanese, "保存", "ホットキーを登録しました"),
            (LanguageMode::Korean, "저장", "단축키가 등록되었습니다"),
            (LanguageMode::French, "Enregistrer", "Raccourci enregistré"),
            (
                LanguageMode::German,
                "Speichern",
                "Tastenkürzel registriert",
            ),
            (LanguageMode::Spanish, "Guardar", "Atajo registrado"),
            (LanguageMode::Portuguese, "Salvar", "Atalho registrado"),
            (
                LanguageMode::Russian,
                "Сохранить",
                "Горячая клавиша зарегистрирована",
            ),
            (LanguageMode::Hindi, "सहेजें", "हॉटकी पंजीकृत है"),
        ];
        for (language, save, hotkey) in cases {
            assert_eq!(catalog_text(language, "保存"), save);
            assert_eq!(catalog_text(language, "快捷键已注册"), hotkey);
        }
    }

    #[test]
    fn language_preferences_round_trip_without_changing_existing_values() {
        let values = [
            "system",
            "chinese",
            "english",
            "japanese",
            "korean",
            "french",
            "german",
            "spanish",
            "portuguese",
            "russian",
            "hindi",
        ];
        for (mode, value) in LanguageMode::ALL.into_iter().zip(values) {
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(json, format!("\"{value}\""));
            assert_eq!(serde_json::from_str::<LanguageMode>(&json).unwrap(), mode);
        }
    }

    #[test]
    #[should_panic(expected = "missing translation key")]
    fn missing_messages_fail_explicitly() {
        catalog_text(LanguageMode::Russian, "unregistered message");
    }
}
