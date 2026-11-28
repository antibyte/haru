//! FeatureClock: stamped model blocks (Task 1.6).

use crate::audio::features::{Features, MIC_BANDS, MODEL_BANDS};
use crate::ipc::{BattlePhase, BattleViz, MicLevels, ModelLevels, VizFrame, WordPiece};
use std::collections::VecDeque;

pub struct FeatureClock {
    blocks: VecDeque<(u64, Features)>,
    block_len_out: u64,
}

impl FeatureClock {
    pub fn new(block_len_out: u64) -> Self {
        Self { blocks: VecDeque::new(), block_len_out }
    }

    pub fn push(&mut self, start: u64, f: Features) {
        self.blocks.push_back((start, f));
    }

    /// Block with `start <= played < start + block_len_out`; drops older blocks.
    pub fn current(&mut self, played: u64) -> Option<Features> {
        while let Some((start, _)) = self.blocks.front() {
            if start + self.block_len_out <= played {
                self.blocks.pop_front();
            } else {
                break;
            }
        }
        match self.blocks.front() {
            Some((start, f)) if *start <= played => Some(f.clone()),
            _ => None,
        }
    }
}

fn fixed<const N: usize>(f: Option<Features>) -> (f32, [f32; N]) {
    let mut bands = [0.0f32; N];
    match f {
        Some(f) => {
            for (d, s) in bands.iter_mut().zip(f.bands.iter()) {
                *d = *s;
            }
            (f.rms, bands)
        }
        None => (0.0, bands),
    }
}

pub fn build_frame(t: f64, model: Option<Features>, mic: Option<Features>, latency_ms: f64, words: Vec<WordPiece>) -> VizFrame {
    let (mr, mb) = fixed::<MODEL_BANDS>(model);
    let (kr, kb) = fixed::<MIC_BANDS>(mic);
    VizFrame { t, model: ModelLevels { rms: mr, bands: mb }, mic: MicLevels { rms: kr, bands: kb }, latency_ms, words, battle: None }
}

/// The louder of the two voices heard now, the angel's on a tie: `model` of a battle frame.
pub fn louder(angel: Option<Features>, daemon: Option<Features>) -> Option<Features> {
    match (angel, daemon) {
        (Some(a), Some(d)) => Some(if d.rms > a.rms { d } else { a }),
        (a, d) => a.or(d),
    }
}

/// A battle's viz frame: both voices as played, the louder as `model`, the mic silent and no latency.
pub fn battle_frame(t: f64, angel: Option<Features>, daemon: Option<Features>, phase: BattlePhase, prepared: f64, words: Vec<WordPiece>) -> VizFrame {
    let level = |f: Option<Features>| {
        let (rms, bands) = fixed::<MODEL_BANDS>(f);
        ModelLevels { rms, bands }
    };
    let model = louder(angel.clone(), daemon.clone());
    VizFrame {
        battle: Some(BattleViz { angel: level(angel), daemon: level(daemon), phase, prepared }),
        ..build_frame(t, model, None, 0.0, words)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(rms: f32) -> Features {
        Features { rms, bands: vec![rms; 16] }
    }

    #[test]
    fn returns_block_containing_played() {
        let mut c = FeatureClock::new(400);
        c.push(0, f(0.1));
        c.push(400, f(0.2));
        assert_eq!(c.current(399).unwrap().rms, 0.1);
        assert_eq!(c.current(400).unwrap().rms, 0.2);
        assert_eq!(c.current(799).unwrap().rms, 0.2);
    }

    #[test]
    fn none_before_first_block() {
        let mut c = FeatureClock::new(400);
        c.push(1000, f(0.1));
        assert!(c.current(999).is_none());
        assert!(c.current(1000).is_some());
    }

    #[test]
    fn none_after_last_block_end() {
        let mut c = FeatureClock::new(400);
        c.push(0, f(0.1));
        assert!(c.current(400).is_none());
    }

    #[test]
    fn drops_old_blocks() {
        let mut c = FeatureClock::new(400);
        c.push(0, f(0.1));
        c.push(400, f(0.2));
        c.push(800, f(0.3));
        assert_eq!(c.current(850).unwrap().rms, 0.3);
        assert_eq!(c.blocks.len(), 1);
    }

    #[test]
    fn louder_picks_more_rms_and_the_angel_on_a_tie() {
        assert_eq!(louder(Some(f(0.1)), Some(f(0.3))).unwrap().rms, 0.3);
        let a = Features { rms: 0.2, bands: vec![0.1; 16] };
        let d = Features { rms: 0.2, bands: vec![0.9; 16] };
        assert_eq!(louder(Some(a), Some(d)).unwrap().bands, vec![0.1; 16]);
        assert_eq!(louder(None, Some(f(0.1))).unwrap().rms, 0.1);
        assert!(louder(None, None).is_none());
    }

    #[test]
    fn battle_frame_has_both_levels_a_silent_mic_and_no_latency() {
        let v = battle_frame(1.0, Some(f(0.1)), Some(f(0.4)), BattlePhase::Paused, 0.5, vec![]);
        assert_eq!(v.model.rms, 0.4);
        assert_eq!((v.mic.rms, v.mic.bands, v.latency_ms), (0.0, [0.0; 8], 0.0));
        let b = v.battle.unwrap();
        assert_eq!((b.angel.rms, b.daemon.rms, b.phase, b.prepared), (0.1, 0.4, BattlePhase::Paused, 0.5));
    }

    #[test]
    fn build_frame_zero_fills() {
        let v = build_frame(1.0, None, None, 5.0, vec![]);
        assert_eq!(v.model.bands, [0.0; 16]);
        assert_eq!(v.mic.bands, [0.0; 8]);
        assert_eq!(v.model.rms, 0.0);
        assert_eq!(v.latency_ms, 5.0);
    }
}
