//! Official Haru character cards (preset `chat`), embedded from `assets/haru-cards/`.
use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub persona: String,
    pub name: String,
    pub backchannel: String,
    pub relationship: String,
    pub situation: String,
}

#[derive(Deserialize)]
struct CardFile {
    chat: Card,
}

const SOURCES: [(&str, &str); 9] = [
    ("de", include_str!("../assets/haru-cards/de.json")),
    ("en", include_str!("../assets/haru-cards/en.json")),
    ("es", include_str!("../assets/haru-cards/es.json")),
    ("fr", include_str!("../assets/haru-cards/fr.json")),
    ("ja", include_str!("../assets/haru-cards/ja.json")),
    ("ko", include_str!("../assets/haru-cards/ko.json")),
    ("pt", include_str!("../assets/haru-cards/pt.json")),
    ("ru", include_str!("../assets/haru-cards/ru.json")),
    ("zh", include_str!("../assets/haru-cards/zh.json")),
];

fn all() -> &'static HashMap<&'static str, Card> {
    static CARDS: OnceLock<HashMap<&'static str, Card>> = OnceLock::new();
    CARDS.get_or_init(|| {
        SOURCES
            .iter()
            .map(|(lang, json)| {
                let file: CardFile = serde_json::from_str(json).expect("embedded card");
                (*lang, file.chat)
            })
            .collect()
    })
}

/// The official chat card of `language`; German for a language without one.
pub fn official(language: &str) -> &'static Card {
    let cards = all();
    cards.get(language).unwrap_or_else(|| &cards["de"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::LANGUAGES;

    #[test]
    fn every_language_has_a_complete_card() {
        for lang in LANGUAGES {
            let c = official(lang);
            assert!(!c.persona.trim().is_empty(), "{lang} persona");
            assert!(!c.name.trim().is_empty(), "{lang} name");
            assert!(!c.relationship.trim().is_empty(), "{lang} relationship");
            assert!(!c.situation.trim().is_empty(), "{lang} situation");
            assert!(["low", "mid", "high"].contains(&c.backchannel.as_str()), "{lang} backchannel {}", c.backchannel);
            assert!(all().contains_key(lang), "{lang} is embedded itself, not via the fallback");
        }
    }

    #[test]
    fn japanese_card_is_named_haru_in_katakana() {
        assert_eq!(official("ja").name, "ハル");
    }

    #[test]
    fn unknown_language_falls_back_to_german() {
        assert_eq!(official("xx"), official("de"));
    }
}
