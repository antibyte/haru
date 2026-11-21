#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode { Download, HashMismatch, DiskSpace, NoBackend, Worker, NoMicrophone, AudioDevice, LagLimit, Settings, Io, Busy, Internal }

#[derive(Debug, Clone, PartialEq, serde::Serialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{code:?}: {message}")]
pub struct HaruError {
    pub code: ErrorCode,
    pub message: String,          // German, user-facing
    pub detail: Option<String>,   // technical: stderr tail, io error text
    #[serde(skip)]
    pub backend_failure: bool,    // true → the next backend attempt may succeed (device/out_of_memory/internal/crash/timeout)
}

impl HaruError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        HaruError { code, message: message.into(), detail: None, backend_failure: false }
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
        HaruError::new(ErrorCode::Io, "Dateifehler").with_detail(e.to_string())
    }
}
