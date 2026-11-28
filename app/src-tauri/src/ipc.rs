use serde::{Deserialize, Serialize};
use crate::messages::UiText;
use crate::{error::HaruError, settings::Settings};

pub const STATE_EVENT: &str = "haru:state";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppState { Booting, Downloading, Loading, Ready, Talking, Error }

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateEvent {
    pub state: AppState,
    pub backend: Option<String>,  // "cuda" | "vulkan" | "metal" | "cpu"
    pub device: Option<String>,   // device description, e.g. "Intel(R) Arc(TM) B580 Graphics"
    pub error: Option<HaruError>,
    pub warning: Option<UiText>,  // e.g. CPU-is-slow, backend switched after crash
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<UiText>,   // one-off info, e.g. the conversation continued in a fresh segment
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "event", content = "data")]
pub enum DownloadEvent {
    Progress { downloaded: u64, total: u64, bytes_per_sec: f64 },
    Verifying { done: u64, total: u64 },
    Done { path: String },
    Error(UiText),
}

#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct ModelLevels { pub rms: f32, pub bands: [f32; 16] }
#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct MicLevels { pub rms: f32, pub bands: [f32; 8] }
/// Who speaks in a battle; the index in the battle request's `speakers`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Speaker { Angel, Daemon }

impl Speaker {
    pub fn from_index(i: u8) -> Option<Speaker> {
        match i { 0 => Some(Speaker::Angel), 1 => Some(Speaker::Daemon), _ => None }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct WordPiece { pub seq: u64, pub text: String, #[serde(skip_serializing_if = "Option::is_none")] pub speaker: Option<Speaker> }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BattlePhase { Preparing, Playing, Paused }

/// The battle part of a viz frame: each voice as played now, and the pre-buffer.
#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct BattleViz { pub angel: ModelLevels, pub daemon: ModelLevels, pub phase: BattlePhase, pub prepared: f64 }

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VizFrame {
    pub t: f64,               // seconds of Haru audio actually played in this conversation
    pub model: ModelLevels,   // block audible now; zeros when nothing is audible; in a battle the louder voice
    pub mic: MicLevels,       // latest mic chunk; zeros while muted and in a battle
    pub latency_ms: f64,      // backlog per latency policy, in ms; 0 in a battle
    pub words: Vec<WordPiece>,// every frame that became audible since the last VizFrame, seq order, text may be "" or " "
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battle: Option<BattleViz>,
}

#[derive(Clone, Debug, Serialize)] #[serde(rename_all = "camelCase")]
pub struct SaveSettingsResult { pub settings: Settings, pub reload_required: bool }
#[derive(Clone, Debug, Serialize)] #[serde(rename_all = "camelCase")]
pub struct AudioDevices { pub inputs: Vec<String>, pub outputs: Vec<String>, pub default_input: Option<String>, pub default_output: Option<String> }
#[derive(Clone, Debug, Serialize)] #[serde(rename_all = "camelCase")]
pub struct DeviceInfo { pub variant: String, pub name: String, pub description: String, pub kind: String, pub memory_total: u64 }
#[derive(Clone, Debug, Serialize)] #[serde(rename_all = "camelCase")]
pub struct BackendOptions { pub variants: Vec<String>, pub devices: Vec<DeviceInfo>, pub cuda_available: bool, pub backends_dir: Option<String> }
#[derive(Clone, Debug, Serialize)] #[serde(rename_all = "camelCase")]
pub struct AttemptInfo { pub backend: String, pub device: String, pub description: String }
#[derive(Clone, Debug, Serialize)] #[serde(rename_all = "camelCase")]
pub struct BootInfo { pub backend: String, pub device: String, pub device_name: String, pub model_path: String, pub plan: Vec<AttemptInfo>, pub warning: Option<UiText>, pub battle: bool }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::messages::{MsgKey, UiText};
    use serde_json::{json, Value};

    #[test]
    fn contract_fixtures_match() {
        let fx: Value = serde_json::from_str(include_str!("../../src/ipc/contract.fixtures.json")).unwrap();
        let ser = |v: &dyn erased::Ser| v.to_value();

        let state = StateEvent { state: AppState::Talking, backend: Some("vulkan".into()),
            device: Some("Intel(R) Arc(TM) B580 Graphics".into()), error: None, warning: None, notice: None };
        assert_eq!(ser(&state), fx["stateEvent"]);

        let state_err = StateEvent { state: AppState::Error, backend: None, device: None,
            error: Some(HaruError::new(ErrorCode::NoBackend, MsgKey::NoBackend)), warning: None, notice: None };
        assert_eq!(ser(&state_err), fx["stateEventError"]);

        let state_notice = StateEvent { notice: Some(UiText::new(MsgKey::SegmentRenewed)), ..state.clone() };
        assert_eq!(ser(&state_notice), fx["stateEventNotice"]);

        let state_warning = StateEvent { state: AppState::Ready, backend: Some("vulkan".into()),
            device: Some("Intel(R) UHD Graphics".into()), error: None, notice: None,
            warning: Some(UiText::new(MsgKey::SmallGpu).with_param("device", "Intel(R) UHD Graphics")) };
        assert_eq!(ser(&state_warning), fx["stateEventWarning"]);

        let disk = HaruError::new(ErrorCode::DiskSpace, MsgKey::DiskSpace).with_param("need", 7.5).with_param("free", 2.5);
        assert_eq!(ser(&disk), fx["errorDiskSpace"]);

        assert_eq!(ser(&MsgKey::ALL), fx["messageKeys"]);

        let p = DownloadEvent::Progress { downloaded: 1_048_576, total: 6_623_102_720, bytes_per_sec: 52_428_800.0 };
        assert_eq!(ser(&p), fx["downloadProgress"]);
        let v = DownloadEvent::Verifying { done: 1_048_576, total: 6_623_102_720 };
        assert_eq!(ser(&v), fx["downloadVerifying"]);
        let d = DownloadEvent::Done { path: "C:\\models\\Haru-4B-Q8_0.gguf".into() };
        assert_eq!(ser(&d), fx["downloadDone"]);
        let e = DownloadEvent::Error(UiText::new(MsgKey::DownloadFailed));
        assert_eq!(ser(&e), fx["downloadError"]);

        let mut bands = [0.0f32; 16];
        bands[1] = 0.5;
        let viz = VizFrame { t: 1.5, model: ModelLevels { rms: 0.25, bands }, mic: MicLevels { rms: 0.0, bands: [0.0; 8] },
            latency_ms: 312.0, words: vec![WordPiece { seq: 7, text: " Hal".into(), speaker: None }], battle: None };
        assert_eq!(ser(&viz), fx["vizFrame"]);

        let boot = BootInfo { backend: "vulkan".into(), device: "Vulkan0".into(), device_name: "Intel(R) Arc(TM) B580 Graphics".into(),
            model_path: "C:\\models\\Haru-4B-Q8_0.gguf".into(),
            plan: vec![AttemptInfo { backend: "vulkan".into(), device: "Vulkan0".into(), description: "Intel(R) Arc(TM) B580 Graphics".into() }],
            warning: None, battle: true };
        assert_eq!(ser(&boot), fx["bootInfo"]);

        let mut daemon_bands = [0.0f32; 16];
        daemon_bands[1] = 0.5;
        let battle_viz = VizFrame { t: 2.0, model: ModelLevels { rms: 0.25, bands: daemon_bands }, mic: MicLevels { rms: 0.0, bands: [0.0; 8] },
            latency_ms: 0.0, words: vec![WordPiece { seq: 3, text: " nein".into(), speaker: Some(Speaker::Daemon) }],
            battle: Some(BattleViz { angel: ModelLevels { rms: 0.0, bands: [0.0; 16] }, daemon: ModelLevels { rms: 0.25, bands: daemon_bands },
                phase: BattlePhase::Playing, prepared: 1.0 }) };
        assert_eq!(ser(&battle_viz), fx["vizFrameBattle"]);
        assert!(!fx["startBattle"]["topic"].as_str().unwrap().trim().is_empty());

        assert_eq!(ser(&Settings::default()), fx["settingsDefault"]);

        // `start_conversation` takes the persona as a JS argument; the fixture must deserialize into it.
        let persona: crate::cards::Persona = serde_json::from_value(fx["startConversation"]["persona"].clone()).unwrap();
        assert_eq!(persona, crate::cards::Persona::Daemon);
        assert_eq!(serde_json::to_value(persona).unwrap(), fx["startConversation"]["persona"]);
        let _ = json!(null);
    }

    #[test]
    fn speaker_and_phase_serialize_lowercase() {
        assert_eq!(serde_json::to_value(Speaker::Angel).unwrap(), "angel");
        assert_eq!(serde_json::to_value(Speaker::Daemon).unwrap(), "daemon");
        assert_eq!(serde_json::to_value(BattlePhase::Preparing).unwrap(), "preparing");
        assert_eq!(serde_json::to_value(BattlePhase::Paused).unwrap(), "paused");
        assert_eq!((Speaker::from_index(0), Speaker::from_index(1), Speaker::from_index(2)), (Some(Speaker::Angel), Some(Speaker::Daemon), None));
        let w = serde_json::to_value(WordPiece { seq: 1, text: "x".into(), speaker: None }).unwrap();
        assert!(w.get("speaker").is_none());
    }

    mod erased {
        pub trait Ser { fn to_value(&self) -> serde_json::Value; }
        impl<T: serde::Serialize> Ser for T {
            fn to_value(&self) -> serde_json::Value { serde_json::to_value(self).unwrap() }
        }
    }
}
