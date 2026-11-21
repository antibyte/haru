//! Frame timeline {seq, text, startSample} (Task 1.6).

use crate::ipc::WordPiece;
use std::collections::VecDeque;

#[derive(Default)]
pub struct FrameTimeline {
    entries: VecDeque<(u64, String, u64)>,
}

impl FrameTimeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, seq: u64, text: String, start_sample: u64) {
        self.entries.push_back((seq, text, start_sample));
    }

    /// Entries whose audio has started playing (`start_sample <= played`), in order, once.
    pub fn drain_audible(&mut self, played: u64) -> Vec<WordPiece> {
        let mut out = Vec::new();
        while let Some((_, _, start)) = self.entries.front() {
            if *start > played {
                break;
            }
            let (seq, text, _) = self.entries.pop_front().unwrap();
            out.push(WordPiece { seq, text });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_released_when_audible() {
        let mut t = FrameTimeline::default();
        t.push(0, "a".into(), 0);
        t.push(1, "b".into(), 6400);
        assert_eq!(t.drain_audible(0).iter().map(|w| w.seq).collect::<Vec<_>>(), vec![0]);
        assert!(t.drain_audible(6399).is_empty());
        assert_eq!(t.drain_audible(6400).iter().map(|w| w.seq).collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn released_once_in_order() {
        let mut t = FrameTimeline::default();
        for i in 0..4 {
            t.push(i, format!("w{i}"), i * 100);
        }
        let a = t.drain_audible(250);
        assert_eq!(a.iter().map(|w| w.seq).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert!(t.drain_audible(250).is_empty());
        assert_eq!(t.drain_audible(10_000).len(), 1);
    }

    #[test]
    fn empty_and_space_text_included() {
        let mut t = FrameTimeline::default();
        t.push(0, "".into(), 0);
        t.push(1, " ".into(), 0);
        let w = t.drain_audible(0);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].text, "");
        assert_eq!(w[1].text, " ");
    }
}
