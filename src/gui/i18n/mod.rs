//! Minimal UI translation. Strings are written in English in the code and wrapped in
//! `t("...")`; the loaded language table maps them. Missing entries fall back to English.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
}

#[derive(Deserialize)]
struct LangFile {
    meta: LangMeta,
    #[serde(default)]
    strings: HashMap<String, String>,
}

#[derive(Deserialize)]
struct LangMeta {
    name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct RawLang {
    pub code: String,
    pub name: String,
    pub strings: HashMap<String, String>,
}

struct LanguageState {
    languages: &'static [Language],
    tables: Vec<Option<HashMap<String, &'static str>>>,
}

static STATE: OnceLock<LanguageState> = OnceLock::new();
static CURRENT_LANG: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn parse_lang_file(code: &str, content: &str) -> Result<RawLang, toml::de::Error> {
    let file: LangFile = toml::from_str(content)?;
    Ok(RawLang {
        code: code.to_string(),
        name: file.meta.name,
        strings: file.strings,
    })
}

pub(crate) fn merge_language_sources<I1, I2, S1, S2, S3, S4>(
    builtins: I1,
    user_files: I2,
) -> Vec<RawLang>
where
    I1: IntoIterator<Item = (S1, S2)>,
    I2: IntoIterator<Item = (S3, S4)>,
    S1: AsRef<str>,
    S2: AsRef<str>,
    S3: AsRef<str>,
    S4: AsRef<str>,
{
    let mut map: HashMap<String, RawLang> = HashMap::new();

    for (code, content) in builtins {
        let code = code.as_ref();
        match parse_lang_file(code, content.as_ref()) {
            Ok(raw) => {
                map.insert(code.to_string(), raw);
            }
            Err(err) => {
                eprintln!("Lapse: failed to parse built-in language {}: {}", code, err);
            }
        }
    }

    for (code, content) in user_files {
        let code = code.as_ref();
        match parse_lang_file(code, content.as_ref()) {
            Ok(raw) => {
                map.insert(code.to_string(), raw);
            }
            Err(err) => {
                eprintln!("Lapse: failed to parse user language file {}: {}", code, err);
            }
        }
    }

    let mut list: Vec<RawLang> = map.into_values().collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

fn load_user_lang_files() -> Vec<(String, String)> {
    let mut files = Vec::new();
    let config_dir = match crate::config::Config::get_config_dir() {
        Ok(dir) => dir,
        Err(_) => return files,
    };
    let lang_dir = config_dir.join("lang");
    if !lang_dir.is_dir() {
        return files;
    }
    let entries = match std::fs::read_dir(&lang_dir) {
        Ok(entries) => entries,
        Err(err) => {
            eprintln!("Lapse: failed to read language directory {}: {}", lang_dir.display(), err);
            return files;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                eprintln!("Lapse: failed to read language entry: {}", err);
                continue;
            }
        };
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("toml") {
            let code = match path.file_stem().and_then(|s| s.to_str()) {
                Some(stem) if !stem.is_empty() => stem.to_string(),
                _ => continue,
            };
            match std::fs::read_to_string(&path) {
                Ok(content) => files.push((code, content)),
                Err(err) => {
                    eprintln!("Lapse: failed to read language file {}: {}", path.display(), err);
                }
            }
        }
    }
    files
}

fn build_language_state(raw_langs: Vec<RawLang>) -> LanguageState {
    let mut lang_list = Vec::with_capacity(raw_langs.len() + 1);
    let mut table_list = Vec::with_capacity(raw_langs.len() + 1);

    // English (code "en", name "English") is built in and has no table.
    lang_list.push(Language {
        code: "en",
        name: "English",
    });
    table_list.push(None);

    for raw in raw_langs {
        if raw.code == "en" {
            continue;
        }
        let code: &'static str = Box::leak(raw.code.into_boxed_str());
        let name: &'static str = Box::leak(raw.name.into_boxed_str());
        let mut table: HashMap<String, &'static str> = HashMap::with_capacity(raw.strings.len());
        for (k, v) in raw.strings {
            table.insert(k, Box::leak(v.into_boxed_str()));
        }
        lang_list.push(Language { code, name });
        table_list.push(Some(table));
    }

    let languages: &'static [Language] = Box::leak(lang_list.into_boxed_slice());
    LanguageState {
        languages,
        tables: table_list,
    }
}

fn get_state() -> &'static LanguageState {
    STATE.get_or_init(|| {
        let builtins = [("tr", include_str!("../../../lang/tr.toml"))];
        let user_files = load_user_lang_files();
        let raw_langs = merge_language_sources(builtins, user_files);
        build_language_state(raw_langs)
    })
}

pub fn languages() -> &'static [Language] {
    get_state().languages
}

pub fn set_language(code: &str) {
    let state = get_state();
    let idx = state
        .languages
        .iter()
        .position(|l| l.code == code)
        .unwrap_or(0);
    CURRENT_LANG.store(idx, Ordering::Relaxed);
}

pub fn t(en: &'static str) -> &'static str {
    tr_any(en)
}

/// Like `t`, for text built at runtime (e.g. backend messages). Unknown text is returned as is.
pub fn tr_any(en: &str) -> &str {
    let idx = CURRENT_LANG.load(Ordering::Relaxed);
    if idx == 0 {
        return en;
    }
    let state = get_state();
    if let Some(Some(table)) = state.tables.get(idx) {
        if let Some(&tr) = table.get(en) {
            return tr;
        }
    }
    en
}

/// `en` in the given language, for the daemon, which doesn't set a current one.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn tr_in(code: &str, en: &'static str) -> &'static str {
    let state = get_state();
    let idx = state.languages.iter().position(|l| l.code == code).unwrap_or(0);
    state.tables.get(idx).and_then(|t| t.as_ref()).and_then(|t| t.get(en)).copied().unwrap_or(en)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(en: &str) -> Option<&'static str> {
        let state = get_state();
        let tr_idx = state
            .languages
            .iter()
            .position(|l| l.code == "tr")
            .expect("tr should be loaded");
        state.tables[tr_idx].as_ref().and_then(|t| t.get(en).copied())
    }

    #[test]
    fn translates_and_falls_back() {
        assert_eq!(lookup("Settings"), Some("Ayarlar"));
        assert_eq!(lookup("All games"), Some("Tüm oyunlar"));
        assert_eq!(lookup("Favorites only"), Some("Sadece favoriler"));
        assert_eq!(lookup("Date"), Some("Tarih"));
        assert_eq!(lookup("Any time"), Some("Tüm zamanlar"));
        assert_eq!(lookup("Unfavorite"), Some("Favorilerden Çıkar"));
        assert_eq!(lookup("Game"), Some("Oyun"));
        assert_eq!(lookup("+ Tag"), Some("+ Etiket"));
        assert_eq!(
            lookup("Adds a microphone named “Virtual-Microphone”. Pick it in Discord, games or as Lapse's mic."),
            Some("“Virtual-Microphone” adında bir mikrofon ekler. Discord'da, oyunlarda veya Lapse'in mikrofonu olarak seçin.")
        );
        assert_eq!(
            lookup("Use as default microphone"),
            Some("Varsayılan mikrofon olarak kullan")
        );
        assert_eq!(
            lookup("Other apps like Discord will hear the cleaned-up microphone."),
            Some("Discord gibi diğer uygulamalar temizlenmiş mikrofonu duyar.")
        );
        assert_eq!(lookup("Upload to Streamable"), Some("Streamable'a yükle"));
        assert_eq!(lookup("Crop"), Some("Kırp"));
        assert_eq!(lookup("Free"), Some("Serbest"));
        assert_eq!(lookup("not a known string"), None);
    }

    #[test]
    fn tr_toml_parses_and_has_more_than_700_unique_strings() {
        let content = include_str!("../../../lang/tr.toml");
        let parsed = parse_lang_file("tr", content).expect("tr.toml must parse successfully");
        assert_eq!(parsed.name, "Türkçe");
        assert!(
            parsed.strings.len() > 700,
            "Expected more than 700 strings, got {}",
            parsed.strings.len()
        );
    }

    #[test]
    fn user_file_overrides_builtin() {
        let builtin_tr = r#"
[meta]
name = "Türkçe"
[strings]
"Settings" = "Ayarlar"
"Library" = "Kütüphane"
"#;
        let user_tr = r#"
[meta]
name = "Türkçe (Özel)"
[strings]
"Settings" = "Özel Ayarlar"
"#;
        let merged = merge_language_sources(
            [("tr", builtin_tr)],
            [("tr", user_tr)],
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].code, "tr");
        assert_eq!(merged[0].name, "Türkçe (Özel)");
        assert_eq!(merged[0].strings.get("Settings").map(|s| s.as_str()), Some("Özel Ayarlar"));
        assert_eq!(merged[0].strings.get("Library"), None);
    }

    #[test]
    fn invalid_user_file_is_skipped() {
        let builtin_tr = r#"
[meta]
name = "Türkçe"
[strings]
"Settings" = "Ayarlar"
"#;
        let bad_user_tr = "invalid = [toml { syntax";
        let merged = merge_language_sources(
            [("tr", builtin_tr)],
            [("tr", bad_user_tr)],
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].code, "tr");
        assert_eq!(merged[0].name, "Türkçe");
        assert_eq!(merged[0].strings.get("Settings").map(|s| s.as_str()), Some("Ayarlar"));
    }

    #[test]
    fn user_file_can_add_new_language() {
        let builtin_tr = r#"
[meta]
name = "Türkçe"
[strings]
"Settings" = "Ayarlar"
"#;
        let user_de = r#"
[meta]
name = "Deutsch"
[strings]
"Settings" = "Einstellungen"
"#;
        let merged = merge_language_sources(
            [("tr", builtin_tr)],
            [("de", user_de)],
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].code, "de");
        assert_eq!(merged[0].name, "Deutsch");
        assert_eq!(merged[1].code, "tr");
        assert_eq!(merged[1].name, "Türkçe");
    }

    #[test]
    fn languages_list_starts_with_english() {
        let langs = languages();
        assert!(!langs.is_empty());
        assert_eq!(langs[0].code, "en");
        assert_eq!(langs[0].name, "English");

        if langs.len() > 2 {
            for window in langs[1..].windows(2) {
                assert!(window[0].name <= window[1].name);
            }
        }
    }

    #[test]
    fn set_language_and_translations() {
        set_language("tr");
        assert_eq!(t("Settings"), "Ayarlar");
        assert_eq!(tr_any("not a real string"), "not a real string");

        set_language("unknown_lang");
        assert_eq!(t("Settings"), "Settings");

        set_language("en");
        assert_eq!(t("Settings"), "Settings");
    }
}
