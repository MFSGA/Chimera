use language_tags::LanguageTag;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
pub enum I18nLanguage {
    #[serde(rename = "en", alias = "en-US")]
    English,
    #[serde(rename = "ja")]
    Japanese,
    #[serde(rename = "ko")]
    Korean,
    #[serde(rename = "ru")]
    Russian,
    #[serde(rename = "zh-cn", alias = "zh-CN")]
    SimplifiedChinese,
    #[serde(rename = "zh-tw", alias = "zh-TW")]
    TraditionalChinese,
}

impl I18nLanguage {
    /// The canonical i18n key. Identical to the serialized representation.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Japanese => "ja",
            Self::Korean => "ko",
            Self::Russian => "ru",
            Self::SimplifiedChinese => "zh-cn",
            Self::TraditionalChinese => "zh-tw",
        }
    }
}

pub fn default_i18n_language() -> I18nLanguage {
    let locale = sys_locale::get_locale().unwrap_or_else(|| "en-US".into());
    language_for_locale(&locale)
}

fn language_for_locale(locale: &str) -> I18nLanguage {
    let normalized = locale.replace('_', "-");
    let Ok(tag) = LanguageTag::parse(&normalized) else {
        return I18nLanguage::English;
    };
    if is_korean(&tag) {
        I18nLanguage::Korean
    } else if is_japanese(&tag) {
        I18nLanguage::Japanese
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

fn is_japanese(lang: &LanguageTag) -> bool {
    lang.primary_language().eq_ignore_ascii_case("ja")
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

    #[test]
    fn selects_japanese_for_system_locale_values() {
        assert_eq!(language_for_locale("ja"), I18nLanguage::Japanese);
        assert_eq!(language_for_locale("ja-JP"), I18nLanguage::Japanese);
        assert_eq!(language_for_locale("ja_JP"), I18nLanguage::Japanese);
        assert_eq!(
            language_for_locale("zh-CN"),
            I18nLanguage::SimplifiedChinese
        );
    }

    #[test]
    fn locale_keys_match_supported_frontend_and_rust_bundles() {
        assert_eq!(I18nLanguage::English.as_str(), "en");
        assert_eq!(I18nLanguage::Japanese.as_str(), "ja");
        assert_eq!(I18nLanguage::Korean.as_str(), "ko");
        assert_eq!(I18nLanguage::Russian.as_str(), "ru");
        assert_eq!(I18nLanguage::SimplifiedChinese.as_str(), "zh-cn");
        assert_eq!(I18nLanguage::TraditionalChinese.as_str(), "zh-tw");
    }

    #[test]
    fn serializes_canonical_keys_and_reads_legacy_config_values() {
        let languages = [
            I18nLanguage::English,
            I18nLanguage::Japanese,
            I18nLanguage::Korean,
            I18nLanguage::Russian,
            I18nLanguage::SimplifiedChinese,
            I18nLanguage::TraditionalChinese,
        ];

        for language in languages {
            assert_eq!(
                serde_json::to_string(&language).unwrap(),
                format!("\"{}\"", language.as_str())
            );
        }

        assert_eq!(
            serde_json::from_str::<I18nLanguage>("\"en-US\"").unwrap(),
            I18nLanguage::English
        );
        assert_eq!(
            serde_json::from_str::<I18nLanguage>("\"zh-CN\"").unwrap(),
            I18nLanguage::SimplifiedChinese
        );
        assert_eq!(
            serde_json::from_str::<I18nLanguage>("\"zh-TW\"").unwrap(),
            I18nLanguage::TraditionalChinese
        );
    }

    #[test]
    fn downgrade_recovery_resets_language_to_a_value_the_previous_release_reads() {
        #[derive(Deserialize, PartialEq, Debug)]
        enum PreviousI18nLanguage {
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

        #[derive(Deserialize)]
        struct PreviousApplicationConfig {
            language: PreviousI18nLanguage,
        }

        for language in ["en", "ja", "zh-cn", "zh-tw"] {
            let config = format!("language: {language}\n");
            assert!(
                serde_yaml_ng::from_str::<PreviousApplicationConfig>(&config).is_err(),
                "the previous release must reject the new canonical value {language}"
            );
        }

        let recovery = serde_yaml_ng::from_str::<PreviousApplicationConfig>("language: en-US\n")
            .expect("the documented downgrade value must remain readable by the previous release");
        assert_eq!(recovery.language, PreviousI18nLanguage::English);
    }
}
