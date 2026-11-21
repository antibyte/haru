use serde::{Deserialize, Serialize};
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
    pub warning: Option<String>,  // e.g. CPU-is-slow, backend switched after crash
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,   // one-off info, e.g. the conversation continued in a fresh segment
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "event", content = "data")]
pub enum DownloadEvent {
    Progress { downloaded: u64, total: u64, bytes_per_sec: f64 },
    Verifying { done: u64, total: u64 },
    Done { path: String },
    Error { message: String },
}

#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct ModelLevels { pub rms: f32, pub bands: [f32; 16] }
#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct MicLevels { pub rms: f32, pub bands: [f32; 8] }
#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct WordPiece { pub seq: u64, pub text: String }

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VizFrame {
    pub t: f64,               // seconds of Haru audio actually played in this conversation
    pub model: ModelLevels,   // block audible now; zeros when nothing is audible
    pub mic: MicLevels,       // latest mic chunk; zeros while muted
    pub latency_ms: f64,      // backlog per latency policy, in ms
    pub words: Vec<WordPiece>,// every frame that became audible since the last VizFrame, seq order, text may be "" or " "
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
pub struct BootInfo { pub backend: String, pub device: String, pub device_name: String, pub model_path: String, pub plan: Vec<AttemptInfo>, pub warning: Option<String> }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use serde_json::{json, Value};

    #[test]
    fn contract_fixtures_match() {
        let fx: Value = serde_json::from_str(include_str!("../../src/ipc/contract.fixtures.json")).unwrap();
        let ser = |v: &dyn erased::Ser| v.to_value();

        let state = StateEvent { state: AppState::Talking, backend: Some("vulkan".into()),
            device: Some("Intel(R) Arc(TM) B580 Graphics".into()), error: None, warning: None, notice: None };
        assert_eq!(ser(&state), fx["stateEvent"]);

        let state_err = StateEvent { state: AppState::Error, backend: None, device: None,
            error: Some(HaruError::new(ErrorCode::NoBackend, "Keine Backend-Variante gefunden.")), warning: None, notice: None };
        assert_eq!(ser(&state_err), fx["stateEventError"]);

        let state_notice = StateEvent { notice: Some("Neuer Gesprächsabschnitt – Haru fängt frisch an.".into()), ..state.clone() };
        assert_eq!(ser(&state_notice), fx["stateEventNotice"]);

        let p = DownloadEvent::Progress { downloaded: 1_048_576, total: 6_623_102_720, bytes_per_sec: 52_428_800.0 };
        assert_eq!(ser(&p), fx["downloadProgress"]);
        let v = DownloadEvent::Verifying { done: 1_048_576, total: 6_623_102_720 };
        assert_eq!(ser(&v), fx["downloadVerifying"]);
        let d = DownloadEvent::Done { path: "C:\\models\\Haru-4B-Q8_0.gguf".into() };
        assert_eq!(ser(&d), fx["downloadDone"]);
        let e = DownloadEvent::Error { message: "Netzwerkfehler".into() };
        assert_eq!(ser(&e), fx["downloadError"]);

        let mut bands = [0.0f32; 16];
        bands[1] = 0.5;
        let viz = VizFrame { t: 1.5, model: ModelLevels { rms: 0.25, bands }, mic: MicLevels { rms: 0.0, bands: [0.0; 8] },
            latency_ms: 312.0, words: vec![WordPiece { seq: 7, text: " Hal".into() }] };
        assert_eq!(ser(&viz), fx["vizFrame"]);

        assert_eq!(ser(&Settings::default()), fx["settingsDefault"]);
        let _ = json!(null);
    }

    mod erased {
        pub trait Ser { fn to_value(&self) -> serde_json::Value; }
        impl<T: serde::Serialize> Ser for T {
            fn to_value(&self) -> serde_json::Value { serde_json::to_value(self).unwrap() }
        }
    }
}
