//! User interface translations.
//!
//! Strings are written in English in the code and looked up with [`tr`].
//! Each language is a tab-separated table (`English<TAB>Translation` per
//! line) in `lang/`; extra tables can be dropped in
//! `~/.config/merge360/lang/<code>.txt` to add or override translations.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Language {
    #[default]
    English,
    French,
    German,
    Spanish,
}

impl Language {
    pub const ALL: [Language; 4] = [Language::English, Language::French, Language::German, Language::Spanish];

    /// Name of the language in itself.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::French => "Français",
            Language::German => "Deutsch",
            Language::Spanish => "Español",
        }
    }

    fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::French => "fr",
            Language::German => "de",
            Language::Spanish => "es",
        }
    }

    fn builtin(self) -> &'static str {
        match self {
            Language::English => "",
            Language::French => include_str!("lang/fr.txt"),
            Language::German => include_str!("lang/de.txt"),
            Language::Spanish => include_str!("lang/es.txt"),
        }
    }

    fn index(self) -> u8 {
        Self::ALL.iter().position(|l| *l == self).unwrap_or(0) as u8
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);
static TABLES: [OnceLock<HashMap<&'static str, &'static str>>; 4] = [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()];

pub fn set_language(lang: Language) {
    CURRENT.store(lang.index(), Ordering::Relaxed);
}

pub fn language() -> Language {
    Language::ALL[CURRENT.load(Ordering::Relaxed) as usize % Language::ALL.len()]
}

fn parse(text: &'static str, out: &mut HashMap<&'static str, &'static str>) {
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        if let Some((en, tr)) = line.split_once('\t') {
            let tr = tr.trim();
            if !en.is_empty() && !tr.is_empty() {
                out.insert(en, tr);
            }
        }
    }
}

fn table(lang: Language) -> &'static HashMap<&'static str, &'static str> {
    TABLES[lang.index() as usize].get_or_init(|| {
        let mut map = HashMap::new();
        parse(lang.builtin(), &mut map);
        // User overrides (leaked once: the table lives for the whole run).
        if let Some(dir) = crate::files::config_dir() {
            if let Ok(text) = std::fs::read_to_string(dir.join("lang").join(format!("{}.txt", lang.code()))) {
                parse(Box::leak(text.into_boxed_str()), &mut map);
            }
        }
        map
    })
}

/// Translate an English UI string into the current language (unknown
/// strings are returned unchanged).
pub fn tr(s: &str) -> &str {
    let lang = language();
    if lang == Language::English {
        return s;
    }
    table(lang).get(s).copied().unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_well_formed() {
        for lang in Language::ALL {
            for (n, line) in lang.builtin().lines().enumerate() {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let parts: Vec<&str> = line.split('\t').collect();
                assert_eq!(parts.len(), 2, "{lang:?} line {}: {line:?}", n + 1);
            }
        }
    }
}
