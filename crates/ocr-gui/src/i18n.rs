//! Interface language for the GUI.
//!
//! The module only defines the language set, locale detection and plural
//! selection. The translated strings themselves live at their call sites and
//! are filled in by the UI work that follows this scaffold.

/// A language offered by the interface language switcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Ru,
    Zh,
}

impl Lang {
    /// Every language, in the order the switcher cycles through them.
    pub const ALL: [Lang; 3] = [Lang::En, Lang::Ru, Lang::Zh];

    /// Stable identifier used in preferences: `en`, `ru` or `zh`.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
            Lang::Zh => "zh",
        }
    }

    /// Parse a stored code. Unknown or malformed values return `None`.
    pub fn from_code(code: &str) -> Option<Self> {
        match code.trim().to_ascii_lowercase().as_str() {
            "en" => Some(Lang::En),
            "ru" => Some(Lang::Ru),
            "zh" => Some(Lang::Zh),
            _ => None,
        }
    }

    /// Pick a language from an OS locale tag (`zh-Hans`, `ru-RU`, `en-US`, …).
    ///
    /// A missing locale keeps the historical Russian default; an unknown but
    /// present locale falls back to English.
    pub fn from_locale(locale: Option<&str>) -> Self {
        let Some(locale) = locale else {
            return Lang::Ru;
        };
        let tag = locale.trim().to_ascii_lowercase();
        if tag.starts_with("zh") {
            Lang::Zh
        } else if tag.starts_with("ru") {
            Lang::Ru
        } else if tag.starts_with("en") {
            Lang::En
        } else {
            Lang::En
        }
    }

    /// Detect the interface language from the operating system.
    pub fn detect() -> Self {
        Self::from_locale(sys_locale::get_locale().as_deref())
    }

    /// Short caption for a language button or menu item.
    pub fn label(self) -> &'static str {
        match self {
            Lang::En => "EN",
            Lang::Ru => "RU",
            Lang::Zh => "中文",
        }
    }

    /// Full language name shown next to the short label.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ru => "Русский",
            Lang::Zh => "中文",
        }
    }

    /// The next language in the `En -> Ru -> Zh -> En` cycle.
    pub fn next(self) -> Self {
        match self {
            Lang::En => Lang::Ru,
            Lang::Ru => Lang::Zh,
            Lang::Zh => Lang::En,
        }
    }

    /// Localized window title.
    pub fn title(self) -> &'static str {
        match self {
            Lang::En => "ocr-tool — document recognition",
            Lang::Ru => "ocr-tool — распознавание документов",
            Lang::Zh => "ocr-tool — 文档识别",
        }
    }
}

/// Select a plural form for `n`.
///
/// `ru` holds the one/few/many forms, `en` the one/many forms and `zh` the
/// single Chinese form.
pub fn plural(
    lang: Lang,
    n: u64,
    ru: [&'static str; 3],
    en: [&'static str; 2],
    zh: &'static str,
) -> &'static str {
    match lang {
        Lang::Ru => {
            let last_two = n % 100;
            let last = n % 10;
            if last == 1 && last_two != 11 {
                ru[0]
            } else if (2..=4).contains(&last) && !(12..=14).contains(&last_two) {
                ru[1]
            } else {
                ru[2]
            }
        }
        Lang::En => {
            if n == 1 {
                en[0]
            } else {
                en[1]
            }
        }
        Lang::Zh => zh,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_roundtrip_and_reject_unknown_values() {
        for lang in Lang::ALL {
            assert_eq!(Lang::from_code(lang.code()), Some(lang));
        }
        assert_eq!(Lang::from_code(" EN "), Some(Lang::En));
        assert_eq!(Lang::from_code("de"), None);
        assert_eq!(Lang::from_code(""), None);
        assert_eq!(Lang::from_code("english"), None);
    }

    #[test]
    fn locales_map_to_the_supported_languages() {
        assert_eq!(Lang::from_locale(Some("zh-Hans")), Lang::Zh);
        assert_eq!(Lang::from_locale(Some("ru-RU")), Lang::Ru);
        assert_eq!(Lang::from_locale(Some("en-US")), Lang::En);
        assert_eq!(
            Lang::from_locale(None),
            Lang::Ru,
            "no locale keeps the historical Russian default"
        );
        assert_eq!(
            Lang::from_locale(Some("de-DE")),
            Lang::En,
            "an unknown locale falls back to English"
        );
    }

    #[test]
    fn plurals_follow_each_language_rules() {
        let ru = ["файл", "файла", "файлов"];
        let en = ["file", "files"];
        assert_eq!(plural(Lang::Ru, 1, ru, en, "文件"), "файл");
        assert_eq!(plural(Lang::Ru, 2, ru, en, "文件"), "файла");
        assert_eq!(plural(Lang::Ru, 5, ru, en, "文件"), "файлов");
        assert_eq!(plural(Lang::Ru, 11, ru, en, "文件"), "файлов");
        assert_eq!(plural(Lang::Ru, 21, ru, en, "文件"), "файл");

        assert_eq!(plural(Lang::En, 1, ru, en, "文件"), "file");
        assert_eq!(plural(Lang::En, 2, ru, en, "文件"), "files");
        assert_eq!(plural(Lang::En, 5, ru, en, "文件"), "files");

        assert_eq!(plural(Lang::Zh, 1, ru, en, "文件"), "文件");
        assert_eq!(plural(Lang::Zh, 2, ru, en, "文件"), "文件");
        assert_eq!(plural(Lang::Zh, 5, ru, en, "文件"), "文件");
    }

    #[test]
    fn language_cycle_and_titles_cover_all_languages() {
        assert_eq!(Lang::En.next(), Lang::Ru);
        assert_eq!(Lang::Ru.next(), Lang::Zh);
        assert_eq!(Lang::Zh.next(), Lang::En);
        assert_eq!(Lang::En.title(), "ocr-tool — document recognition");
        assert_eq!(Lang::Ru.title(), "ocr-tool — распознавание документов");
        assert_eq!(Lang::Zh.title(), "ocr-tool — 文档识别");
    }
}
