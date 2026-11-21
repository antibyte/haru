//! Dev/test-only hooks, driven by environment variables. Without them the app behaves normally.
//!
//! * `HARU_TEST_WAV=<path>`      feed this WAV file as the microphone (paced in real time from the
//!                               start of each conversation, then silence). Playback stays on the
//!                               real output device unless `HARU_TEST_NO_PLAYBACK=1`.
//! * `HARU_TEST_NO_PLAYBACK=1`   Haru's audio is consumed by a virtual sink (silent run). Also works
//!                               without a WAV (the real microphone is then still used).
//!   Echo cancellation is off with either of these two (no real speaker reference), headless/feeder runs
//!   therefore exercise the raw-microphone path.
//! * `HARU_TEST_AUTOSTART=1`     the UI dismisses the headphone hint and presses Start by itself once
//!                               the core is ready (see the `test_hooks` command).
//! * `HARU_TEST_NO_RENDER=1`    (with `HARU_TEST_DEBUG`) stops the WebGL render loop, to tell UI GPU load
//!                               apart from core/worker lag.
//! * `HARU_MAX_SECONDS=<s>`      (not a test hook proper) fixes the length of a conversation segment, to
//!                               exercise the automatic segment renewal quickly (10..=3600).
//! * `HARU_TEST_QUALITY=<level>` (with `HARU_TEST_DEBUG`) pins the render quality (no auto-degrade).
//! * `HARU_TEST_DEBUG=1`         the UI enables its `?debug` overlay and logs GL renderer / fps / state
//!                               to this process' stderr (`test_log` command).

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use crate::session::conversation::ConversationIo;

fn flag(name: &str) -> bool {
    std::env::var(name).map(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false")).unwrap_or(false)
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TestHooks {
    pub autostart: bool,
    pub debug: bool,
    /// Stops the render loop right after start-up (isolates GPU contention from the worker).
    pub no_render: bool,
    /// Forces a render quality level (low|medium|high|ultra) through the debug overlay's `?quality=`.
    pub quality: Option<String>,
}

impl TestHooks {
    pub fn from_env() -> Self {
        Self {
            autostart: flag("HARU_TEST_AUTOSTART"), debug: flag("HARU_TEST_DEBUG"), no_render: flag("HARU_TEST_NO_RENDER"),
            quality: std::env::var("HARU_TEST_QUALITY").ok().filter(|q| matches!(q.as_str(), "low" | "medium" | "high" | "ultra")),
        }
    }
}

/// Autostart is granted once per process: a webview reload must not press Start again.
#[derive(Debug)]
pub struct AutostartOnce(AtomicBool);

impl AutostartOnce {
    pub fn new(enabled: bool) -> Self {
        Self(AtomicBool::new(enabled))
    }

    /// True on the first call when enabled, false afterwards.
    pub fn take(&self) -> bool {
        self.0.swap(false, Ordering::SeqCst)
    }
}

/// Decodes a WAV file to mono f32 samples and its sample rate.
pub fn load_wav(path: &str) -> Result<(u32, Vec<f32>), String> {
    let mut r = hound::WavReader::open(path).map_err(|e| format!("{path}: {e}"))?;
    let spec = r.spec();
    let ch = spec.channels.max(1) as usize;
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 / scale)).collect::<Result<_, _>>().map_err(|e| e.to_string())?
        }
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>().map_err(|e| e.to_string())?,
    };
    let mono = raw.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
    Ok((spec.sample_rate, mono))
}

/// The test I/O requested through the environment, or `None` when no hook is set.
pub fn io_from_env() -> Option<ConversationIo> {
    let wav = std::env::var("HARU_TEST_WAV").ok().filter(|s| !s.is_empty());
    let no_playback = flag("HARU_TEST_NO_PLAYBACK");
    if wav.is_none() && !no_playback {
        return None;
    }
    let input_samples = match wav {
        Some(p) => match load_wav(&p) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("[haru test hook] HARU_TEST_WAV ignored: {e}");
                None
            }
        },
        None => None,
    };
    eprintln!("[haru test hook] test I/O active (wav: {}, no_playback: {no_playback})", input_samples.is_some());
    Some(ConversationIo { input_samples, no_playback })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autostart_is_granted_exactly_once() {
        let a = AutostartOnce::new(true);
        assert!(a.take());
        assert!(!a.take());
        assert!(!a.take());
    }

    #[test]
    fn autostart_stays_off_when_disabled() {
        let a = AutostartOnce::new(false);
        assert!(!a.take());
    }
}
