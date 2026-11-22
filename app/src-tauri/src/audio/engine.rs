//! Audio engine combining capture and playback (Task 1.5).
//!
//! `cpal::Stream` is not `Send` on every platform, so both streams live on a dedicated thread.

use super::capture::{build_capture, CaptureHandle};
use super::devices::{input_device, output_device};
use super::playback::{build_playback, PlaybackHandle};
use crate::error::{ErrorCode, HaruError};
use crate::messages::MsgKey;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};

/// Frames of Haru audio buffered before playback starts.
pub const PREBUFFER_FRAMES: usize = 2;

pub struct AudioEngine {
    thread: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}

impl AudioEngine {
    pub fn start(
        input: Option<String>,
        output: Option<String>,
        render_tap: bool,
    ) -> Result<(AudioEngine, CaptureHandle, PlaybackHandle), HaruError> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = stop.clone();
        let (tx, rx) = mpsc::channel::<Result<(CaptureHandle, PlaybackHandle), HaruError>>();
        let thread = thread::Builder::new()
            .name("haru-audio".into())
            .spawn(move || {
                let built = (|| {
                    let in_dev = input_device(input.as_deref())?;
                    let out_dev = output_device(output.as_deref())?;
                    let (in_stream, cap) = build_capture(&in_dev)?;
                    let (out_stream, play) = build_playback(&out_dev, PREBUFFER_FRAMES, render_tap)?;
                    Ok::<_, HaruError>((in_stream, out_stream, cap, play))
                })();
                match built {
                    Ok((in_stream, out_stream, cap, play)) => {
                        if tx.send(Ok((cap, play))).is_err() {
                            return;
                        }
                        while !stop_t.load(Ordering::Acquire) {
                            thread::park();
                        }
                        drop(in_stream);
                        drop(out_stream);
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| HaruError::new(ErrorCode::AudioDevice, MsgKey::AudioStartFailed).with_detail(e.to_string()))?;
        match rx.recv() {
            Ok(Ok((cap, play))) => Ok((AudioEngine { thread: Some(thread), stop }, cap, play)),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(HaruError::new(ErrorCode::AudioDevice, MsgKey::AudioStartFailed)
                    .with_detail("audio thread ended unexpectedly"))
            }
        }
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}
