//! Translations. English is the base language of the whole suite; German is a translation.
//!
//! Write every user-facing text in English and wrap it: `tr("Save workspace")`. With a
//! German system language the German text from [`de`] is shown; a text missing there stays
//! English. Texts with values use named placeholders: `trf("{n} windows", &[("n", &3)])`.
//!
//! The language comes from `TORRO_LANG` (`en`, `de`), else from `LC_ALL`, `LC_MESSAGES`,
//! `LANG` (anything starting with `de` → German), else English. It is read once per process.

mod de;

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;

/// A language the suite speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
}

static LANG: OnceLock<Lang> = OnceLock::new();

/// The language of this process.
pub fn lang() -> Lang {
    *LANG.get_or_init(|| detect(|key| std::env::var(key).ok()))
}

/// The language from environment variables, looked up through `env`.
pub fn detect(env: impl Fn(&str) -> Option<String>) -> Lang {
    let pick = |value: &str| {
        let value = value.trim().to_ascii_lowercase();
        if value.is_empty() || value == "c" || value == "posix" {
            None
        } else if value.starts_with("de") {
            Some(Lang::De)
        } else {
            Some(Lang::En)
        }
    };
    ["TORRO_LANG", "LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|key| env(key))
        .find_map(|value| pick(&value))
        .unwrap_or(Lang::En)
}

/// `english` in the language of this process.
pub fn tr(english: &'static str) -> &'static str {
    tr_in(lang(), english)
}

/// `english` in `lang`.
pub fn tr_in(lang: Lang, english: &'static str) -> &'static str {
    match lang {
        Lang::En => english,
        Lang::De => german().get(english).copied().unwrap_or(english),
    }
}

/// [`tr`] with `{name}` placeholders replaced by `args`.
pub fn trf(english: &'static str, args: &[(&str, &dyn Display)]) -> String {
    fill(tr(english), args)
}

fn fill(text: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = text.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

fn german() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| de::TABLE.iter().copied().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| pairs.iter().find(|(k, _)| *k == key).map(|(_, v)| v.to_string())
    }

    #[test]
    fn detects_language() {
        assert_eq!(detect(env(&[("LANG", "de_DE.UTF-8")])), Lang::De);
        assert_eq!(detect(env(&[("LANG", "en_US.UTF-8")])), Lang::En);
        assert_eq!(detect(env(&[("LANG", "de_DE.UTF-8"), ("TORRO_LANG", "en")])), Lang::En);
        assert_eq!(detect(env(&[("LC_ALL", "C"), ("LANG", "de_AT.UTF-8")])), Lang::De);
        assert_eq!(detect(env(&[])), Lang::En);
    }

    #[test]
    fn translates_and_falls_back_to_english() {
        assert_eq!(tr_in(Lang::De, "Save workspace"), "Workspace speichern");
        assert_eq!(tr_in(Lang::En, "Save workspace"), "Save workspace");
        assert_eq!(tr_in(Lang::De, "No such text"), "No such text");
        assert_eq!(fill("{n} windows", &[("n", &3)]), "3 windows");
    }

    #[test]
    fn table_has_no_duplicates_and_keeps_placeholders() {
        let mut seen = std::collections::HashSet::new();
        for (english, german) in de::TABLE {
            assert!(seen.insert(english), "duplicate entry {english:?}");
            let placeholders = |s: &str| {
                let mut found: Vec<String> =
                    s.split('{').skip(1).filter_map(|rest| rest.split_once('}').map(|(n, _)| n.to_string())).collect();
                found.sort();
                found
            };
            assert_eq!(placeholders(english), placeholders(german), "placeholders differ in {english:?}");
        }
    }
}
