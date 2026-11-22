//! Operating-system language detection: maps OS locale tags onto the supported UI/conversation languages.
use crate::settings::LANGUAGES;

pub const FALLBACK_LANGUAGE: &str = "en";

pub fn os_locales() -> Vec<String> {
    sys_locale::get_locales().collect()
}

/// Primary subtag of a locale tag (`pt-BR`, `en_GB.UTF-8`, `de_DE@euro`) if it is a supported language.
pub fn match_language(tag: &str) -> Option<&'static str> {
    let primary = tag.trim().split(['-', '_', '.', '@']).next()?.to_ascii_lowercase();
    LANGUAGES.iter().copied().find(|l| *l == primary)
}

/// First supported language in OS preference order, otherwise English.
pub fn pick_language(locales: impl IntoIterator<Item = impl AsRef<str>>) -> &'static str {
    locales
        .into_iter()
        .flat_map(|l| l.as_ref().split(':').map(str::to_string).collect::<Vec<_>>())
        .find_map(|t| match_language(&t))
        .unwrap_or(FALLBACK_LANGUAGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_language_returns_first_supported_in_order() {
        assert_eq!(pick_language(["fr-CA", "de-AT"]), "fr");
    }

    #[test]
    fn pick_language_reads_primary_subtag_of_any_form() {
        assert_eq!(pick_language(["pt-BR"]), "pt");
        assert_eq!(pick_language(["zh-Hant-TW"]), "zh");
        assert_eq!(pick_language(["en_GB.UTF-8"]), "en");
        assert_eq!(pick_language(["de_DE@euro"]), "de");
        assert_eq!(pick_language(["JA"]), "ja");
    }

    #[test]
    fn pick_language_skips_unsupported_and_posix() {
        assert_eq!(pick_language(["C", "nl-NL", "ko-KR"]), "ko");
    }

    #[test]
    fn pick_language_falls_back_to_english() {
        assert_eq!(pick_language(Vec::<String>::new()), "en");
        assert_eq!(pick_language(["nl"]), "en");
    }

    #[test]
    fn pick_language_splits_colon_lists() {
        assert_eq!(pick_language(["nl:de"]), "de");
    }
}
