//! Settings load/save/validate and dialogue options mapping (Task 1.1).
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cards::{self, Persona};
use crate::error::{ErrorCode, HaruError};
use crate::messages::MsgKey;
use crate::worker::protocol::DialogueOptions;

pub const LANGUAGES: [&str; 9] = ["de", "en", "es", "fr", "ja", "ko", "pt", "ru", "zh"];

/// The name setting of a fresh install; it means "use the name of the card".
pub const DEFAULT_NAME: &str = "Haru";

/// Sampling temperature range of the slider. The worker divides ALL text logits by it, including the
/// silence (PAD/EOP) decision: near 0 Haru never speaks, far above 1 she talks nonstop.
pub const TEMPERATURE_MIN: f64 = 0.3;
pub const TEMPERATURE_MAX: f64 = 1.0;
pub const TEMPERATURE_DEFAULT: f64 = 0.6;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub language: String,            // "de"; one of de,en,es,fr,ja,ko,pt,ru,zh
    pub name: String,                // "Haru"
    pub gender: Option<String>,      // None | "female" | "male"
    pub backchannel: Option<String>, // None | "low" | "mid" | "high"
    pub speak_first: bool,           // true
    pub temperature: f64,            // 0.6 (TEMPERATURE_DEFAULT)
    pub top_k: u32,                  // 25
    pub voice_path: Option<String>,  // None
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub backend: String,             // "auto" | "cuda" | "vulkan" | "metal" | "cpu"
    pub device: String,              // "auto" | device name from `speech devices` (e.g. "Vulkan1")
    pub quality: String,             // "auto" | "low" | "medium" | "high" | "ultra"
    pub subtitles: bool,             // false
    pub show_face: bool,             // true: "Ein Gesicht im Nebel" (Haru's face behind the fluid)
    pub lag_limit_secs: f64,         // 1.0
    pub headphone_hint_seen: bool,   // false: persists the first-start headphone hint (spec 4.7)
    pub echo_cancellation: bool,     // true: remove Haru's voice from the mic (AEC); applies from the next conversation
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: "de".into(),
            name: DEFAULT_NAME.into(),
            gender: None,
            backchannel: None,
            speak_first: true,
            temperature: TEMPERATURE_DEFAULT,
            top_k: 25,
            voice_path: None,
            input_device: None,
            output_device: None,
            backend: "auto".into(),
            device: "auto".into(),
            quality: "auto".into(),
            subtitles: false,
            show_face: true,
            lag_limit_secs: 1.0,
            headphone_hint_seen: false,
            echo_cancellation: true,
        }
    }
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Defaults with the language detected from the operating system (first start, unreadable file).
fn detected_defaults(os_locales: impl FnOnce() -> Vec<String>) -> Settings {
    let language = crate::locale::pick_language(os_locales());
    eprintln!("settings: using detected language {language}");
    Settings { language: language.to_string(), ..Default::default() }
}

/// Missing file gives defaults in the OS language; a corrupt file is renamed to `<path>.bad` and defaults
/// (again in the OS language) are returned.
pub fn load(path: &Path) -> Settings {
    load_with(path, crate::locale::os_locales)
}

/// `load` with an injectable source of OS locales (only asked when a language has to be detected).
pub fn load_with(path: &Path, os_locales: impl FnOnce() -> Vec<String>) -> Settings {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return detected_defaults(os_locales),
        Err(_) => {
            let _ = std::fs::rename(path, sibling(path, ".bad"));
            return detected_defaults(os_locales);
        }
    };
    match serde_json::from_str::<Settings>(&text) {
        Ok(s) => s.sanitized(os_locales),
        Err(_) => {
            let bad = sibling(path, ".bad");
            let _ = std::fs::remove_file(&bad);
            let _ = std::fs::rename(path, &bad);
            detected_defaults(os_locales)
        }
    }
}

/// Validates, writes `<path>.tmp`, then renames over `path`.
pub fn save(path: &Path, s: &Settings) -> Result<(), HaruError> {
    s.validate()?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let json = serde_json::to_string_pretty(s)
        .map_err(|e| HaruError::new(ErrorCode::Settings, MsgKey::SettingsSaveFailed).with_detail(e.to_string()))?;
    let tmp = sibling(path, ".tmp");
    std::fs::write(&tmp, json)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

fn invalid(message: MsgKey, detail: String) -> HaruError {
    HaruError::new(ErrorCode::Settings, message).with_detail(detail)
}

impl Settings {
    /// Pulls values of an old or hand-edited file back into their valid range (never fails loading).
    /// The corrected value is persisted by the next save.
    /// An unsupported language tag is normalized (`DE-at` -> `de`) or replaced by the OS language.
    pub fn sanitized(mut self, os_locales: impl FnOnce() -> Vec<String>) -> Settings {
        if !LANGUAGES.contains(&self.language.as_str()) {
            self.language = crate::locale::match_language(&self.language)
                .unwrap_or_else(|| crate::locale::pick_language(os_locales()))
                .to_string();
        }
        self.temperature = if self.temperature.is_finite() {
            self.temperature.clamp(TEMPERATURE_MIN, TEMPERATURE_MAX)
        } else {
            TEMPERATURE_DEFAULT
        };
        self
    }

    pub fn validate(&self) -> Result<(), HaruError> {
        if !LANGUAGES.contains(&self.language.as_str()) {
            return Err(invalid(MsgKey::LanguageUnsupported, format!("language={}", self.language)));
        }
        if !(TEMPERATURE_MIN..=TEMPERATURE_MAX).contains(&self.temperature) {
            return Err(invalid(MsgKey::TemperatureRange, format!("temperature={}", self.temperature)));
        }
        if self.top_k > 500 {
            return Err(invalid(MsgKey::TopKRange, format!("top_k={}", self.top_k)));
        }
        if !(0.5..=10.0).contains(&self.lag_limit_secs) {
            return Err(invalid(
                MsgKey::LagLimitRange,
                format!("lag_limit_secs={}", self.lag_limit_secs),
            ));
        }
        if !["auto", "cuda", "vulkan", "metal", "cpu"].contains(&self.backend.as_str()) {
            return Err(invalid(MsgKey::BackendUnknown, format!("backend={}", self.backend)));
        }
        if !["auto", "low", "medium", "high", "ultra"].contains(&self.quality.as_str()) {
            return Err(invalid(MsgKey::QualityUnknown, format!("quality={}", self.quality)));
        }
        if let Some(g) = &self.gender {
            if !["female", "male"].contains(&g.as_str()) {
                return Err(invalid(MsgKey::GenderUnknown, format!("gender={g}")));
            }
        }
        if let Some(b) = &self.backchannel {
            if !["low", "mid", "high"].contains(&b.as_str()) {
                return Err(invalid(MsgKey::BackchannelUnknown, format!("backchannel={b}")));
            }
        }
        if let Some(v) = &self.voice_path {
            if !Path::new(v).is_file() {
                return Err(invalid(MsgKey::VoiceFileMissing, format!("voice_path={v}")));
            }
        }
        Ok(())
    }

    /// Options of a dialogue request: the card of `persona` in the language, overridden by the own
    /// name / backchannel. `max_seconds` is set by the session (it depends on free memory).
    pub fn dialogue_options(&self, persona: Persona) -> DialogueOptions {
        let card = cards::card(persona, &self.language);
        let own = self.name.trim();
        // The card name wins while the own name is empty or the untouched default; otherwise the own
        // name replaces the card name inside the card texts.
        let name = if own.is_empty() || own == DEFAULT_NAME { card.name.clone() } else { own.to_string() };
        let fit = |t: &str| if name == card.name { t.to_string() } else { t.replace(&card.name, &name) };
        DialogueOptions {
            language: Some(self.language.clone()),
            name: Some(name.clone()),
            persona: Some(fit(&card.persona)),
            // The daemon always speaks with the male voice; the angel follows the setting.
            gender: if persona == Persona::Daemon { Some("male".to_string()) } else { self.gender.clone() },
            backchannel: Some(self.backchannel.clone().unwrap_or_else(|| card.backchannel.clone())),
            relationship: Some(fit(&card.relationship)),
            situation: Some(fit(&card.situation)),
            speak_first: self.speak_first.then_some(true),
            temperature: Some(self.temperature),
            top_k: (self.top_k != 25).then_some(self.top_k),
            voice: self.voice_path.as_ref().map(|_| "user".to_string()),
            ..Default::default()
        }
    }

    /// Options of a battle's two speakers, the angel's first: the battle cards with `topic`, Haru against Mephisto
    /// whatever the own name, the angel's voice gender from the setting (female without one), the daemon male, the
    /// cards' backchannels, and the sampling as a dialogue sends it. The battle's `max_frames` sets each
    /// conversation's length, so `max_seconds` stays unset.
    pub fn battle_options(&self, topic: &str) -> [DialogueOptions; 2] {
        [Persona::Angel, Persona::Daemon].map(|p| {
            let card = cards::battle_card(p, &self.language).render(topic);
            let gender = match p {
                Persona::Angel => self.gender.clone().unwrap_or(card.gender),
                Persona::Daemon => card.gender,
            };
            DialogueOptions {
                language: Some(self.language.clone()),
                name: Some(card.name),
                persona: Some(card.persona),
                gender: Some(gender),
                backchannel: Some(card.backchannel),
                other_name: Some(card.other_name),
                relationship: Some(card.relationship),
                situation: Some(card.situation),
                voice: self.voice_path.as_ref().map(|_| "user".to_string()),
                temperature: Some(self.temperature),
                top_k: (self.top_k != 25).then_some(self.top_k),
                speak_first: Some(p == Persona::Angel),
                ..Default::default()
            }
        })
    }

    pub fn needs_reload(&self, new: &Settings) -> bool {
        self.backend != new.backend || self.device != new.device || self.voice_path != new.voice_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn opts_json(s: &Settings) -> serde_json::Value {
        serde_json::to_value(s.dialogue_options(Persona::Angel)).unwrap()
    }

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.language, "de");
        assert_eq!(s.name, "Haru");
        assert!(s.gender.is_none() && s.backchannel.is_none() && s.voice_path.is_none());
        assert!(s.speak_first);
        assert_eq!(s.temperature, 0.6);
        assert_eq!(s.top_k, 25);
        assert_eq!((s.backend.as_str(), s.device.as_str(), s.quality.as_str()), ("auto", "auto", "auto"));
        assert!(!s.subtitles);
        assert!(s.show_face);
        assert_eq!(s.lag_limit_secs, 1.0);
        assert!(!s.headphone_hint_seen);
        assert!(s.echo_cancellation);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn missing_file_gives_defaults() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(load_with(&d.path().join("settings.json"), || vec!["de-DE".into()]), Settings::default());
    }

    fn no_os_locales() -> Vec<String> {
        panic!("OS locales must not be queried")
    }

    #[test]
    fn missing_file_detects_os_language() {
        let d = tempfile::tempdir().unwrap();
        let s = load_with(&d.path().join("settings.json"), || vec!["ja-JP".into()]);
        assert_eq!(s, Settings { language: "ja".into(), ..Default::default() });
    }

    #[test]
    fn missing_file_with_unsupported_locale_is_english() {
        let d = tempfile::tempdir().unwrap();
        let s = load_with(&d.path().join("settings.json"), || vec!["nl-NL".into()]);
        assert_eq!(s.language, "en");
    }

    #[test]
    fn stored_language_wins_over_os_locale() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, r#"{"language":"fr"}"#).unwrap();
        assert_eq!(load_with(&p, no_os_locales).language, "fr");
    }

    #[test]
    fn hand_edited_language_tag_is_normalized() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, r#"{"language":"DE-at"}"#).unwrap();
        assert_eq!(load_with(&p, no_os_locales).language, "de");
    }

    #[test]
    fn unsupported_stored_language_is_replaced_by_detected() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, r#"{"language":"xx"}"#).unwrap();
        assert_eq!(load_with(&p, || vec!["es-MX".into()]).language, "es");
        assert!(!d.path().join("settings.json.bad").exists());
    }

    #[test]
    fn corrupt_file_uses_detected_language() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, "{ not json").unwrap();
        assert_eq!(load_with(&p, || vec!["ko-KR".into()]).language, "ko");
        assert!(d.path().join("settings.json.bad").exists());
    }

    #[test]
    fn partial_json_fills_defaults() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, r#"{"language":"en"}"#).unwrap();
        let s = load(&p);
        assert_eq!(s.language, "en");
        assert_eq!(Settings { language: "de".into(), ..s }, Settings::default());
    }

    #[test]
    fn save_then_load_roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        let s = Settings {
            language: "fr".into(),
            gender: Some("female".into()),
            temperature: 0.9,
            top_k: 40,
            subtitles: true,
            ..Default::default()
        };
        save(&p, &s).unwrap();
        assert_eq!(load(&p), s);
    }

    #[test]
    fn save_leaves_no_tmp() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        save(&p, &Settings::default()).unwrap();
        assert!(p.exists());
        assert!(!d.path().join("settings.json.tmp").exists());
    }

    #[test]
    fn corrupt_json_gives_defaults_and_backup() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, "{ not json").unwrap();
        assert_eq!(load_with(&p, || vec!["de-DE".into()]), Settings::default());
        assert!(!p.exists());
        assert_eq!(std::fs::read_to_string(d.path().join("settings.json.bad")).unwrap(), "{ not json");
    }

    #[test]
    fn validate_rejects_unknown_language() {
        let e = Settings { language: "xx".into(), ..Default::default() }.validate().unwrap_err();
        assert_eq!(e.code, ErrorCode::Settings);
    }

    #[test]
    fn validate_temperature_range_is_0_3_to_1_0_inclusive() {
        for bad in [0.29, 1.01, 0.0, 1.5, 2.5, f64::NAN] {
            let e = Settings { temperature: bad, ..Default::default() }.validate().unwrap_err();
            assert_eq!(e.message, MsgKey::TemperatureRange.german());
            assert_eq!(e.key, MsgKey::TemperatureRange);
        }
        for ok in [0.3, 0.6, 1.0] {
            assert!(Settings { temperature: ok, ..Default::default() }.validate().is_ok());
        }
    }

    #[test]
    fn loading_clamps_out_of_range_temperature() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        for (stored, want) in [("1.5", 1.0), ("0", 0.3), ("-3", 0.3), ("0.45", 0.45)] {
            std::fs::write(&p, format!(r#"{{"temperature":{stored}}}"#)).unwrap();
            assert_eq!(load(&p).temperature, want, "stored {stored}");
            assert!(!d.path().join("settings.json.bad").exists());
        }
    }

    #[test]
    fn sanitized_replaces_non_finite_temperature_with_default() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let s = Settings { temperature: bad, ..Default::default() }.sanitized(no_os_locales);
            assert_eq!(s.temperature, 0.6);
        }
    }

    #[test]
    fn dialogue_options_always_send_temperature() {
        assert_eq!(opts_json(&Settings::default())["temperature"], json!(0.6));
    }

    #[test]
    fn validate_rejects_backend_opencl() {
        assert!(Settings { backend: "opencl".into(), ..Default::default() }.validate().is_err());
    }

    #[test]
    fn validate_rejects_lag_limit_0_2() {
        assert!(Settings { lag_limit_secs: 0.2, ..Default::default() }.validate().is_err());
    }

    #[test]
    fn validate_rejects_missing_voice_file() {
        let d = tempfile::tempdir().unwrap();
        let missing = d.path().join("nope.wav").to_string_lossy().into_owned();
        assert!(Settings { voice_path: Some(missing), ..Default::default() }.validate().is_err());
        let present = d.path().join("v.wav");
        std::fs::write(&present, b"x").unwrap();
        assert!(Settings { voice_path: Some(present.to_string_lossy().into_owned()), ..Default::default() }
            .validate()
            .is_ok());
    }

    #[test]
    fn dialogue_options_defaults() {
        let o = Settings::default().dialogue_options(Persona::Angel);
        let card = cards::card(Persona::Angel, "de");
        assert_eq!(o.language.as_deref(), Some("de"));
        assert_eq!(o.name.as_deref(), Some("Haru"));
        assert_eq!(o.persona.as_deref(), Some(card.persona.as_str()));
        assert_eq!(o.backchannel.as_deref(), Some("mid"));
        assert_eq!(o.relationship.as_deref(), Some(card.relationship.as_str()));
        assert_eq!(o.situation.as_deref(), Some(card.situation.as_str()));
        assert_eq!(o.speak_first, Some(true));
        assert_eq!(o.gender, None);
        assert_eq!(o.max_seconds, None);
        assert_eq!(o.other_name, None);
    }

    #[test]
    fn dialogue_options_include_changed_sampling() {
        let v = opts_json(&Settings { temperature: 0.9, top_k: 40, ..Default::default() });
        assert_eq!(v["temperature"], json!(0.9));
        assert_eq!(v["top_k"], json!(40));
    }

    #[test]
    fn dialogue_options_include_setting_fields() {
        let v = opts_json(&Settings {
            gender: Some("male".into()),
            backchannel: Some("high".into()),
            ..Default::default()
        });
        assert_eq!(v["gender"], json!("male"));
        assert_eq!(v["backchannel"], json!("high"));
    }

    #[test]
    fn dialogue_options_empty_name_uses_card_name() {
        let v = opts_json(&Settings { name: "  ".into(), ..Default::default() });
        assert_eq!(v["name"], json!("Haru"));
    }

    #[test]
    fn dialogue_options_custom_name_replaces_card_name_in_texts() {
        let card = cards::card(Persona::Angel, "de");
        let o = Settings { name: "Mika".into(), ..Default::default() }.dialogue_options(Persona::Angel);
        assert_eq!(o.name.as_deref(), Some("Mika"));
        assert_eq!(o.persona.as_deref(), Some(card.persona.replace("Haru", "Mika").as_str()));
        assert_eq!(o.relationship.as_deref(), Some(card.relationship.replace("Haru", "Mika").as_str()));
        assert_eq!(o.situation.as_deref(), Some(card.situation.replace("Haru", "Mika").as_str()));
        assert!(!o.situation.unwrap().contains("Haru"));
    }

    #[test]
    fn dialogue_options_japanese_default_name_becomes_haru_katakana() {
        let o = Settings { language: "ja".into(), ..Default::default() }.dialogue_options(Persona::Angel);
        assert_eq!(o.name.as_deref(), Some("ハル"));
        assert_eq!(o.persona.as_deref(), Some(cards::card(Persona::Angel, "ja").persona.as_str()));
    }

    #[test]
    fn dialogue_options_japanese_custom_name_wins() {
        let o = Settings { language: "ja".into(), name: "Mika".into(), ..Default::default() }.dialogue_options(Persona::Angel);
        assert_eq!(o.name.as_deref(), Some("Mika"));
        assert!(!o.situation.unwrap().contains("ハル"));
    }

    #[test]
    fn dialogue_options_gender_none_is_omitted() {
        let v = opts_json(&Settings::default());
        assert!(v.get("gender").is_none());
    }

    #[test]
    fn dialogue_options_backchannel_setting_overrides_card() {
        let v = opts_json(&Settings { backchannel: Some("low".into()), ..Default::default() });
        assert_eq!(v["backchannel"], json!("low"));
    }

    #[test]
    fn dialogue_options_never_set_max_seconds() {
        for lang in LANGUAGES {
            let v = opts_json(&Settings { language: lang.into(), ..Default::default() });
            assert!(v.get("max_seconds").is_none());
        }
    }

    #[test]
    fn dialogue_options_voice_uses_added_name() {
        let v = opts_json(&Settings { voice_path: Some("C:\\x\\v.wav".into()), ..Default::default() });
        assert_eq!(v["voice"], json!("user"));
    }

    #[test]
    fn battle_options_put_haru_against_mephisto() {
        let [a, d] = Settings::default().battle_options("Ist eine Notlüge erlaubt?");
        assert_eq!((a.language.as_deref(), d.language.as_deref()), (Some("de"), Some("de")));
        assert_eq!((a.name.as_deref(), a.other_name.as_deref()), (Some("Haru"), Some("Mephisto")));
        assert_eq!((d.name.as_deref(), d.other_name.as_deref()), (Some("Mephisto"), Some("Haru")));
        assert_eq!((a.backchannel.as_deref(), d.backchannel.as_deref()), (Some("mid"), Some("low")));
        assert_eq!((a.speak_first, d.speak_first), (Some(true), Some(false)));
        assert_eq!((a.gender.as_deref(), d.gender.as_deref()), (Some("female"), Some("male")));
        assert!(a.situation.as_deref().unwrap().ends_with("„Mephisto, Ist eine Notlüge erlaubt?“"));
        assert!(d.situation.as_deref().unwrap().contains("„Ist eine Notlüge erlaubt?“"));
        assert_eq!(d.persona.as_deref(), Some(cards::battle_card(Persona::Daemon, "de").persona.as_str()));
    }

    #[test]
    fn battle_options_follow_gender_language_and_voice_but_not_the_own_name() {
        let s = Settings { language: "ja".into(), gender: Some("male".into()), name: "Mara".into(), backchannel: Some("high".into()),
            voice_path: Some("C:\\x\\v.wav".into()), ..Default::default() };
        let [a, d] = s.battle_options("x");
        assert_eq!((a.name.as_deref(), d.name.as_deref()), (Some("ハル"), Some("メフィスト")));
        assert_eq!((a.gender.as_deref(), d.gender.as_deref()), (Some("male"), Some("male")));
        assert_eq!((a.backchannel.as_deref(), d.backchannel.as_deref()), (Some("mid"), Some("low")));
        assert_eq!((a.voice.as_deref(), d.voice.as_deref()), (Some("user"), Some("user")));
        let female = Settings { gender: Some("female".into()), ..Default::default() }.battle_options("x");
        assert_eq!((female[0].gender.as_deref(), female[1].gender.as_deref()), (Some("female"), Some("male")));
    }

    #[test]
    fn battle_options_send_sampling_as_today_and_never_max_seconds() {
        for s in [Settings::default(), Settings { temperature: 0.9, top_k: 40, ..Default::default() }] {
            for o in s.battle_options("x") {
                let v = serde_json::to_value(&o).unwrap();
                assert_eq!(v["temperature"], json!(s.temperature));
                assert_eq!(v.get("top_k").cloned(), (s.top_k != 25).then(|| json!(s.top_k)));
                assert!(v.get("max_seconds").is_none() && v.get("seed").is_none() && v.get("sample_rate").is_none(), "{v}");
            }
        }
    }

    #[test]
    fn needs_reload_on_backend_device_voice_only() {
        let a = Settings::default();
        assert!(a.needs_reload(&Settings { backend: "cpu".into(), ..a.clone() }));
        assert!(a.needs_reload(&Settings { device: "Vulkan0".into(), ..a.clone() }));
        assert!(a.needs_reload(&Settings { voice_path: Some("v.wav".into()), ..a.clone() }));
        assert!(!a.needs_reload(&Settings { echo_cancellation: false, ..a.clone() }));
    }

    #[test]
    fn old_settings_without_echo_cancellation_turn_it_on() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, r#"{ "language": "en", "showFace": false }"#).unwrap();
        let s = load(&p);
        assert_eq!(s.language, "en");
        assert!(!s.show_face);
        assert!(s.echo_cancellation);
        assert!(!d.path().join("settings.json.bad").exists());
    }

    #[test]
    fn echo_cancellation_off_roundtrips() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        let s = Settings { echo_cancellation: false, ..Default::default() };
        save(&p, &s).unwrap();
        assert!(std::fs::read_to_string(&p).unwrap().contains("\"echoCancellation\": false"));
        assert!(!load(&p).echo_cancellation);
    }

    #[test]
    fn daemon_options_use_the_daemon_card() {
        let s = Settings::default();
        let o = s.dialogue_options(Persona::Daemon);
        assert_eq!(o.persona.as_deref(), Some(cards::card(Persona::Daemon, "de").persona.as_str()));
        assert_eq!(o.relationship.as_deref(), Some(cards::card(Persona::Daemon, "de").relationship.as_str()));
        assert_eq!(o.backchannel.as_deref(), Some("low"));
    }

    #[test]
    fn daemon_always_gets_the_male_voice_angel_follows_the_setting() {
        for g in [None, Some("female".to_string()), Some("male".to_string())] {
            let s = Settings { gender: g.clone(), ..Settings::default() };
            assert_eq!(s.dialogue_options(Persona::Daemon).gender.as_deref(), Some("male"));
            assert_eq!(s.dialogue_options(Persona::Angel).gender, g);
        }
    }

    #[test]
    fn own_name_replaces_haru_in_the_daemon_card() {
        let s = Settings { name: "Mara".into(), ..Settings::default() };
        let o = s.dialogue_options(Persona::Daemon);
        assert_eq!(o.name.as_deref(), Some("Mara"));
        assert!(o.persona.as_deref().unwrap().contains("Mara") && !o.persona.as_deref().unwrap().contains("Haru"));
    }

    #[test]
    fn old_settings_with_persona_still_load() {
        let s: Settings = serde_json::from_value(json!({ "language": "de", "persona": "alt" })).unwrap();
        assert_eq!(s.language, "de");
    }

    #[test]
    fn backchannel_setting_overrides_the_persona_card() {
        let s = Settings { backchannel: Some("high".into()), ..Settings::default() };
        assert_eq!(s.dialogue_options(Persona::Daemon).backchannel.as_deref(), Some("high"));
    }
}
