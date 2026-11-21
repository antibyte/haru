//! Settings load/save/validate and dialogue options mapping (Task 1.1).
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cards;
use crate::error::{ErrorCode, HaruError};
use crate::worker::protocol::DialogueOptions;

pub const LANGUAGES: [&str; 9] = ["de", "en", "es", "fr", "ja", "ko", "pt", "ru", "zh"];

/// The name setting of a fresh install; it means "use the name of the official card".
pub const DEFAULT_NAME: &str = "Haru";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub language: String,            // "de"; one of de,en,es,fr,ja,ko,pt,ru,zh
    pub name: String,                // "Haru"
    pub persona: String,             // ""
    pub gender: Option<String>,      // None | "female" | "male"
    pub backchannel: Option<String>, // None | "low" | "mid" | "high"
    pub speak_first: bool,           // true
    pub temperature: f64,            // 0.7
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
            persona: String::new(),
            gender: None,
            backchannel: None,
            speak_first: true,
            temperature: 0.7,
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

/// Missing file gives defaults; a corrupt file is renamed to `<path>.bad` and defaults are returned.
pub fn load(path: &Path) -> Settings {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Settings::default(),
        Err(_) => {
            let _ = std::fs::rename(path, sibling(path, ".bad"));
            return Settings::default();
        }
    };
    match serde_json::from_str::<Settings>(&text) {
        Ok(s) => s,
        Err(_) => {
            let bad = sibling(path, ".bad");
            let _ = std::fs::remove_file(&bad);
            let _ = std::fs::rename(path, &bad);
            Settings::default()
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
        .map_err(|e| HaruError::new(ErrorCode::Settings, "Einstellungen konnten nicht gespeichert werden.").with_detail(e.to_string()))?;
    let tmp = sibling(path, ".tmp");
    std::fs::write(&tmp, json)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

fn invalid(message: &str, detail: String) -> HaruError {
    HaruError::new(ErrorCode::Settings, message).with_detail(detail)
}

impl Settings {
    pub fn validate(&self) -> Result<(), HaruError> {
        if !LANGUAGES.contains(&self.language.as_str()) {
            return Err(invalid("Diese Sprache wird nicht unterstützt.", format!("language={}", self.language)));
        }
        if !(0.0..=2.0).contains(&self.temperature) {
            return Err(invalid("Die Temperatur muss zwischen 0 und 2 liegen.", format!("temperature={}", self.temperature)));
        }
        if self.top_k > 500 {
            return Err(invalid("Top-K muss zwischen 0 und 500 liegen.", format!("top_k={}", self.top_k)));
        }
        if !(0.5..=10.0).contains(&self.lag_limit_secs) {
            return Err(invalid(
                "Die Verzögerungsgrenze muss zwischen 0,5 und 10 Sekunden liegen.",
                format!("lag_limit_secs={}", self.lag_limit_secs),
            ));
        }
        if !["auto", "cuda", "vulkan", "metal", "cpu"].contains(&self.backend.as_str()) {
            return Err(invalid("Unbekanntes Backend.", format!("backend={}", self.backend)));
        }
        if !["auto", "low", "medium", "high", "ultra"].contains(&self.quality.as_str()) {
            return Err(invalid("Unbekannte Qualitätsstufe.", format!("quality={}", self.quality)));
        }
        if let Some(g) = &self.gender {
            if !["female", "male"].contains(&g.as_str()) {
                return Err(invalid("Unbekanntes Geschlecht.", format!("gender={g}")));
            }
        }
        if let Some(b) = &self.backchannel {
            if !["low", "mid", "high"].contains(&b.as_str()) {
                return Err(invalid("Unbekannte Rückmeldungsstärke.", format!("backchannel={b}")));
            }
        }
        if let Some(v) = &self.voice_path {
            if !Path::new(v).is_file() {
                return Err(invalid("Die Stimmdatei wurde nicht gefunden.", format!("voice_path={v}")));
            }
        }
        Ok(())
    }

    /// Options of a dialogue request: the official card of the language, overridden by the own
    /// name / persona / backchannel. `max_seconds` is set by the session (it depends on free memory).
    pub fn dialogue_options(&self) -> DialogueOptions {
        let card = cards::official(&self.language);
        let own = self.name.trim();
        // The card name wins while the own name is empty or the untouched default; otherwise the own
        // name replaces the card name inside the card texts.
        let name = if own.is_empty() || own == DEFAULT_NAME { card.name.clone() } else { own.to_string() };
        let fit = |t: &str| if name == card.name { t.to_string() } else { t.replace(&card.name, &name) };
        DialogueOptions {
            language: Some(self.language.clone()),
            name: Some(name.clone()),
            persona: Some(if self.persona.trim().is_empty() { fit(&card.persona) } else { self.persona.clone() }),
            gender: self.gender.clone(),
            backchannel: Some(self.backchannel.clone().unwrap_or_else(|| card.backchannel.clone())),
            relationship: Some(fit(&card.relationship)),
            situation: Some(fit(&card.situation)),
            speak_first: self.speak_first.then_some(true),
            temperature: ((self.temperature - 0.7).abs() > 1e-9).then_some(self.temperature),
            top_k: (self.top_k != 25).then_some(self.top_k),
            voice: self.voice_path.as_ref().map(|_| "user".to_string()),
            ..Default::default()
        }
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
        serde_json::to_value(s.dialogue_options()).unwrap()
    }

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.language, "de");
        assert_eq!(s.name, "Haru");
        assert_eq!(s.persona, "");
        assert!(s.gender.is_none() && s.backchannel.is_none() && s.voice_path.is_none());
        assert!(s.speak_first);
        assert_eq!(s.temperature, 0.7);
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
        assert_eq!(load(&d.path().join("settings.json")), Settings::default());
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
            persona: "freundlich".into(),
            gender: Some("female".into()),
            temperature: 1.1,
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
        assert_eq!(load(&p), Settings::default());
        assert!(!p.exists());
        assert_eq!(std::fs::read_to_string(d.path().join("settings.json.bad")).unwrap(), "{ not json");
    }

    #[test]
    fn validate_rejects_unknown_language() {
        let e = Settings { language: "xx".into(), ..Default::default() }.validate().unwrap_err();
        assert_eq!(e.code, ErrorCode::Settings);
    }

    #[test]
    fn validate_rejects_temperature_2_5() {
        assert!(Settings { temperature: 2.5, ..Default::default() }.validate().is_err());
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
        let o = Settings::default().dialogue_options();
        let card = cards::official("de");
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
    fn dialogue_options_include_card_fields() {
        let v = opts_json(&Settings {
            persona: "ruhig".into(),
            gender: Some("male".into()),
            backchannel: Some("high".into()),
            ..Default::default()
        });
        assert_eq!(v["persona"], json!("ruhig"));
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
        let card = cards::official("de");
        let o = Settings { name: "Mika".into(), ..Default::default() }.dialogue_options();
        assert_eq!(o.name.as_deref(), Some("Mika"));
        assert_eq!(o.persona.as_deref(), Some(card.persona.replace("Haru", "Mika").as_str()));
        assert_eq!(o.relationship.as_deref(), Some(card.relationship.replace("Haru", "Mika").as_str()));
        assert_eq!(o.situation.as_deref(), Some(card.situation.replace("Haru", "Mika").as_str()));
        assert!(!o.situation.unwrap().contains("Haru"));
    }

    #[test]
    fn dialogue_options_japanese_default_name_becomes_haru_katakana() {
        let o = Settings { language: "ja".into(), ..Default::default() }.dialogue_options();
        assert_eq!(o.name.as_deref(), Some("ハル"));
        assert_eq!(o.persona.as_deref(), Some(cards::official("ja").persona.as_str()));
    }

    #[test]
    fn dialogue_options_japanese_custom_name_wins() {
        let o = Settings { language: "ja".into(), name: "Mika".into(), ..Default::default() }.dialogue_options();
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
    fn needs_reload_on_backend_device_voice_only() {
        let a = Settings::default();
        assert!(a.needs_reload(&Settings { backend: "cpu".into(), ..a.clone() }));
        assert!(a.needs_reload(&Settings { device: "Vulkan0".into(), ..a.clone() }));
        assert!(a.needs_reload(&Settings { voice_path: Some("v.wav".into()), ..a.clone() }));
        assert!(!a.needs_reload(&Settings { persona: "x".into(), ..a.clone() }));
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
}
