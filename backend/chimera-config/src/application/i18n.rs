use language_tags::LanguageTag;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
pub enum I18nLanguage {
    #[serde(rename = "en-US")]
    English,
    #[serde(rename = "ko")]
    Korean,
    #[serde(rename = "ru")]
    Russian,
    #[serde(rename = "zh-CN")]
    SimplifiedChinese,
    #[serde(rename = "zh-TW")]
    TraditionalChinese,
}

pub fn default_i18n_language() -> I18nLanguage {
    let locale = sys_locale::get_locale().unwrap_or_else(|| "en-US".into());
    let normalized = locale.replace('_', "-");
    let Ok(tag) = LanguageTag::parse(&normalized) else {
        return I18nLanguage::English;
    };
    if is_korean(&tag) {
        I18nLanguage::Korean
    } else if is_russian(&tag) {
        I18nLanguage::Russian
    } else if is_simplified_chinese(&tag) {
        I18nLanguage::SimplifiedChinese
    } else if is_traditional_chinese(&tag) {
        I18nLanguage::TraditionalChinese
    } else {
        I18nLanguage::English
    }
}

fn is_korean(lang: &LanguageTag) -> bool {
    lang.primary_language().eq_ignore_ascii_case("ko")
}

fn is_russian(lang: &LanguageTag) -> bool {
    lang.primary_language().eq_ignore_ascii_case("ru")
}

fn is_simplified_chinese(lang: &LanguageTag) -> bool {
    if !lang.primary_language().eq_ignore_ascii_case("zh") {
        return false;
    }
    match lang.script() {
        Some(script) if script.eq_ignore_ascii_case("Hans") => return true,
        Some(script) if script.eq_ignore_ascii_case("Hant") => return false,
        _ => {}
    }
    match lang.region() {
        Some(region) => !matches!(region.to_ascii_uppercase().as_str(), "TW" | "HK" | "MO"),
        None => true,
    }
}

fn is_traditional_chinese(lang: &LanguageTag) -> bool {
    if !lang.primary_language().eq_ignore_ascii_case("zh") {
        return false;
    }
    match lang.script() {
        Some(script) if script.eq_ignore_ascii_case("Hant") => return true,
        Some(script) if script.eq_ignore_ascii_case("Hans") => return false,
        _ => {}
    }
    match lang.region() {
        Some(region) => matches!(region.to_ascii_uppercase().as_str(), "TW" | "HK" | "MO"),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(value: &str) -> LanguageTag {
        LanguageTag::parse(value).unwrap()
    }

    #[test]
    fn distinguishes_chinese_scripts_and_regions() {
        assert!(is_simplified_chinese(&tag("zh-CN")));
        assert!(is_simplified_chinese(&tag("zh-Hans")));
        assert!(is_traditional_chinese(&tag("zh-TW")));
        assert!(is_traditional_chinese(&tag("zh-Hant")));
    }
}
