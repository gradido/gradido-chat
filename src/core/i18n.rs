//! The web chat's own strings, copied into `locales/` by `tools/sync-assets.py`,
//! plus the few this program adds (`locales/app.json`, under `app.`).
//!
//! The format is vue-i18n's: `{name}` placeholders and `one | many` plurals.

use serde_json::Value;
use std::collections::HashMap;

const LOCALES: &[(&str, &str)] = &[
    ("de", include_str!("../../locales/de.json")),
    ("en", include_str!("../../locales/en.json")),
    ("es", include_str!("../../locales/es.json")),
    ("fr", include_str!("../../locales/fr.json")),
    ("nl", include_str!("../../locales/nl.json")),
    ("tr", include_str!("../../locales/tr.json")),
    ("it", include_str!("../../locales/it.json")),
    ("ru", include_str!("../../locales/ru.json")),
    ("pt", include_str!("../../locales/pt.json")),
    ("el", include_str!("../../locales/el.json")),
];
const APP: &str = include_str!("../../locales/app.json");
const FALLBACK: &str = "en";

pub struct I18n {
    lang: String,
    strings: HashMap<String, String>,
    fallback: HashMap<String, String>,
}

fn flatten(value: &Value, prefix: &str, into: &mut HashMap<String, String>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
                flatten(inner, &path, into);
            }
        }
        Value::String(text) => {
            into.insert(prefix.to_string(), text.clone());
        }
        _ => {}
    }
}

fn load(lang: &str) -> HashMap<String, String> {
    let mut strings = HashMap::new();
    if let Some((_, json)) = LOCALES.iter().find(|(code, _)| *code == lang)
        && let Ok(value) = serde_json::from_str::<Value>(json)
    {
        flatten(&value, "", &mut strings);
    }
    if let Ok(app) = serde_json::from_str::<Value>(APP)
        && let Some(own) = app.get(lang)
    {
        flatten(own, "app", &mut strings);
    }
    strings
}

pub fn supported(lang: &str) -> bool {
    LOCALES.iter().any(|(code, _)| *code == lang)
}

/// The system's language, as far as the environment tells it.
pub fn system_language() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|value| value.chars().take(2).collect::<String>().to_lowercase())
        .find(|code| supported(code))
        .unwrap_or_else(|| FALLBACK.to_string())
}

impl I18n {
    pub fn new(lang: &str) -> Self {
        let lang = if supported(lang) { lang } else { FALLBACK };
        Self {
            lang: lang.to_string(),
            strings: load(lang),
            fallback: load(FALLBACK),
        }
    }

    pub fn lang(&self) -> &str {
        &self.lang
    }

    fn raw<'a>(&'a self, key: &'a str) -> &'a str {
        self.strings.get(key).or_else(|| self.fallback.get(key)).map_or(key, String::as_str)
    }

    /// The singular form of a plural message, as vue-i18n gives it without a count.
    pub fn t(&self, key: &str) -> String {
        self.raw(key).split(" | ").next().unwrap_or("").to_string()
    }

    pub fn with(&self, key: &str, args: &[(&str, &str)]) -> String {
        fill(&self.t(key), args)
    }

    /// Picks the plural form for `n`; `{n}` and `{count}` are filled with it.
    pub fn plural(&self, key: &str, n: i64, args: &[(&str, &str)]) -> String {
        let forms: Vec<&str> = self.raw(key).split(" | ").collect();
        let pick = if self.lang == "ru" {
            slavic(n, forms.len())
        } else {
            default_rule(n, forms.len())
        };
        let count = n.to_string();
        let mut all = vec![("n", count.as_str()), ("count", count.as_str())];
        all.extend_from_slice(args);
        fill(forms[pick.min(forms.len() - 1)], &all)
    }

    pub fn date(&self, at: chrono::DateTime<chrono::Local>) -> String {
        let pattern = match self.lang.as_str() {
            "de" | "ru" | "tr" => "%-d.%-m.%Y",
            "nl" => "%-d-%-m-%Y",
            "en" => "%-m/%-d/%Y",
            _ => "%-d/%-m/%Y",
        };
        at.format(pattern).to_string()
    }

    pub fn month_and_year(&self, at: chrono::DateTime<chrono::Local>) -> String {
        at.format("%m/%Y").to_string()
    }

    pub fn time(&self, at: chrono::DateTime<chrono::Local>) -> String {
        if self.lang == "en" {
            at.format("%-I:%M %p").to_string()
        } else {
            at.format("%H:%M").to_string()
        }
    }

    /// `1.234,50` the way the language writes it.
    pub fn amount(&self, value: f64) -> String {
        let text = format!("{:.2}", value.abs());
        let (whole, cents) = text.split_once('.').unwrap_or((&text, "00"));
        let (thousands, decimal) = if self.lang == "en" { (",", ".") } else { (".", ",") };
        let mut grouped = String::new();
        for (i, digit) in whole.chars().enumerate() {
            if i > 0 && (whole.len() - i) % 3 == 0 {
                grouped.push_str(thousands);
            }
            grouped.push(digit);
        }
        format!("{grouped}{decimal}{cents}")
    }
}

fn fill(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

fn default_rule(n: i64, forms: usize) -> usize {
    let n = n.abs();
    if forms == 2 { if n == 1 { 0 } else { 1 } } else { (n as usize).min(2) }
}

/// frontend/src/utils/pluralRules.js
fn slavic(n: i64, forms: usize) -> usize {
    let n = n.abs();
    if forms < 3 {
        return if n == 1 { 0 } else { 1 };
    }
    let (mod10, mod100) = (n % 10, n % 100);
    if mod10 == 1 && mod100 != 11 {
        0
    } else if (2..=4).contains(&mod10) && !(12..=14).contains(&mod100) {
        1
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_the_chat_strings() {
        for (code, _) in LOCALES {
            let i18n = I18n::new(code);
            assert_ne!(i18n.t("chatThread.send"), "chatThread.send", "{code}");
        }
    }

    #[test]
    fn plurals_and_placeholders() {
        let de = I18n::new("de");
        assert_eq!(de.plural("contacts.bookings", 1, &[]), "1 Buchung");
        assert_eq!(de.plural("contacts.bookings", 3, &[]), "3 Buchungen");
        assert_eq!(de.with("chatThread.replyingTo", &[("name", "Bibi")]), "Antwort an Bibi");
        assert_eq!(de.t("app.menu.quit"), "Beenden");
        assert_eq!(I18n::new("fr").t("app.menu.quit"), "Quit");
    }

    #[test]
    fn russian_declines() {
        let picks: Vec<usize> = [1, 2, 5, 11, 21, 22, 25].iter().map(|n| slavic(*n, 3)).collect();
        assert_eq!(picks, vec![0, 1, 2, 2, 0, 1, 2]);
    }

    #[test]
    fn amounts() {
        assert_eq!(I18n::new("de").amount(-1234.5), "1.234,50");
        assert_eq!(I18n::new("en").amount(1234567.0), "1,234,567.00");
    }
}
