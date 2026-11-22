//! 命令行、终端界面和原生菜单的多语言文案。
//!
//! 语言包是仓库根目录的 `locales/*.json`，和桌面界面共用同一份（规范见 docs/i18n.md）。
//! 编译时打包进程序；用 [`t`] / [`tf`] 按当前语言取文案，找不到时退回英文。
//!
//! 当前语言默认按 `~/.duolo/settings.json` 的 `language` 选择，`auto` 时跟随系统语言，
//! 都不支持时用英文。桌面应用切换语言时调用 [`set_locale`]。

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// 支持的语言代码，以及它们的语言包。与 `app/src/i18n/index.tsx` 的 `LOCALES` 一致。
macro_rules! locales {
    ($($code:literal),* $(,)?) => {
        &[$(($code, include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../locales/", $code, ".json")))),*]
    };
}
const LOCALES: &[(&str, &str)] =
    locales!["en", "zh-CN", "zh-TW", "ja", "ko", "es", "fr", "de", "pt-BR", "ru", "it", "tr", "vi", "id"];

/// 基准语言。
pub const BASE_LOCALE: &str = "en";

type Catalog = HashMap<String, String>;

fn catalogs() -> &'static HashMap<&'static str, Catalog> {
    static CATALOGS: OnceLock<HashMap<&'static str, Catalog>> = OnceLock::new();
    CATALOGS.get_or_init(|| {
        LOCALES
            .iter()
            .map(|(code, json)| (*code, serde_json::from_str(json).expect("locales/*.json is valid JSON")))
            .collect()
    })
}

static CURRENT: RwLock<Option<&'static str>> = RwLock::new(None);

/// 支持的语言代码。
pub fn locales() -> impl Iterator<Item = &'static str> {
    LOCALES.iter().map(|(code, _)| *code)
}

/// 切换当前语言。不支持的代码按 [`match_locale`] 处理。
pub fn set_locale(code: &str) {
    *CURRENT.write().unwrap_or_else(|p| p.into_inner()) = Some(match_locale([code]));
}

/// 当前语言：设置过就用设置的，否则跟随系统语言。
pub fn locale() -> &'static str {
    if let Some(code) = *CURRENT.read().unwrap_or_else(|p| p.into_inner()) {
        return code;
    }
    let code = match_locale(sys_locale::get_locales());
    *CURRENT.write().unwrap_or_else(|p| p.into_inner()) = Some(code);
    code
}

/// 按语言偏好（`auto` 或语言代码）设置当前语言。
pub fn apply_preference(pref: &str) {
    if pref == crate::settings::LANGUAGE_AUTO {
        set_locale_from_system();
    } else {
        set_locale(pref);
    }
}

fn set_locale_from_system() {
    *CURRENT.write().unwrap_or_else(|p| p.into_inner()) = Some(match_locale(sys_locale::get_locales()));
}

/// 把系统语言标签（`zh-Hans-CN`、`pt-PT`、`en-GB`）对应到支持的语言，找不到时用英文。
/// 规则与 `app/src/i18n/index.tsx` 的 `matchLocale` 一致。
pub fn match_locale<S: AsRef<str>>(tags: impl IntoIterator<Item = S>) -> &'static str {
    for tag in tags {
        let tag = tag.as_ref().to_ascii_lowercase().replace('_', "-");
        if tag.starts_with("zh") {
            let traditional = ["hant", "tw", "hk", "mo"].iter().any(|t| tag.contains(t));
            return if traditional { "zh-TW" } else { "zh-CN" };
        }
        if tag.starts_with("pt") {
            return "pt-BR";
        }
        let base = tag.split('-').next().unwrap_or_default();
        if let Some(code) = locales().find(|code| code.to_ascii_lowercase() == base) {
            return code;
        }
    }
    BASE_LOCALE
}

/// 语言代码对应的英文名，填进发给 Agent 的提示词（例如审阅结论用什么语言写）。
pub fn language_name(code: &str) -> &'static str {
    match code {
        "zh-CN" => "Simplified Chinese",
        "zh-TW" => "Traditional Chinese",
        "ja" => "Japanese",
        "ko" => "Korean",
        "es" => "Spanish",
        "fr" => "French",
        "de" => "German",
        "pt-BR" => "Brazilian Portuguese",
        "ru" => "Russian",
        "it" => "Italian",
        "tr" => "Turkish",
        "vi" => "Vietnamese",
        "id" => "Indonesian",
        _ => "English",
    }
}

/// 当前语言的文案。
pub fn t(key: &str) -> String {
    translate(locale(), key)
}

/// 当前语言的文案，`{name}` 换成 `vars` 里对应的值。
pub fn tf(key: &str, vars: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut text = t(key);
    for (name, value) in vars {
        text = text.replace(&format!("{{{name}}}"), &value.to_string());
    }
    text
}

/// 指定语言的文案，找不到时退回英文，再找不到时返回键名本身。
/// 文案里的快捷键按 macOS 写（⌘N），在 Windows / Linux 上换成那边的写法（Ctrl+Shift+N）。
pub fn translate(locale: &str, key: &str) -> String {
    let all = catalogs();
    let text = all
        .get(locale)
        .and_then(|c| c.get(key))
        .or_else(|| all.get(BASE_LOCALE).and_then(|c| c.get(key)))
        .map_or(key, String::as_str);
    if cfg!(target_os = "macos") {
        text.to_string()
    } else {
        pc_keys(text)
    }
}

/// 把 macOS 的快捷键写法换成 Windows / Linux 的，规则和界面一致（见 app/src/lib/platform.ts）：
/// ⌘ → Ctrl，⌥ → Alt，⇧ → Shift，↵ → Enter；⌘ 加字母（⌘Z 除外）再加 Shift，字母留给终端里的 Agent。
pub fn pc_keys(text: &str) -> String {
    const MODS: [char; 4] = ['⌘', '⌥', '⇧', '⌃'];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(MODS) {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let mods_len: usize = tail.chars().take_while(|c| MODS.contains(c)).map(char::len_utf8).sum();
        let mods = &tail[..mods_len];
        let after = &tail[mods_len..];
        let key = if after.starts_with("Space") {
            "Space"
        } else {
            after
                .chars()
                .next()
                .filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, '↵' | '↑' | '↓'))
                .map_or("", |c| &after[..c.len_utf8()])
        };
        let cmd = mods.contains(['⌘', '⌃']);
        let letter = key.len() == 1 && key.chars().all(|c| c.is_ascii_uppercase());
        let shift = mods.contains('⇧') || (cmd && letter && key != "Z");
        let parts = [
            cmd.then_some("Ctrl"),
            mods.contains('⌥').then_some("Alt"),
            shift.then_some("Shift"),
            (!key.is_empty()).then_some(if key == "↵" { "Enter" } else { key }),
        ];
        out.push_str(&parts.into_iter().flatten().collect::<Vec<_>>().join("+"));
        rest = &after[key.len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_shortcuts_for_windows_and_linux() {
        assert_eq!(pc_keys("press ⌘N in Duolo"), "press Ctrl+Shift+N in Duolo");
        assert_eq!(
            pc_keys("(⌘⇧R) (⌘↵) (⌘Z) (⌘⌥↑) (⌥ drag)"),
            "(Ctrl+Shift+R) (Ctrl+Enter) (Ctrl+Z) (Ctrl+Alt+↑) (Alt drag)"
        );
        assert_eq!(pc_keys("只想收起窗口的话，用 ⌘W。"), "只想收起窗口的话，用 Ctrl+Shift+W。");
        assert_eq!(pc_keys("no shortcuts"), "no shortcuts");
    }

    #[test]
    fn every_locale_has_every_key() {
        let base = &catalogs()[BASE_LOCALE];
        for code in locales() {
            let catalog = &catalogs()[code];
            for key in base.keys() {
                assert!(catalog.contains_key(key), "{code} is missing {key}");
            }
        }
    }

    #[test]
    fn matches_system_tags() {
        assert_eq!(match_locale(["zh-Hans-CN"]), "zh-CN");
        assert_eq!(match_locale(["zh_TW"]), "zh-TW");
        assert_eq!(match_locale(["pt-PT"]), "pt-BR");
        assert_eq!(match_locale(["en-GB"]), "en");
        assert_eq!(match_locale(["xx", "ja-JP"]), "ja");
        assert_eq!(match_locale(["xx"]), "en");
    }

    #[test]
    fn language_names() {
        assert_eq!(language_name("xx"), "English");
        assert_eq!(language_name("zh-CN"), "Simplified Chinese");
    }

    #[test]
    fn translates_with_fallback() {
        assert_eq!(translate("ja", "no.such.key"), "no.such.key");
        assert_ne!(translate("zh-CN", "common.cancel"), translate("en", "common.cancel"));
        let text = translate("en", "sidebar.newTaskIn").replace("{project}", "shop");
        assert!(text.contains("shop"));
    }
}
