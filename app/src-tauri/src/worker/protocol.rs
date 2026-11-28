//! speech.cpp worker JSON Lines protocol (v4).
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::error::{ErrorCode, HaruError};
use crate::messages::MsgKey;

pub const PROTOCOL: u32 = 4;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DialogueOptions {
    #[serde(skip_serializing_if = "Option::is_none")] pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub persona: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub gender: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub backchannel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub other_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub relationship: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub situation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub voice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")] pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub seed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub guidance_scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub max_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub speak_first: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClientMsg {
    Dialogue { id: String, sample_rate: u32, options: DialogueOptions },
    /// Two conversations of the model that hear each other; `speakers[0]` is the angel, `speakers[1]` the daemon.
    Battle { id: String, speakers: [DialogueOptions; 2], max_frames: u32 },
    Chunk { id: String, seq: u64, pcm: String },
    HangUp { id: String },
    Cancel { id: String },
}

impl ClientMsg {
    /// One JSON object followed by a newline; options are flattened into the dialogue object.
    pub fn to_line(&self) -> String {
        let value = match self {
            ClientMsg::Dialogue { id, sample_rate, options } => {
                let mut m = Map::new();
                m.insert("type".into(), json!("dialogue"));
                m.insert("id".into(), json!(id));
                m.insert("sample_rate".into(), json!(sample_rate));
                if let Value::Object(o) = serde_json::to_value(options).expect("options serialize") {
                    for (k, val) in o { m.insert(k, val); }
                }
                Value::Object(m)
            }
            ClientMsg::Battle { id, speakers, max_frames } => {
                json!({"type":"battle","id":id,"speakers":speakers,"max_frames":max_frames})
            }
            ClientMsg::Chunk { id, seq, pcm } => json!({"type":"chunk","id":id,"seq":seq,"pcm":pcm}),
            ClientMsg::HangUp { id } => json!({"type":"hang_up","id":id}),
            ClientMsg::Cancel { id } => json!({"type":"cancel","id":id}),
        };
        let mut s = value.to_string();
        s.push('\n');
        s
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WorkerErrorBody { pub code: String, pub option: Option<String>, pub message: String }

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ModelInfo {
    pub name: String, pub task: String, pub sample_rate: u32,
    #[serde(default)] pub frame_samples: Option<u32>,
    #[serde(default)] pub device: Option<String>,
    #[serde(default)] pub weight_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Ready {
    pub protocol: u32, pub version: String,
    /// Requests added since protocol 4 began that this worker serves (worker.md "Start").
    #[serde(default)] pub features: Vec<String>,
    #[serde(default)] pub defaults: serde_json::Value,
    pub model: ModelInfo,
}

impl Ready {
    pub fn supports_battle(&self) -> bool {
        self.features.iter().any(|f| f == "battle")
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerMsg {
    Ready(Ready),
    Fatal { error: WorkerErrorBody },
    Chunk { id: String, seq: u64, pcm: String, #[serde(default)] text: Option<String>, #[serde(default)] speaker: Option<u8> },
    Progress { id: String, done: f64 },
    End { id: String, #[serde(default)] frames: Option<u64>, #[serde(default)] samples: Option<u64> },
    Error { #[serde(default)] id: Option<String>, error: WorkerErrorBody },
    Cancelled { id: String },
}

/// Non-JSON or unknown type is a worker defect (worker.md:24-25).
pub fn parse_line(line: &str) -> Result<WorkerMsg, HaruError> {
    serde_json::from_str::<WorkerMsg>(line.trim()).map_err(|e| {
        HaruError::new(ErrorCode::Worker, MsgKey::WorkerBadReply)
            .with_detail(format!("{e}: {}", line.chars().take(200).collect::<String>()))
    })
}

pub fn check_ready(r: &Ready) -> Result<(), HaruError> {
    let ok = r.protocol == PROTOCOL
        && r.model.task == "dialogue"
        && r.model.frame_samples == Some(3200)
        && r.model.sample_rate == 24000;
    if ok { Ok(()) } else {
        Err(HaruError::new(ErrorCode::Worker, MsgKey::WorkerIncompatible).with_detail(format!(
            "protocol={} task={} sample_rate={} frame_samples={:?}",
            r.protocol, r.model.task, r.model.sample_rate, r.model.frame_samples)))
    }
}

pub fn worker_error(body: &WorkerErrorBody) -> HaruError {
    let mut detail = format!("{}: {}", body.code, body.message);
    if let Some(o) = &body.option { detail.push_str(&format!(" (option {o})")); }
    let e = HaruError::new(ErrorCode::Worker, MsgKey::WorkerError).with_detail(detail);
    if matches!(body.code.as_str(), "device" | "out_of_memory" | "internal") { e.backend_failure() } else { e }
}

/// A battle's refusal or failure, `out_of_memory` included: the worker still serves normal conversations, so the
/// app never moves to another backend for it.
pub fn battle_error(body: &WorkerErrorBody) -> HaruError {
    let mut e = worker_error(body);
    e.backend_failure = false;
    e
}

/// The worker ended a request because `max_seconds` ran out (haru-engine.cpp: `OutOfRange` on option `max_seconds`).
pub fn is_segment_limit(e: &WorkerErrorBody) -> bool {
    e.code == "out_of_range" && e.option.as_deref() == Some("max_seconds")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_limit_is_out_of_range_max_seconds_only() {
        let body = |code: &str, option: &str| WorkerErrorBody { code: code.into(), option: Some(option.into()), message: "m".into() };
        assert!(is_segment_limit(&body("out_of_range", "max_seconds")));
        assert!(!is_segment_limit(&body("out_of_memory", "max_seconds")));
        assert!(!is_segment_limit(&body("out_of_range", "speed")));
        assert!(!is_segment_limit(&WorkerErrorBody { code: "out_of_range".into(), option: None, message: "m".into() }));
    }

    fn v(s: &str) -> Value { serde_json::from_str(s).unwrap() }
    fn line_json(m: &ClientMsg) -> Value {
        let l = m.to_line();
        assert!(l.ends_with('\n'));
        serde_json::from_str(l.trim_end()).unwrap()
    }

    #[test]
    fn serializes_dialogue_with_only_set_options() {
        let m = ClientMsg::Dialogue {
            id: "d1".into(), sample_rate: 48000,
            options: DialogueOptions { language: Some("de".into()), speak_first: Some(true), ..Default::default() },
        };
        assert_eq!(line_json(&m), v(r#"{"type":"dialogue","id":"d1","sample_rate":48000,"language":"de","speak_first":true}"#));
    }

    #[test]
    fn serializes_chunk() {
        let m = ClientMsg::Chunk { id: "d1".into(), seq: 3, pcm: "AAA=".into() };
        assert_eq!(line_json(&m), v(r#"{"type":"chunk","id":"d1","seq":3,"pcm":"AAA="}"#));
    }

    #[test]
    fn serializes_hang_up_and_cancel() {
        assert_eq!(line_json(&ClientMsg::HangUp { id: "d1".into() }), v(r#"{"type":"hang_up","id":"d1"}"#));
        assert_eq!(line_json(&ClientMsg::Cancel { id: "d1".into() }), v(r#"{"type":"cancel","id":"d1"}"#));
    }

    #[test]
    fn parses_ready_with_model_info() {
        let m = parse_line(r#"{"type":"ready","protocol":4,"version":"0.11.1","defaults":{},"model":{"name":"Haru-4B","task":"dialogue","sample_rate":24000,"frame_samples":3200,"device":"Vulkan1","weight_type":"Q8_0","options":[]}}"#).unwrap();
        match m {
            WorkerMsg::Ready(r) => {
                assert_eq!(r.model.device.as_deref(), Some("Vulkan1"));
                assert!(check_ready(&r).is_ok());
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_dialogue_chunk_with_text() {
        match parse_line(r#"{"type":"chunk","id":"d","seq":12,"pcm":"AAA=","text":"そう"}"#).unwrap() {
            WorkerMsg::Chunk { seq, text, .. } => { assert_eq!(seq, 12); assert_eq!(text.as_deref(), Some("そう")); }
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn parses_chunk_without_text_as_none() {
        match parse_line(r#"{"type":"chunk","id":"a","seq":0,"pcm":"AAA="}"#).unwrap() {
            WorkerMsg::Chunk { text, .. } => assert_eq!(text, None),
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn parses_dialogue_end() {
        match parse_line(r#"{"type":"end","id":"d","frames":151,"samples":483200}"#).unwrap() {
            WorkerMsg::End { frames, samples, .. } => { assert_eq!(frames, Some(151)); assert_eq!(samples, Some(483200)); }
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn ignores_unknown_members() {
        match parse_line(r#"{"type":"end","id":"a","seed":1,"samples":9,"stop":"complete"}"#).unwrap() {
            WorkerMsg::End { frames, .. } => assert_eq!(frames, None),
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn reads_ready_chunk_and_end_of_a_worker_with_battle() {
        let ready = r#"{"type":"ready","protocol":4,"version":"0.11.1","features":["battle"],"defaults":{},"model":{"name":"Haru-4B","task":"dialogue","sample_rate":24000,"frame_samples":3200,"device":"Vulkan1","weight_type":"Q8_0","options":[]}}"#;
        match parse_line(ready).unwrap() {
            WorkerMsg::Ready(r) => {
                assert!(check_ready(&r).is_ok());
                assert!(r.supports_battle());
            }
            o => panic!("{o:?}"),
        }
        match parse_line(r#"{"type":"chunk","id":"b","seq":7,"speaker":1,"pcm":"AAA=","text":"ja"}"#).unwrap() {
            WorkerMsg::Chunk { id, seq, text, speaker, .. } => {
                assert_eq!(id, "b");
                assert_eq!(seq, 7);
                assert_eq!(text.as_deref(), Some("ja"));
                assert_eq!(speaker, Some(1));
            }
            o => panic!("{o:?}"),
        }
        match parse_line(r#"{"type":"end","id":"b","frames":4500}"#).unwrap() {
            WorkerMsg::End { frames, samples, .. } => {
                assert_eq!(frames, Some(4500));
                assert_eq!(samples, None);
            }
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn parses_error_with_nested_error_object() {
        match parse_line(r#"{"type":"error","id":"g","error":{"code":"out_of_range","option":"max_seconds","message":"m"}}"#).unwrap() {
            WorkerMsg::Error { id, error } => { assert_eq!(id.as_deref(), Some("g")); assert_eq!(error.code, "out_of_range"); }
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn parses_error_without_id() {
        match parse_line(r#"{"type":"error","error":{"code":"invalid_argument","option":"id","message":"m"}}"#).unwrap() {
            WorkerMsg::Error { id, .. } => assert_eq!(id, None),
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn parses_fatal() {
        match parse_line(r#"{"type":"fatal","error":{"code":"device","option":"device","message":"no such device"}}"#).unwrap() {
            WorkerMsg::Fatal { error } => assert_eq!(error.code, "device"),
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn parses_cancelled() {
        assert_eq!(parse_line(r#"{"type":"cancelled","id":"x"}"#).unwrap(), WorkerMsg::Cancelled { id: "x".into() });
    }

    #[test]
    fn rejects_non_json_line() {
        assert_eq!(parse_line("hello").unwrap_err().code, ErrorCode::Worker);
    }

    #[test]
    fn rejects_unknown_type() {
        assert_eq!(parse_line(r#"{"type":"nope"}"#).unwrap_err().code, ErrorCode::Worker);
    }

    #[test]
    fn a_dialogue_chunk_has_no_speaker() {
        match parse_line(r#"{"type":"chunk","id":"d","seq":1,"pcm":"AAA=","text":""}"#).unwrap() {
            WorkerMsg::Chunk { speaker, .. } => assert_eq!(speaker, None),
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn an_old_worker_without_features_cannot_battle() {
        let r = ready(4, "dialogue");
        assert!(!r.supports_battle());
        let parsed = parse_line(r#"{"type":"ready","protocol":4,"version":"0.11.0","defaults":{},"model":{"name":"Haru-4B","task":"dialogue","sample_rate":24000,"frame_samples":3200}}"#).unwrap();
        assert!(matches!(parsed, WorkerMsg::Ready(r) if r.features.is_empty() && !r.supports_battle()));
    }

    #[test]
    fn serializes_battle_with_two_speakers_and_max_frames() {
        let angel = DialogueOptions { name: Some("Haru".into()), other_name: Some("Mephisto".into()), speak_first: Some(true), ..Default::default() };
        let daemon = DialogueOptions { name: Some("Mephisto".into()), other_name: Some("Haru".into()), speak_first: Some(false), seed: Some(7), ..Default::default() };
        let m = ClientMsg::Battle { id: "b1".into(), speakers: [angel, daemon], max_frames: 4500 };
        assert_eq!(
            line_json(&m),
            v(r#"{"type":"battle","id":"b1","speakers":[{"name":"Haru","other_name":"Mephisto","speak_first":true},{"name":"Mephisto","other_name":"Haru","speak_first":false,"seed":7}],"max_frames":4500}"#)
        );
    }

    #[test]
    fn a_battle_error_never_asks_for_another_backend() {
        let b = |c: &str| WorkerErrorBody { code: c.into(), option: Some("max_frames".into()), message: "speaker 1: no room".into() };
        for code in ["out_of_memory", "out_of_range", "invalid_argument", "device"] {
            let e = battle_error(&b(code));
            assert_eq!(e.key, MsgKey::WorkerError);
            assert!(!e.backend_failure, "{code}");
            assert!(e.detail.as_deref().unwrap().contains(code));
        }
    }

    fn ready(protocol: u32, task: &str) -> Ready {
        Ready { protocol, version: "0.11.1".into(), features: Vec::new(), defaults: Value::Null,
            model: ModelInfo { name: "Haru-4B".into(), task: task.into(), sample_rate: 24000, frame_samples: Some(3200), device: None, weight_type: None } }
    }

    #[test]
    fn check_ready_rejects_wrong_protocol() {
        assert_eq!(check_ready(&ready(5, "dialogue")).unwrap_err().code, ErrorCode::Worker);
    }

    #[test]
    fn check_ready_rejects_non_dialogue_model() {
        assert_eq!(check_ready(&ready(4, "synthesis")).unwrap_err().code, ErrorCode::Worker);
    }

    #[test]
    fn worker_error_marks_device_as_backend_failure() {
        let b = |c: &str| WorkerErrorBody { code: c.into(), option: None, message: "m".into() };
        assert!(worker_error(&b("device")).backend_failure);
        assert!(!worker_error(&b("model_file")).backend_failure);
    }
}
