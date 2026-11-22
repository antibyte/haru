//! Haru character cards (preset `chat`). The app uses the persona cards (angel, daemon) embedded from
//! `assets/persona-cards/<persona>/`; the official cards in `assets/haru-cards/` stay embedded as reference.
use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    pub persona: String,
    pub name: String,
    pub backchannel: String,
    pub relationship: String,
    pub situation: String,
}

/// Which Haru the user chose at the start screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Persona {
    Angel,
    Daemon,
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

const ANGEL: [(&str, &str); 9] = [
    ("de", include_str!("../assets/persona-cards/angel/de.json")),
    ("en", include_str!("../assets/persona-cards/angel/en.json")),
    ("es", include_str!("../assets/persona-cards/angel/es.json")),
    ("fr", include_str!("../assets/persona-cards/angel/fr.json")),
    ("ja", include_str!("../assets/persona-cards/angel/ja.json")),
    ("ko", include_str!("../assets/persona-cards/angel/ko.json")),
    ("pt", include_str!("../assets/persona-cards/angel/pt.json")),
    ("ru", include_str!("../assets/persona-cards/angel/ru.json")),
    ("zh", include_str!("../assets/persona-cards/angel/zh.json")),
];

const DAEMON: [(&str, &str); 9] = [
    ("de", include_str!("../assets/persona-cards/daemon/de.json")),
    ("en", include_str!("../assets/persona-cards/daemon/en.json")),
    ("es", include_str!("../assets/persona-cards/daemon/es.json")),
    ("fr", include_str!("../assets/persona-cards/daemon/fr.json")),
    ("ja", include_str!("../assets/persona-cards/daemon/ja.json")),
    ("ko", include_str!("../assets/persona-cards/daemon/ko.json")),
    ("pt", include_str!("../assets/persona-cards/daemon/pt.json")),
    ("ru", include_str!("../assets/persona-cards/daemon/ru.json")),
    ("zh", include_str!("../assets/persona-cards/daemon/zh.json")),
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

fn parse(sources: &[(&'static str, &str); 9]) -> HashMap<&'static str, Card> {
    sources
        .iter()
        .map(|(lang, json)| {
            let file: CardFile = serde_json::from_str(json).expect("embedded persona card");
            (*lang, file.chat)
        })
        .collect()
}

fn persona_cards(p: Persona) -> &'static HashMap<&'static str, Card> {
    static ANGEL_CARDS: OnceLock<HashMap<&'static str, Card>> = OnceLock::new();
    static DAEMON_CARDS: OnceLock<HashMap<&'static str, Card>> = OnceLock::new();
    match p {
        Persona::Angel => ANGEL_CARDS.get_or_init(|| parse(&ANGEL)),
        Persona::Daemon => DAEMON_CARDS.get_or_init(|| parse(&DAEMON)),
    }
}

/// The card of `persona` in `language`; German for a language without one.
pub fn card(persona: Persona, language: &str) -> &'static Card {
    let cards = persona_cards(persona);
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

    #[test]
    fn every_persona_has_a_complete_card_in_every_language() {
        for p in [Persona::Angel, Persona::Daemon] {
            for lang in LANGUAGES {
                let c = card(p, lang);
                assert!(!c.persona.trim().is_empty() && !c.relationship.trim().is_empty() && !c.situation.trim().is_empty(), "{p:?} {lang}");
                assert_eq!(c.name, official(lang).name, "{p:?} {lang} keeps the official name");
                assert!(c.persona.contains(&c.name), "{p:?} {lang}: the name occurs in the persona (for `fit`)");
                assert!(c.relationship.contains(&c.name) && c.situation.contains(&c.name), "{p:?} {lang}: name in relationship and situation");
                assert!(["low", "mid", "high"].contains(&c.backchannel.as_str()));
                assert!(persona_cards(p).contains_key(lang), "{p:?} {lang} embedded itself");
            }
        }
    }

    #[test]
    fn angel_and_daemon_differ() {
        for lang in LANGUAGES {
            assert_ne!(card(Persona::Angel, lang).persona, card(Persona::Daemon, lang).persona);
        }
        assert_eq!(card(Persona::Daemon, "de").backchannel, "low");
        assert_eq!(card(Persona::Angel, "de").backchannel, "mid");
    }

    #[test]
    fn persona_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Persona::Daemon).unwrap(), "\"daemon\"");
        assert_eq!(serde_json::from_str::<Persona>("\"angel\"").unwrap(), Persona::Angel);
        assert!(serde_json::from_str::<Persona>("\"devil\"").is_err());
    }

    #[test]
    fn unknown_language_falls_back_to_german_persona_card() {
        assert_eq!(card(Persona::Daemon, "xx"), card(Persona::Daemon, "de"));
    }
}
