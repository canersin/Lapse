//! Minimal UI translation. Strings are written in English in the code and wrapped in
//! `t("...")`; the Turkish table maps them. Missing entries fall back to English.

mod tr;

use std::sync::atomic::{AtomicU8, Ordering};

static LANGUAGE: AtomicU8 = AtomicU8::new(0);

pub const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("tr", "Türkçe")];

pub fn set_language(code: &str) {
    LANGUAGE.store(if code == "tr" { 1 } else { 0 }, Ordering::Relaxed);
}

pub fn t(en: &'static str) -> &'static str {
    tr_any(en)
}

/// Like `t`, for text built at runtime (e.g. backend messages). Unknown text is returned as is.
pub fn tr_any(en: &str) -> &str {
    match LANGUAGE.load(Ordering::Relaxed) {
        1 => tr::lookup(en).unwrap_or(en),
        _ => en,
    }
}
