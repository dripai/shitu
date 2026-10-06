use std::{collections::BTreeMap, fmt::Write, fs, path::Path};

use regex::Regex;
use rspolib::TranslatedEntry;

const LANGUAGES: [(&str, &str); 10] = [
    ("zh", "Chinese"),
    ("en", "English"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("fr", "French"),
    ("de", "German"),
    ("es", "Spanish"),
    ("pt", "Portuguese"),
    ("ru", "Russian"),
    ("hi", "Hindi"),
];

type Catalog = BTreeMap<String, String>;

pub fn compile() {
    println!("cargo:rerun-if-changed=translations");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-changed=../../crates/shi-ui/ui");

    // Use the same PO parser as Slint 1.17.0. Its bundled translations have no
    // public Rust lookup by msgid, so compile our runtime messages from the
    // same catalogs instead of calling Slint's private_unstable_api.
    let catalogs: Vec<_> = LANGUAGES
        .iter()
        .map(|(code, _)| read_catalog(code))
        .collect();
    let reference = &catalogs[1];
    // Slint's native LineEdit/TextEdit menus use these English source keys.
    for key in ["Cut", "Copy", "Paste", "Select All"] {
        assert!(
            reference.contains_key(key),
            "missing native text menu translation: {key}"
        );
    }
    let placeholders = Regex::new(r"\{[^{}]*\}").unwrap();
    for ((code, _), catalog) in LANGUAGES.iter().zip(&catalogs) {
        assert!(
            catalog.keys().eq(reference.keys()),
            "{code}: translation keys differ from the English catalog"
        );
        for (key, translation) in catalog {
            assert_eq!(
                placeholder_counts(key, &placeholders),
                placeholder_counts(translation, &placeholders),
                "{code}: translation placeholders differ for {key:?}"
            );
        }
    }
    check_source_keys(Path::new("src"), "rs", r"i18n::text", reference);
    check_source_keys(Path::new("ui"), "slint", "@tr", reference);
    check_source_keys(
        Path::new("../../crates/shi-ui/ui"),
        "slint",
        "@tr",
        reference,
    );

    let mut generated = String::from(
        "// Generated from translations/*/LC_MESSAGES/shitu.po.\n\
         fn catalog_text(language: LanguageMode, key: &str) -> &'static str {\n\
         let language_index = match language {\n\
         LanguageMode::System => panic!(\"system language must be resolved\"),\n",
    );
    for (index, (_, variant)) in LANGUAGES.iter().enumerate() {
        writeln!(generated, "LanguageMode::{variant} => {index},").unwrap();
    }
    generated.push_str("};\nlet messages = match key {\n");
    for key in reference.keys() {
        write!(generated, "{key:?} => [").unwrap();
        for (index, catalog) in catalogs.iter().enumerate() {
            if index != 0 {
                generated.push_str(", ");
            }
            write!(generated, "{:?}", catalog[key]).unwrap();
        }
        generated.push_str("],\n");
    }
    generated.push_str(
        "_ => panic!(\"missing translation key: {key}\"),\n\
         };\nmessages[language_index]\n}\n",
    );
    generated.push_str("#[cfg(test)]\nconst CATALOG_KEYS: &[&str] = &[\n");
    for key in reference.keys() {
        writeln!(generated, "{key:?},").unwrap();
    }
    generated.push_str("];\n");
    let output = std::env::var_os("OUT_DIR").expect("Cargo must provide OUT_DIR");
    fs::write(
        Path::new(&output).join("runtime_translations.rs"),
        generated,
    )
    .expect("write runtime translations");
}

fn read_catalog(code: &str) -> Catalog {
    let path = Path::new("translations")
        .join(code)
        .join("LC_MESSAGES/shitu.po");
    let po = rspolib::pofile(path.as_path())
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    assert_eq!(
        po.metadata.get("Language").map(String::as_str),
        Some(code),
        "{}: incorrect Language header",
        path.display()
    );
    let mut catalog = Catalog::new();
    for entry in po.entries {
        assert!(
            entry.msgctxt.is_none() && entry.msgid_plural.is_none(),
            "{}: runtime messages must use singular, context-free keys: {:?}",
            path.display(),
            entry.msgid
        );
        assert!(
            entry.translated(),
            "{}: empty, fuzzy or obsolete translation: {:?}",
            path.display(),
            entry.msgid
        );
        let translation = entry.msgstr.expect("translated entry must have msgstr");
        assert!(
            !translation.trim().is_empty(),
            "{}: blank translation: {:?}",
            path.display(),
            entry.msgid
        );
        assert!(
            catalog.insert(entry.msgid, translation).is_none(),
            "{}: duplicate translation key",
            path.display()
        );
    }
    assert!(!catalog.is_empty(), "{}: empty catalog", path.display());
    catalog
}

fn placeholder_counts(text: &str, pattern: &Regex) -> BTreeMap<String, usize> {
    let mut result = BTreeMap::new();
    for placeholder in pattern.find_iter(text) {
        *result.entry(placeholder.as_str().to_owned()).or_default() += 1;
    }
    result
}

fn check_source_keys(directory: &Path, extension: &str, call: &str, catalog: &Catalog) {
    let calls = Regex::new(&format!(r"{call}\s*\(")).unwrap();
    let literals = Regex::new(&format!(r#"{call}\s*\(\s*("(?:[^"\\]|\\.)*")"#)).unwrap();
    for entry in fs::read_dir(directory).expect("read translation source directory") {
        let path = entry.expect("read translation source entry").path();
        if path.is_dir() {
            check_source_keys(&path, extension, call, catalog);
        } else if path.extension().and_then(|s| s.to_str()) == Some(extension) {
            let source = fs::read_to_string(&path).expect("translation sources must be UTF-8");
            assert_eq!(
                calls.find_iter(&source).count(),
                literals.captures_iter(&source).count(),
                "{}: translation keys must be string literals",
                path.display()
            );
            for capture in literals.captures_iter(&source) {
                let key: String = serde_json::from_str(&capture[1])
                    .expect("translation keys must use plain string literals");
                assert!(
                    catalog.contains_key(&key),
                    "{}: missing translation key: {key:?}",
                    path.display()
                );
            }
        }
    }
}
