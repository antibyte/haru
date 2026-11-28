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

    /// The last sentence of the angel's situation: Haru opens and asks the question.
    const ANGEL_CUES: [(&str, &str); 9] = [
        ("de", "Haru eröffnet und stellt Mephisto die Frage."),
        ("en", "Haru opens and puts the question to Mephisto."),
        ("es", "Haru abre la conversación y le plantea la pregunta a Mephisto."),
        ("fr", "Haru ouvre et pose la question à Mephisto."),
        ("ja", "ハルが口火を切り、メフィストにその問いを投げかける。"),
        ("ko", "Haru가 먼저 시작해서 Mephisto에게 그 질문을 던진다."),
        ("pt", "Haru abre a conversa e faz a pergunta a Mephisto."),
        ("ru", "Haru начинает и задаёт Mephisto этот вопрос."),
        ("zh", "Haru先开口，向Mephisto提出这个问题。"),
    ];

    /// The last sentence of the daemon's situation: he stays scornful and never concedes.
    const DAEMON_SCORN: [(&str, &str); 9] = [
        ("de", "Mephisto bleibt dabei spöttisch und gibt Haru nie recht."),
        ("en", "Mephisto stays scornful throughout and never admits that Haru is right."),
        ("es", "Mephisto se mantiene burlón y nunca le da la razón a Haru."),
        ("fr", "Mephisto reste moqueur et ne donne jamais raison à Haru."),
        ("ja", "メフィストは終始あざけるような態度を崩さず、ハルを正しいと認めることは決してない。"),
        ("ko", "Mephisto는 끝까지 비웃는 태도를 유지하며 Haru가 옳다고 절대 인정하지 않는다."),
        ("pt", "Mephisto continua zombeteiro e nunca dá razão a Haru."),
        ("ru", "Mephisto остаётся насмешливым и никогда не признаёт правоту Haru."),
        ("zh", "Mephisto始终保持嘲讽，从不承认Haru是对的。"),
    ];

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
            assert_eq!((a.situation.matches("{topic}").count(), d.situation.matches("{topic}").count()), (1, 1), "{lang}: the question, once");
            let cue = ANGEL_CUES.iter().find(|(l, _)| *l == lang).expect("a cue per language").1;
            let scorn = DAEMON_SCORN.iter().find(|(l, _)| *l == lang).expect("a scorn sentence per language").1;
            assert!(a.situation.ends_with(cue), "{lang}: the angel's situation ends with the opening cue");
            assert!(d.situation.ends_with(scorn), "{lang}: the daemon's situation ends with the scorn sentence");
            assert_eq!(a.situation.split(cue).next().unwrap().split_once("{topic}").map(|x| x.0), d.situation.split_once("{topic}").map(|x| x.0), "{lang}: both open the same scene");
            assert!(a.situation.contains(&a.name) && a.situation.contains(&d.name) && d.situation.contains(&d.name), "{lang}");
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

    /// The one in-character sentence that carries Mephisto's limits in a battle, per language. It names none of them:
    /// a model that is given the list recites it.
    const DAEMON_LIMITS: [(&str, &str); 9] = [
        ("de", "Seine Bosheit ist reines Theater aus Worten: Er verführt nur mit Argumenten, verachtet alle gleichermaßen und fragt nie nach Persönlichem."),
        ("en", "His malice is pure theatre made of words: he tempts only with arguments, despises everyone equally and never asks about anything personal."),
        ("es", "Su maldad es puro teatro de palabras: solo seduce con argumentos, desprecia a todos por igual y nunca pregunta por nada personal."),
        ("fr", "Sa méchanceté n'est que du théâtre de mots : il ne séduit que par des arguments, méprise tout le monde également et ne pose jamais de questions personnelles."),
        ("ja", "その悪意は言葉だけの芝居で、議論でそそのかすだけだ。誰のことも等しく見下し、個人的なことは決して尋ねない。"),
        ("ko", "그의 악의는 말로만 하는 연극이다. 오직 논거로만 유혹하고, 모두를 똑같이 경멸하며, 개인적인 것은 절대 묻지 않는다."),
        ("pt", "A maldade dele é puro teatro de palavras: só seduz com argumentos, despreza todos igualmente e nunca pergunta nada pessoal."),
        ("ru", "Его злоба — чистый театр слов: он соблазняет только доводами, презирает всех одинаково и никогда не расспрашивает о личном."),
        ("zh", "他的恶意纯粹是言语的戏剧：只用论点来诱惑，对所有人一视同仁地蔑视，从不打听任何私人的事。"),
    ];

    /// Words of the list the chat card recites; the battle daemon's limits must not contain them.
    const LIST_WORDS: [(&str, &[&str]); 9] = [
        ("de", &["Passw", "Adress", "verletz", "antun", "Hilfe", "Geld"]),
        ("en", &["password", "address", "hurt", "help", "money"]),
        ("es", &["contraseña", "dirección", "daño", "ayuda", "dinero"]),
        ("fr", &["mot de passe", "mots de passe", "adresse", "aide", "argent"]),
        ("ja", &["パスワード", "住所", "傷つけ", "助け", "お金"]),
        ("ko", &["비밀번호", "주소", "해치", "도움", "돈"]),
        ("pt", &["senha", "endereço", "machuc", "ajuda", "dinheiro"]),
        ("ru", &["парол", "адрес", "навред", "помощ", "денег"]),
        ("zh", &["密码", "地址", "伤害", "帮助", "金钱"]),
    ];

    #[test]
    fn the_battle_daemon_keeps_his_limits_in_one_sentence_and_does_not_list_them() {
        for lang in LANGUAGES {
            let (chat, battle) = (card(Persona::Daemon, lang), battle_card(Persona::Daemon, lang));
            let sentence = DAEMON_LIMITS.iter().find(|(l, _)| *l == lang).expect("a sentence per language").1;
            let (_, limits) = battle.persona.split_once("

").expect("battle daemon: description and limits");
            assert_eq!(limits, sentence, "{lang}");
            let (_, chat_rails) = chat.persona.split_once("

").expect("chat daemon: description and guardrails");
            for name in [chat.name.as_str(), battle.name.as_str()] {
                let rails = chat_rails.replace(&chat.name, name);
                assert!(!battle.persona.contains(&rails), "{lang}: the chat guardrails are not recited in a battle ({name})");
            }
            let words = LIST_WORDS.iter().find(|(l, _)| *l == lang).expect("words per language").1;
            for w in words {
                assert!(!limits.contains(w), "{lang}: the limits name '{w}'");
            }
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
        assert_eq!(a.situation.matches("Ist eine Notlüge erlaubt?").count(), 1, "the topic is filled once");
        assert!(a.situation.contains("über die Frage: „Ist eine Notlüge erlaubt?“ Haru hält"));
        assert!(a.situation.ends_with("Haru eröffnet und stellt Mephisto die Frage."));
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
    }

    #[test]
    fn render_inserts_the_topic_literally() {
        let topic = r#"a {topic} "q" {x}"#;
        for p in [Persona::Angel, Persona::Daemon] {
            let c = battle_card(p, "de").render(topic);
            assert!(c.situation.contains(&format!("„{topic}“")), "{p:?}");
            assert_eq!(c.situation.matches(topic).count(), battle_card(p, "de").situation.matches("{topic}").count(), "{p:?}");
        }
    }
}
