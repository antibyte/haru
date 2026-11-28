//! The angel-vs-daemon battle: one worker request whose two speakers are paired, voiced, mixed into the one
//! playback ring and pre-buffered. A battle has no microphone, no echo cancellation, no segment renewal and no lag
//! limit, so it runs beside `Conversation` and shares its pure pieces.

use crate::cards;
use crate::error::{ErrorCode, HaruError};
use crate::messages::MsgKey;

/// The topic a battle starts with, or why it cannot start: a worker without the battle feature, or an empty topic.
pub fn battle_start(topic: &str, supported: bool) -> Result<String, HaruError> {
    if !supported {
        return Err(HaruError::new(ErrorCode::Worker, MsgKey::BattleUnsupported));
    }
    cards::battle_topic(topic).ok_or_else(|| HaruError::new(ErrorCode::Settings, MsgKey::BattleTopic))
}

/// The two voices summed sample by sample; overlap is allowed, the sum is clamped like the cue mix.
pub fn mix(angel: &[f32], daemon: &[f32]) -> Vec<f32> {
    angel.iter().zip(daemon).map(|(a, d)| (a + d).clamp(-1.0, 1.0)).collect()
}

/// Both speakers' frame of one `seq`: index 0 the angel, 1 the daemon.
#[derive(Debug, PartialEq)]
pub struct Pair {
    pub seq: u64,
    pub frames: [Vec<f32>; 2],
    pub texts: [String; 2],
}

/// Joins the worker's chunks, which come speaker 0's and then speaker 1's for each `seq` (worker.md, battle).
#[derive(Default)]
pub struct FramePairs {
    held: Option<(u64, Vec<f32>, String)>,
    /// The `seq` of the last pair given out; a pair never comes twice.
    last: Option<u64>,
}

impl FramePairs {
    /// An angel frame is waiting for its daemon frame.
    pub fn pending(&self) -> bool {
        self.held.is_some()
    }

    pub fn accept(&mut self, speaker: Option<u8>, seq: u64, pcm: Vec<f32>, text: String) -> Result<Option<Pair>, String> {
        match (speaker, self.held.take()) {
            (Some(0), None) if self.last.is_some_and(|l| seq <= l) => Err(format!("angel frame with seq {seq} after the pair {:?}", self.last)),
            (Some(0), None) => {
                self.held = Some((seq, pcm, text));
                Ok(None)
            }
            (Some(1), Some((held, angel, _))) if held == seq && angel.len() != pcm.len() => {
                Err(format!("frames of seq {seq} have {} and {} samples", angel.len(), pcm.len()))
            }
            (Some(1), Some((held, angel, angel_text))) if held == seq => {
                self.last = Some(seq);
                Ok(Some(Pair { seq, frames: [angel, pcm], texts: [angel_text, text] }))
            }
            (speaker, held) => Err(format!("chunk of speaker {speaker:?} with seq {seq} after {:?}", held.map(|h| h.0))),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battle_start_refuses_an_old_worker_and_an_empty_topic() {
        assert_eq!(battle_start("x", false).unwrap_err().key, MsgKey::BattleUnsupported);
        let e = battle_start(" \t ", true).unwrap_err();
        assert_eq!((e.key, e.code), (MsgKey::BattleTopic, ErrorCode::Settings));
        assert_eq!(battle_start("  Darf man den letzten Keks nehmen?  ", true).unwrap(), "Darf man den letzten Keks nehmen?");
    }

    #[test]
    fn mix_sums_both_voices_and_clamps() {
        assert_eq!(mix(&[0.1, 0.8, -0.8, 0.0], &[0.2, 0.7, -0.7, 0.0]), vec![0.1 + 0.2, 1.0, -1.0, 0.0]);
    }

    #[test]
    fn frame_pairs_join_the_angel_and_the_daemon_of_one_seq() {
        let mut p = FramePairs::default();
        assert!(p.accept(Some(0), 4, vec![0.1], "a".into()).unwrap().is_none());
        let pair = p.accept(Some(1), 4, vec![0.2], "d".into()).unwrap().unwrap();
        assert_eq!(pair.seq, 4);
        assert_eq!(pair.frames, [vec![0.1], vec![0.2]]);
        assert_eq!(pair.texts, ["a".to_string(), "d".to_string()]);
    }

    #[test]
    fn frame_pairs_never_give_a_pair_twice_and_know_what_waits() {
        let mut p = FramePairs::default();
        assert!(!p.pending());
        p.accept(Some(0), 3, vec![0.1], String::new()).unwrap();
        assert!(p.pending());
        assert!(p.accept(Some(1), 3, vec![0.2], String::new()).unwrap().is_some());
        assert!(!p.pending());
        assert!(p.accept(Some(0), 3, vec![0.1], String::new()).is_err(), "a duplicate pair");
        assert!(p.accept(Some(0), 2, vec![0.1], String::new()).is_err(), "an older pair");
        assert!(p.accept(Some(0), 4, vec![0.1], String::new()).is_ok());
    }

    #[test]
    fn frame_pairs_refuse_frames_of_unequal_length() {
        let mut p = FramePairs::default();
        p.accept(Some(0), 0, vec![0.0; 3], String::new()).unwrap();
        assert!(p.accept(Some(1), 0, vec![0.0; 2], String::new()).is_err());
    }

    #[test]
    fn frame_pairs_refuse_what_the_worker_never_sends() {
        let mut p = FramePairs::default();
        assert!(p.accept(None, 0, vec![], String::new()).is_err(), "a chunk without speaker");
        assert!(p.accept(Some(1), 0, vec![], String::new()).is_err(), "the daemon first");
        assert!(p.accept(Some(2), 0, vec![], String::new()).is_err(), "a third speaker");
        p.accept(Some(0), 0, vec![], String::new()).unwrap();
        assert!(p.accept(Some(1), 1, vec![], String::new()).is_err(), "another seq");
        p.accept(Some(0), 0, vec![], String::new()).unwrap();
        assert!(p.accept(Some(0), 1, vec![], String::new()).is_err(), "two angel frames in a row");
    }
}
