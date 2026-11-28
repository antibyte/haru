//! Haru character cards (preset `chat`). The app uses the persona cards (angel, daemon) embedded from
//! `assets/persona-cards/<persona>/`; the official cards in `assets/haru-cards/` stay embedded as reference.
//! Battle cards (angel and daemon facing each other on a topic) come from `assets/persona-cards/battle/<persona>/`.
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

/// One speaker of a battle: the persona card for facing the other on a topic, `{topic}` in its situation.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BattleCard {
    pub persona: String,
    pub name: String,
    pub other_name: String,
    pub gender: String,
    pub backchannel: String,
    pub relationship: String,
    pub situation: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BattleFile {
    battle: BattleCard,
}

const BATTLE_ANGEL: [(&str, &str); 9] = [
    ("de", include_str!("../assets/persona-cards/battle/angel/de.json")),
    ("en", include_str!("../assets/persona-cards/battle/angel/en.json")),
    ("es", include_str!("../assets/persona-cards/battle/angel/es.json")),
    ("fr", include_str!("../assets/persona-cards/battle/angel/fr.json")),
    ("ja", include_str!("../assets/persona-cards/battle/angel/ja.json")),
    ("ko", include_str!("../assets/persona-cards/battle/angel/ko.json")),
    ("pt", include_str!("../assets/persona-cards/battle/angel/pt.json")),
    ("ru", include_str!("../assets/persona-cards/battle/angel/ru.json")),
    ("zh", include_str!("../assets/persona-cards/battle/angel/zh.json")),
];

const BATTLE_DAEMON: [(&str, &str); 9] = [
    ("de", include_str!("../assets/persona-cards/battle/daemon/de.json")),
    ("en", include_str!("../assets/persona-cards/battle/daemon/en.json")),
    ("es", include_str!("../assets/persona-cards/battle/daemon/es.json")),
    ("fr", include_str!("../assets/persona-cards/battle/daemon/fr.json")),
    ("ja", include_str!("../assets/persona-cards/battle/daemon/ja.json")),
    ("ko", include_str!("../assets/persona-cards/battle/daemon/ko.json")),
    ("pt", include_str!("../assets/persona-cards/battle/daemon/pt.json")),
    ("ru", include_str!("../assets/persona-cards/battle/daemon/ru.json")),
    ("zh", include_str!("../assets/persona-cards/battle/daemon/zh.json")),
];

/// The placeholder of the topic in a battle card.
pub const TOPIC_PLACEHOLDER: &str = "{topic}";
/// The longest topic a battle takes, in code points.
pub const TOPIC_MAX_CHARS: usize = 120;

fn battle_cards(p: Persona) -> &'static HashMap<&'static str, BattleCard> {
    static ANGEL_BATTLE: OnceLock<HashMap<&'static str, BattleCard>> = OnceLock::new();
    static DAEMON_BATTLE: OnceLock<HashMap<&'static str, BattleCard>> = OnceLock::new();
    let parse = |sources: &[(&'static str, &str); 9]| {
        sources
            .iter()
            .map(|(lang, json)| (*lang, serde_json::from_str::<BattleFile>(json).expect("embedded battle card").battle))
            .collect()
    };
    match p {
        Persona::Angel => ANGEL_BATTLE.get_or_init(|| parse(&BATTLE_ANGEL)),
        Persona::Daemon => DAEMON_BATTLE.get_or_init(|| parse(&BATTLE_DAEMON)),
    }
}

/// The battle card of `persona` in `language`; German for a language without one.
pub fn battle_card(persona: Persona, language: &str) -> &'static BattleCard {
    let cards = battle_cards(persona);
    cards.get(language).unwrap_or_else(|| &cards["de"])
}

impl BattleCard {
    /// The card with every `{topic}` replaced by `topic`.
    pub fn render(&self, topic: &str) -> BattleCard {
        let fill = |t: &str| t.replace(TOPIC_PLACEHOLDER, topic);
        BattleCard {
            persona: fill(&self.persona),
            relationship: fill(&self.relationship),
            situation: fill(&self.situation),
            ..self.clone()
        }
    }
}

/// The topic as a battle takes it: trimmed and cut to 120 code points; `None` when nothing is left.
pub fn battle_topic(raw: &str) -> Option<String> {
    let t: String = raw.trim().chars().take(TOPIC_MAX_CHARS).collect();
    (!t.is_empty()).then_some(t)
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

    #[test]
    fn every_language_has_both_battle_cards() {
        for lang in LANGUAGES {
            let a = battle_card(Persona::Angel, lang);
            let d = battle_card(Persona::Daemon, lang);
            assert!(battle_cards(Persona::Angel).contains_key(lang) && battle_cards(Persona::Daemon).contains_key(lang), "{lang} embedded itself");
            assert_eq!(a.name, official(lang).name, "{lang}: the angel is Haru as the chat cards write it");
            assert_eq!(d.name, if lang == "ja" { "メフィスト" } else { "Mephisto" }, "{lang}");
            assert_eq!((a.other_name.as_str(), d.other_name.as_str()), (d.name.as_str(), a.name.as_str()), "{lang}");
            assert_eq!((a.gender.as_str(), d.gender.as_str()), ("female", "male"), "{lang}");
            assert_eq!((a.backchannel.as_str(), d.backchannel.as_str()), ("mid", "low"), "{lang}");
            assert_eq!(a.situation.matches("{topic}").count(), 2, "{lang}: the question and the opening sentence");
            assert_eq!(d.situation.matches("{topic}").count(), 1, "{lang}");
            assert!(a.situation.starts_with(&d.situation), "{lang}: one shared situation, the angel's adds the opening");
            assert!([", ", "、", "，"].iter().any(|c| a.situation.contains(&format!("{}{c}{{topic}}", a.other_name))), "{lang}: the opening addresses Mephisto");
            for t in [&a.persona, &a.relationship, &d.persona, &d.relationship] {
                assert!(!t.contains('{') && !t.trim().is_empty(), "{lang}");
            }
            assert!(a.persona.contains(&a.name) && d.persona.contains(&d.name), "{lang}");
        }
    }

    #[test]
    fn every_battle_card_is_shorter_than_the_chat_card_of_its_persona() {
        let len = |p: &str, r: &str, s: &str| [p, r, s].iter().map(|t| t.chars().count()).sum::<usize>();
        for lang in LANGUAGES {
            for p in [Persona::Angel, Persona::Daemon] {
                let (b, c) = (battle_card(p, lang), card(p, lang));
                let (bl, cl) = (len(&b.persona, &b.relationship, &b.situation), len(&c.persona, &c.relationship, &c.situation));
                assert!(bl < cl, "{p:?} {lang}: battle card {bl} code points, chat card {cl}");
            }
        }
    }

    #[test]
    fn the_daemon_keeps_the_chat_guardrails_with_only_the_name_replaced() {
        for lang in LANGUAGES {
            let (chat, battle) = (card(Persona::Daemon, lang), battle_card(Persona::Daemon, lang));
            let (_, chat_rails) = chat.persona.split_once("\n\n").expect("chat daemon: description and guardrails");
            let (_, rails) = battle.persona.split_once("\n\n").expect("battle daemon: description and guardrails");
            assert_eq!(rails, chat_rails.replace(&chat.name, &battle.name), "{lang}");
        }
    }

    #[test]
    fn render_fills_every_topic_placeholder() {
        for lang in LANGUAGES {
            for p in [Persona::Angel, Persona::Daemon] {
                let c = battle_card(p, lang).render("Darf man lügen?");
                assert!([&c.persona, &c.relationship, &c.situation].iter().all(|t| !t.contains("{topic}")), "{p:?} {lang}");
                assert!(c.situation.contains("Darf man lügen?"), "{p:?} {lang}");
            }
        }
        let a = battle_card(Persona::Angel, "de").render("Ist eine Notlüge erlaubt?");
        assert!(a.situation.ends_with("„Mephisto, Ist eine Notlüge erlaubt?“"));
    }

    #[test]
    fn battle_topic_is_trimmed_and_cut_to_120_code_points() {
        assert_eq!(battle_topic("  Ist eine Notlüge erlaubt?\n").as_deref(), Some("Ist eine Notlüge erlaubt?"));
        assert_eq!(battle_topic(" \t "), None);
        assert_eq!(battle_topic(&"ä".repeat(130)).unwrap().chars().count(), 120);
        assert_eq!(battle_topic(&"あ".repeat(121)).unwrap(), "あ".repeat(120));
    }

    #[test]
    fn unknown_language_falls_back_to_the_german_battle_cards() {
        assert_eq!(battle_card(Persona::Angel, "xx"), battle_card(Persona::Angel, "de"));
        assert_eq!(battle_card(Persona::Daemon, "xx"), battle_card(Persona::Daemon, "de"));
    }}
