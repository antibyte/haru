#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode { Download, HashMismatch, DiskSpace, NoBackend, Worker, NoMicrophone, AudioDevice, LagLimit, Settings, Io, Busy, Internal }

use crate::messages::{render, MsgKey, ParamValue, Params, UiText};

#[derive(Debug, Clone, PartialEq, serde::Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{code:?}: {message}")]
pub struct HaruError {
    pub code: ErrorCode,
    pub key: MsgKey,              // fine-grained key the frontend translates
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub params: Params,
    pub message: String,          // German fallback (logs, technical text)
    pub detail: Option<String>,   // technical: stderr tail, io error text
    #[serde(skip)]
    pub backend_failure: bool,    // true → the next backend attempt may succeed (device/out_of_memory/internal/crash/timeout)
}

impl HaruError {
    pub fn new(code: ErrorCode, key: MsgKey) -> Self {
        HaruError { code, key, params: Params::new(), message: key.german().to_string(), detail: None, backend_failure: false }
    }
    pub fn with_param(mut self, name: &'static str, v: impl Into<ParamValue>) -> Self {
        self.params.insert(name, v.into());
        self.message = render(self.key.german(), &self.params);
        self
    }
    pub fn text(&self) -> UiText {
        UiText { key: self.key, params: self.params.clone(), message: self.message.clone() }
    }
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
    pub fn backend_failure(mut self) -> Self {
        self.backend_failure = true;
        self
    }
}

impl From<std::io::Error> for HaruError {
    fn from(e: std::io::Error) -> Self {
        HaruError::new(ErrorCode::Io, MsgKey::IoError).with_detail(e.to_string())
    }
}
