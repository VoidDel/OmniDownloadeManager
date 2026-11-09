//! Texts the app shows outside its windows (the notification-area menu, notifications), from the
//! same translation files as the windows (`locales/<lang>/app.json`, checked by
//! `pnpm check:locales`), and sizes written the way the windows write them.

use std::{collections::HashMap, sync::OnceLock};

use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

const FILES: [(&str, &str); 4] = [
    ("zh-CN", include_str!("../../../../locales/zh-CN/app.json")),
    ("zh-TW", include_str!("../../../../locales/zh-TW/app.json")),
    ("en-US", include_str!("../../../../locales/en-US/app.json")),
    ("ja-JP", include_str!("../../../../locales/ja-JP/app.json")),
];

fn tables() -> &'static HashMap<&'static str, Value> {
    static TABLES: OnceLock<HashMap<&'static str, Value>> = OnceLock::new();
    TABLES.get_or_init(|| {
        FILES
            .iter()
            .filter_map(|(lang, text)| Some((*lang, serde_json::from_str(text).ok()?)))
            .collect()
    })
}

/// One of the supported languages for a language tag (`zh_TW`, `zh-Hant-HK`, `ja`, `fr-FR`), as
/// the window decides (`normalizeLang`).
pub fn normalize(tag: &str) -> &'static str {
    let l = tag.replace('_', "-").to_ascii_lowercase();
    if l.starts_with("zh") {
        if ["hant", "tw", "hk", "mo"].iter().any(|x| l.contains(x)) {
            "zh-TW"
        } else {
            "zh-CN"
        }
    } else if l.starts_with("ja") {
        "ja-JP"
    } else {
        "en-US"
    }
}

/// The UI language for the language setting: `auto` follows the system.
pub fn resolve(setting: &str) -> &'static str {
    if setting.is_empty() || setting == "auto" {
        normalize(&sys_locale::get_locale().unwrap_or_default())
    } else {
        normalize(setting)
    }
}

/// The language the app speaks now.
pub fn current(app: &AppHandle) -> &'static str {
    resolve(&app.state::<AppState>().settings().language)
}

/// The text for `key` (`tray.show`) in `lang` with its `{name}` arguments filled in; English when
/// the language lacks it, the key itself when nobody has it.
pub fn tr(lang: &str, key: &str, args: &[(&str, &str)]) -> String {
    let find = |l: &str| {
        let mut v = tables().get(l)?;
        for part in key.split('.') {
            v = v.get(part)?;
        }
        v.as_str().map(str::to_owned)
    };
    let mut text = find(lang)
        .or_else(|| find("en-US"))
        .unwrap_or_else(|| key.to_owned());
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

/// 1024-based, like the window: whole numbers from 100, one decimal from 10, two below
/// (`643 MB`, `5.9 GB`, `1.24 GB`).
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let round = |x: f64| {
        if x >= 100.0 {
            x.round()
        } else if x >= 10.0 {
            (x * 10.0).round() / 10.0
        } else {
            (x * 100.0).round() / 100.0
        }
    };
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    let mut r = round(v);
    // 1023.9 KB rounds to 1024: that is 1 MB.
    if r >= 1024.0 && u < UNITS.len() - 1 {
        u += 1;
        r = round(v / 1024.0);
    }
    // `{}` of an f64 drops trailing zeros (1.5, 12), as the window's numbers do.
    format!("{r} {}", UNITS[u])
}

pub fn speed(bps: u64) -> String {
    format!("{}/s", bytes(bps))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_texts_and_falls_back() {
        assert_eq!(tr("zh-CN", "status.completed", &[]), "已完成");
        assert_eq!(tr("en-US", "status.completed", &[]), "Completed");
        assert_eq!(tr("en-US", "no.such.key", &[]), "no.such.key");
        assert_eq!(
            tr("en-US", "format.s", &[("s", "47")]),
            tr("en-US", "format.s", &[]).replace("{s}", "47")
        );
    }

    #[test]
    fn picks_languages_like_the_window() {
        assert_eq!(normalize("zh_TW"), "zh-TW");
        assert_eq!(normalize("zh-Hant-HK"), "zh-TW");
        assert_eq!(normalize("zh-CN"), "zh-CN");
        assert_eq!(normalize("ja"), "ja-JP");
        assert_eq!(normalize("fr-FR"), "en-US");
        assert_eq!(resolve("ja-JP"), "ja-JP");
    }

    #[test]
    fn writes_sizes_like_the_window() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(5 * 1024 * 1024), "5 MB");
        assert_eq!(bytes(643 * 1024 * 1024), "643 MB");
        assert_eq!(bytes(1024 * 1024 - 1), "1 MB");
        assert_eq!(speed(512 * 1024), "512 KB/s");
    }
}
