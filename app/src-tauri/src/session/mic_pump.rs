//! Capture -> 20 ms chunks -> worker (Task 1.7).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::audio::aec::{delay_hint_ms, load_latency_ms, AecError, VoiceProcessor};
use crate::audio::capture::CaptureHandle;
use crate::audio::features::{Analyzer, Features, MIC_BANDS};
use crate::audio::playback::PlaybackShared;
use crate::error::{ErrorCode, HaruError};
use crate::worker::pcm::encode_f32;
use crate::worker::process::WorkerSender;
use crate::worker::protocol::ClientMsg;

pub const POLL: Duration = Duration::from_millis(5);
pub const NO_DATA_TIMEOUT: Duration = Duration::from_millis(1000);
/// Shown once when echo cancellation drops out in the middle of a conversation.
pub const AEC_FAILED_WARNING: &str = "Echounterdrückung ausgefallen – bitte Kopfhörer verwenden.";

#[derive(Default)]
pub struct MicShared {
    pub muted: AtomicBool,
    pub started: AtomicBool,
    pub sent_samples: AtomicU64,
    /// Worker request the pump currently feeds; replaced on every segment renewal.
    pub request_id: Mutex<String>,
    /// Bumped (after `request_id` is set) whenever a new request starts; the pump restarts `seq` then.
    pub segment: AtomicU64,
    pub levels: Mutex<Option<Features>>,
    pub stop: AtomicBool,
}

/// Pops up to `want` samples from the capture ring into `buf`.
fn fill(consumer: &mut rtrb::Consumer<f32>, buf: &mut Vec<f32>, want: usize) {
    while buf.len() < want {
        match consumer.pop() {
            Ok(s) => buf.push(s),
            Err(_) => break,
        }
    }
}

/// Echo cancellation of the mic pump: Haru's own output (render tap) is subtracted from the microphone signal.
pub struct EchoInput {
    processor: VoiceProcessor,
    tap: rtrb::Consumer<f32>,
    playback: Arc<PlaybackShared>,
    on_warning: Arc<dyn Fn(String) + Send + Sync>,
    /// Smoothed stream delay hint in ms.
    hint_ms: Option<f64>,
}

impl EchoInput {
    pub fn new(
        processor: VoiceProcessor,
        tap: rtrb::Consumer<f32>,
        playback: Arc<PlaybackShared>,
        on_warning: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Self {
        Self { processor, tap, playback, on_warning, hint_ms: None }
    }

    /// Drops whatever the output played so far (no conversation yet: nothing may pile up).
    fn discard_reference(&mut self) {
        let n = self.tap.slots();
        if n > 0 {
            if let Ok(c) = self.tap.read_chunk(n) {
                c.commit_all();
            }
        }
    }

    /// Cleans one mic chunk in place (a multiple of the 10 ms frame).
    fn process(&mut self, buf: &mut [f32], capture: &CaptureHandle) -> Result<(), AecError> {
        // 1. Reference: everything the speaker played since the last chunk.
        let n = self.tap.slots();
        if n > 0 {
            if let Ok(chunk) = self.tap.read_chunk(n) {
                let (a, b) = chunk.as_slices();
                self.processor.push_render(a)?;
                self.processor.push_render(b)?;
                chunk.commit_all();
            }
        }
        // 2. Delay hint: output + input latency (from the callback timestamps) + capture backlog - render backlog.
        let raw = delay_hint_ms(
            load_latency_ms(&self.playback.output_latency_us),
            load_latency_ms(&capture.input_latency_us),
            capture.consumer.slots() as f64 * 1000.0 / capture.rate as f64,
            self.processor.pending_render_ms(),
        );
        let hint = match self.hint_ms {
            Some(prev) => prev + 0.1 * (raw - prev),
            None => raw,
        };
        self.hint_ms = Some(hint);
        self.processor.set_delay_hint_ms(hint.round() as i32);
        // 3. Cancel the echo.
        self.processor.process_capture(buf)
    }

    fn summary(&self) -> String {
        format!(
            "ERLE {:?} dB, delay {:?} ms, {} capture / {} render frames, tap dropped {}",
            self.processor.erle_db().map(|e| e.round()),
            self.processor.estimated_delay_ms(),
            self.processor.capture_frames(),
            self.processor.render_frames(),
            self.playback.tap_dropped.load(Ordering::Relaxed)
        )
    }
}

pub fn spawn_mic_pump(
    mut capture: CaptureHandle,
    sender: WorkerSender,
    shared: Arc<MicShared>,
    on_fatal: Arc<dyn Fn(HaruError) + Send + Sync>,
    echo: Option<EchoInput>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("haru-mic-pump".into())
        .spawn(move || {
            let mut echo = echo;
            let chunk = (capture.rate / 50) as usize;
            let mut analyzer = Analyzer::new(capture.rate, chunk, MIC_BANDS, 80.0, 8000.0);
            let mut buf: Vec<f32> = Vec::with_capacity(chunk);
            let mut seq = 0u64;
            let mut seg = u64::MAX;
            let mut id = String::new();
            let mut last_data = Instant::now();
            while !shared.stop.load(Ordering::Acquire) {
                if !shared.started.load(Ordering::Acquire) {
                    // Audio heard before frame 0 is dropped, like `speech talk` does (talk.cpp:127-128).
                    while capture.consumer.pop().is_ok() {}
                    if let Some(e) = echo.as_mut() {
                        e.discard_reference();
                    }
                    last_data = Instant::now();
                    thread::sleep(POLL);
                    continue;
                }
                fill(&mut capture.consumer, &mut buf, chunk);
                if buf.len() < chunk {
                    if last_data.elapsed() > NO_DATA_TIMEOUT {
                        on_fatal(HaruError::new(ErrorCode::NoMicrophone, "Das Mikrofon liefert keine Daten."));
                        return;
                    }
                    thread::sleep(POLL);
                    continue;
                }
                last_data = Instant::now();
                // Remove Haru's own voice before anything else looks at the signal (mute, levels, worker).
                let failed = echo.as_mut().and_then(|e| e.process(&mut buf, &capture).err());
                if let Some(err) = failed {
                    eprintln!("haru: echo cancellation off for this conversation: {err}");
                    if let Some(e) = echo.take() {
                        (e.on_warning)(AEC_FAILED_WARNING.to_string());
                    }
                }
                let muted = shared.muted.load(Ordering::Relaxed);
                if muted {
                    buf.iter_mut().for_each(|s| *s = 0.0);
                }
                let levels = if muted { None } else { Some(analyzer.analyze(&buf)) };
                *shared.levels.lock().unwrap_or_else(|e| e.into_inner()) = levels;
                let cur = shared.segment.load(Ordering::Acquire);
                if cur != seg {
                    seg = cur;
                    id = shared.request_id.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    seq = 0;
                }
                let msg = ClientMsg::Chunk { id: id.clone(), seq, pcm: encode_f32(&buf) };
                if let Err(e) = sender.send(&msg) {
                    on_fatal(e);
                    return;
                }
                seq += 1;
                shared.sent_samples.fetch_add(chunk as u64, Ordering::Relaxed);
                buf.clear();
            }
            if let Some(e) = echo.as_ref() {
                eprintln!("haru: echo cancellation summary: {}", e.summary());
            }
        })
        .expect("spawn mic pump thread")
}

/// Test/headless input: feeds `samples` (mono, `rate`) into a capture ring in real time once
/// `shared.started` is set, then silence, so the pump sees an ordinary (quiet) microphone.
pub fn spawn_sample_feeder(
    samples: Vec<f32>,
    rate: u32,
    mut producer: rtrb::Producer<f32>,
    shared: Arc<MicShared>,
) -> JoinHandle<()> {
    thread::Builder::new()
        .name("haru-mic-feeder".into())
        .spawn(move || {
            while !shared.started.load(Ordering::Acquire) {
                if shared.stop.load(Ordering::Acquire) {
                    return;
                }
                thread::sleep(POLL);
            }
            let t0 = Instant::now();
            let mut fed = 0u64;
            while !shared.stop.load(Ordering::Acquire) {
                let due = (t0.elapsed().as_secs_f64() * rate as f64) as u64;
                while fed < due {
                    let s = samples.get(fed as usize).copied().unwrap_or(0.0);
                    let _ = producer.push(s);
                    fed += 1;
                }
                thread::sleep(POLL);
            }
        })
        .expect("spawn sample feeder thread")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::aec::{Stages, LATENCY_UNKNOWN};
    use crate::audio::test_signals::*;
    use crate::worker::pcm::decode_f32;
    use serde_json::Value;
    use std::io::Write;
    use std::sync::atomic::AtomicU32;
    use std::sync::mpsc::{self, Receiver, Sender};

    /// Fake worker stdin: splits the written bytes into lines and forwards each as JSON.
    struct LineTap {
        buf: Vec<u8>,
        tx: Sender<Value>,
    }

    impl Write for LineTap {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.buf.extend_from_slice(data);
            while let Some(pos) = self.buf.iter().position(|b| *b == 10) {
                let line: Vec<u8> = self.buf.drain(..=pos).collect();
                let v: Value = serde_json::from_slice(&line[..line.len() - 1]).expect("json line");
                let _ = self.tx.send(v);
            }
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn sender() -> (WorkerSender, Receiver<Value>) {
        let (tx, rx) = mpsc::channel();
        (WorkerSender::with_writer(LineTap { buf: Vec::new(), tx }, 512), rx)
    }

    fn capture(rate: u32) -> (CaptureHandle, rtrb::Producer<f32>) {
        let (producer, consumer) = rtrb::RingBuffer::new(rate as usize * 2);
        let handle = CaptureHandle {
            rate,
            consumer,
            overruns: Arc::new(AtomicU64::new(0)),
            failed: Arc::new(AtomicBool::new(false)),
            input_latency_us: Arc::new(AtomicU32::new(LATENCY_UNKNOWN)),
        };
        (handle, producer)
    }

    fn no_fatal() -> Arc<dyn Fn(HaruError) + Send + Sync> {
        Arc::new(|e| panic!("unexpected fatal: {e}"))
    }

    fn stop(shared: &MicShared, pump: JoinHandle<()>) {
        shared.stop.store(true, Ordering::Release);
        pump.join().unwrap();
    }

    fn chunks(rx: &Receiver<Value>, n: usize) -> Vec<Value> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut out = Vec::new();
        while out.len() < n {
            let left = deadline.saturating_duration_since(Instant::now());
            let v = rx.recv_timeout(left).expect("expected chunk within 5 s");
            if v["type"] == "chunk" {
                out.push(v);
            }
        }
        out
    }

    fn input_chunks(n: usize, len: usize) -> Vec<Vec<f32>> {
        let mut rng = Rng(0xABCD);
        (0..n).map(|_| (0..len).map(|_| 0.2 * rng.uniform()).collect()).collect()
    }

    #[test]
    fn pump_without_echo_sends_raw_samples() {
        let (cap, mut prod) = capture(16_000);
        let (sender, rx) = sender();
        let shared = Arc::new(MicShared::default());
        shared.started.store(true, Ordering::Release);
        let input = input_chunks(3, 320);
        for c in &input {
            for s in c {
                prod.push(*s).unwrap();
            }
        }
        let pump = spawn_mic_pump(cap, sender, shared.clone(), no_fatal(), None);
        let got = chunks(&rx, 3);
        stop(&shared, pump);
        for (k, (g, want)) in got.iter().zip(&input).enumerate() {
            assert_eq!(g["seq"], k as u64);
            assert_eq!(g["pcm"], encode_f32(want).as_str());
        }
    }

    #[test]
    fn pump_disables_echo_after_error_and_warns_once() {
        let (cap, mut prod) = capture(16_000);
        let (sender, rx) = sender();
        let shared = Arc::new(MicShared::default());
        shared.started.store(true, Ordering::Release);
        let input = input_chunks(3, 320);
        for c in &input {
            for s in c {
                prod.push(*s).unwrap();
            }
        }
        let mut vp = VoiceProcessor::new(16_000, 16_000, Stages::EchoOnly).unwrap();
        vp.fail_next_for_test();
        let (_tap_prod, tap) = rtrb::RingBuffer::new(4800);
        let warnings = Arc::new(Mutex::new(Vec::<String>::new()));
        let w2 = warnings.clone();
        let echo = EchoInput::new(vp, tap, Arc::new(PlaybackShared::default()), Arc::new(move |t| w2.lock().unwrap().push(t)));
        let pump = spawn_mic_pump(cap, sender, shared.clone(), no_fatal(), Some(echo));
        let got = chunks(&rx, 3);
        stop(&shared, pump);
        assert_eq!(*warnings.lock().unwrap(), vec![AEC_FAILED_WARNING.to_string()]);
        for (g, want) in got.iter().zip(&input) {
            assert_eq!(g["pcm"], encode_f32(want).as_str());
        }
    }

    #[test]
    fn pump_drains_tap_while_not_started() {
        let (cap, _prod) = capture(16_000);
        let (sender, _rx) = sender();
        let shared = Arc::new(MicShared::default());
        let vp = VoiceProcessor::new(16_000, 16_000, Stages::EchoOnly).unwrap();
        let (mut tap_prod, tap) = rtrb::RingBuffer::new(4800);
        for _ in 0..3000 {
            tap_prod.push(0.1).unwrap();
        }
        assert_eq!(tap_prod.slots(), 1800);
        let echo = EchoInput::new(vp, tap, Arc::new(PlaybackShared::default()), Arc::new(|_| {}));
        let pump = spawn_mic_pump(cap, sender, shared.clone(), no_fatal(), Some(echo));
        let deadline = Instant::now() + Duration::from_secs(1);
        while tap_prod.slots() != 4800 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(tap_prod.slots(), 4800);
        stop(&shared, pump);
    }

    /// Real-time simulation of the whole path: a "device" thread feeds far-end audio into the render tap and its
    /// echo (60 ms later, through a room impulse response) into the capture ring, paced by the wall clock; the real
    /// pump cancels it. Slow (8.5 s), hence ignored by default.
    #[test]
    #[ignore]
    fn pump_cancels_echo_in_real_time_simulation() {
        const RATE: u32 = 48_000;
        const SECONDS: usize = 8;
        let n = SECONDS * RATE as usize + RATE as usize / 2;
        let far = speech_like(FAR, RATE, n);
        let h = room_impulse_response(0x5EED_0003, RATE);
        let echo = echo_of(&far, &h, 2880);
        let mut rng = Rng(0x5EED_0004);
        let mic: Vec<f32> = echo.iter().map(|e| e + 1e-4 * rng.uniform()).collect();

        let (cap, mut cap_prod) = capture(RATE);
        let (sender, rx) = sender();
        let shared = Arc::new(MicShared::default());
        shared.started.store(true, Ordering::Release);
        let (mut tap_prod, tap) = rtrb::RingBuffer::new(RATE as usize * 2);
        let vp = VoiceProcessor::new(RATE, RATE, Stages::EchoOnly).unwrap();
        let echo_in = EchoInput::new(vp, tap, Arc::new(PlaybackShared::default()), Arc::new(|_| {}));
        let pump = spawn_mic_pump(cap, sender, shared.clone(), no_fatal(), Some(echo_in));

        let dev_stop = Arc::new(AtomicBool::new(false));
        let ds = dev_stop.clone();
        let (far_d, mic_d) = (far.clone(), mic.clone());
        let device = thread::spawn(move || {
            let t0 = Instant::now();
            let mut fed = 0usize;
            while !ds.load(Ordering::Acquire) {
                let due = ((t0.elapsed().as_secs_f64() * RATE as f64) as usize).min(far_d.len());
                while fed < due {
                    let _ = tap_prod.push(far_d[fed]);
                    let _ = cap_prod.push(mic_d[fed]);
                    fed += 1;
                }
                thread::sleep(Duration::from_millis(10));
            }
        });
        thread::sleep(Duration::from_millis(8500));
        dev_stop.store(true, Ordering::Release);
        device.join().unwrap();
        thread::sleep(Duration::from_millis(100));
        stop(&shared, pump);

        let mut out: Vec<f32> = Vec::new();
        while let Ok(v) = rx.try_recv() {
            if v["type"] == "chunk" {
                out.extend(decode_f32(v["pcm"].as_str().unwrap()).unwrap());
            }
        }
        let (a, b) = (5 * RATE as usize, 8 * RATE as usize);
        assert!(out.len() >= b, "only {} samples came through", out.len());
        let erle = ratio_db(&mic[a..b], &out[a..b]);
        eprintln!("real-time simulation: {} samples through the pump, ERLE 5-8 s {erle:.1} dB", out.len());
        assert!(erle >= 15.0, "ERLE {erle:.1} dB");
    }
}
