//! Backlog and latency policy (Task 1.6).

pub const FRAME_SECS: f64 = 3200.0 / 24000.0;
pub const LAG_WARN_SECS: f64 = 0.5;

/// Seconds of conversation the model is behind real time. Frame 0 arrives before any
/// audio (F7), so it pays for no mic time.
pub fn backlog_secs(sent_mic_samples: u64, mic_rate: u32, frames_received: u64, buffered_out: u64, out_rate: u32) -> f64 {
    sent_mic_samples as f64 / mic_rate as f64 - frames_received.saturating_sub(1) as f64 * FRAME_SECS
        + buffered_out as f64 / out_rate as f64
}

#[derive(Debug, PartialEq)]
pub enum LagLevel {
    Ok,
    Warn,
    Exceeded,
}

pub fn classify(backlog: f64, limit_secs: f64) -> LagLevel {
    if backlog > limit_secs {
        LagLevel::Exceeded
    } else if backlog > LAG_WARN_SECS {
        LagLevel::Warn
    } else {
        LagLevel::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backlog_zero_when_in_sync() {
        // 2.0 s sent, 16 frames => 15 * 0.13333 = 2.0
        assert!(backlog_secs(96000, 48000, 16, 0, 48000).abs() < 1e-9);
    }

    #[test]
    fn counts_buffered_playback() {
        assert!((backlog_secs(96000, 48000, 16, 4800, 48000) - 0.1).abs() < 1e-9);
    }

    #[test]
    fn grows_when_model_slow() {
        assert!((backlog_secs(96000, 48000, 8, 0, 48000) - (2.0 - 7.0 * 0.13333333333)).abs() < 1e-6);
    }

    #[test]
    fn frame_zero_costs_nothing() {
        assert_eq!(backlog_secs(0, 48000, 1, 0, 48000), 0.0);
    }

    #[test]
    fn classify_thresholds() {
        assert_eq!(classify(0.4, 1.0), LagLevel::Ok);
        assert_eq!(classify(0.6, 1.0), LagLevel::Warn);
        assert_eq!(classify(1.01, 1.0), LagLevel::Exceeded);
        assert_eq!(classify(1.5, 2.0), LagLevel::Warn);
    }
}
