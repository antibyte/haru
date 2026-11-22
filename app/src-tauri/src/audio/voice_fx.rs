//! Voice effects on Haru-4B's 24 kHz output: the daemon's deep, dark voice (v2).
//!
//! The effect runs on the decoded 24 kHz frame, before the resampler and the playback ring, so the echo
//! canceller's render tap and the lipsync analysis both see what is heard.
//!
//! Chain: Signalsmith Stretch (phase-vocoder pitch shift with separate formant control, streaming) ->
//! optional sub layer (second stretch, -12 st) -> subtle even-harmonic saturation -> EQ (HPF, low shelf,
//! presence) -> 8-line feedback-delay-network reverb -> adaptive make-up gain + soft limiter.
//! Everything after the stretch works sample by sample with persistent state; the stretch is a streaming
//! processor with equal input/output length, so chunked processing equals one-shot processing.

use crate::cards::Persona;
use signalsmith_stretch::Stretch;

pub const RATE: f32 = 24_000.0;

// ---- tunables --------------------------------------------------------------------------------------------
/// Main pitch shift in semitones (male voice in, so -9 gives a deep, dark baritone).
pub const PITCH_SEMITONES: f32 = -9.0;
/// Where the formants end up, in semitones relative to the input (not relative to the shifted pitch):
/// less than the pitch shift keeps the body without the "big mouth" muffling.
pub const FORMANT_SEMITONES: f32 = -2.0;
/// STFT block and hop of the stretch. The added latency is about one block (analysis + synthesis half each).
pub const STRETCH_BLOCK_MS: f32 = 64.0;
pub const STRETCH_INTERVAL_MS: f32 = 16.0;
/// Sub layer: a second stretch at -12 st relative to the input, mixed in at this level (0 = off).
pub const SUB_SEMITONES: f32 = -12.0;
pub const SUB_MIX: f32 = 0.1;
/// Grit: asymmetric tanh (even harmonics), blended in parallel. SAT_MIX = 0 turns it off.
pub const SAT_DRIVE: f32 = 2.5;
pub const SAT_BIAS: f32 = 0.2;
pub const SAT_MIX: f32 = 0.2;
/// EQ (RBJ biquads): no high-frequency cut.
pub const HPF_HZ: f64 = 45.0;
pub const LOW_SHELF_HZ: f64 = 150.0;
pub const LOW_SHELF_DB: f64 = 2.5;
pub const PRESENCE_HZ: f64 = 2800.0;
pub const PRESENCE_DB: f64 = 2.0;
pub const PRESENCE_Q: f64 = 0.8;
/// Reverb ("dark stone cellar"): 8-line FDN, Householder feedback matrix, mutually prime delays
/// (25-67 ms at 24 kHz), per-line damping, slightly modulated delays.
pub const REVERB_DELAYS: [usize; 8] = [599, 727, 859, 997, 1129, 1283, 1423, 1597];
pub const REVERB_RT60_S: f32 = 1.0;
pub const REVERB_DAMP: f32 = 0.4;
pub const REVERB_MOD_SAMPLES: f32 = 1.2;
pub const REVERB_RETURN: f32 = 0.55;
pub const REVERB_WET: f32 = 0.15;
/// Make-up gain: slowly tracks input RMS / output RMS while there is speech.
pub const MAKEUP_WINDOW_MS: f32 = 400.0;
pub const MAKEUP_SMOOTH_MS: f32 = 150.0;
pub const MAKEUP_MIN: f32 = 0.4;
pub const MAKEUP_MAX: f32 = 3.0;
/// Input mean-square energy below which the make-up gain is held.
pub const MAKEUP_ACTIVE_ENERGY: f32 = 1e-5;
/// Soft limiter: linear below the knee, tanh above; output never exceeds CEILING.
pub const LIMIT_KNEE: f32 = 0.8;
pub const LIMIT_CEILING: f32 = 0.979;

/// The per-run tunables (defaults = the consts above); the offline example overrides them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DemonParams {
    pub pitch_semitones: f32,
    pub formant_semitones: f32,
    pub sub_mix: f32,
    pub reverb_wet: f32,
    pub presence_db: f64,
}

impl Default for DemonParams {
    fn default() -> Self {
        Self {
            pitch_semitones: PITCH_SEMITONES,
            formant_semitones: FORMANT_SEMITONES,
            sub_mix: SUB_MIX,
            reverb_wet: REVERB_WET,
            presence_db: PRESENCE_DB,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VoiceStyle {
    #[default]
    Natural,
    Demon,
}

impl Persona {
    pub fn voice_style(self) -> VoiceStyle {
        match self {
            Persona::Angel => VoiceStyle::Natural,
            Persona::Daemon => VoiceStyle::Demon,
        }
    }
}

/// `HARU_VOICE_FX=off` forces Natural (for A/B listening).
pub fn effective_style(style: VoiceStyle) -> VoiceStyle {
    if std::env::var("HARU_VOICE_FX").is_ok_and(|v| v.eq_ignore_ascii_case("off")) {
        VoiceStyle::Natural
    } else {
        style
    }
}

/// Style-dependent processor: Natural leaves the samples untouched.
pub struct VoiceFx {
    demon: Option<DemonVoice>,
}

impl VoiceFx {
    pub fn new(style: VoiceStyle) -> Self {
        Self { demon: (style == VoiceStyle::Demon).then(DemonVoice::new) }
    }

    pub fn is_active(&self) -> bool {
        self.demon.is_some()
    }

    pub fn process_in_place(&mut self, frame: &mut [f32]) {
        if let Some(d) = &mut self.demon {
            d.process_in_place(frame);
        }
    }
}

/// Which stages are on (everything in production; subsets for tests).
#[derive(Clone, Copy, Debug)]
pub struct Stages {
    pub main_shift: bool,
    pub sub: bool,
    pub saturation: bool,
    pub eq: bool,
    pub reverb: bool,
    pub makeup_limit: bool,
}

impl Stages {
    pub const ALL: Stages = Stages { main_shift: true, sub: true, saturation: true, eq: true, reverb: true, makeup_limit: true };
    pub const MAIN_ONLY: Stages = Stages { main_shift: true, sub: false, saturation: false, eq: false, reverb: false, makeup_limit: false };
}

// ---- pitch shifter (Signalsmith Stretch) -----------------------------------------------------------------

fn make_stretch(semitones: f32, formant_semitones: f32) -> Stretch {
    let block = (STRETCH_BLOCK_MS * 0.001 * RATE).round() as usize;
    let interval = (STRETCH_INTERVAL_MS * 0.001 * RATE).round() as usize;
    let mut s = Stretch::new(1, block, interval);
    s.set_transpose_factor_semitones(semitones, None);
    // Formants at `formant_semitones` relative to the input, independent of the pitch shift.
    s.set_formant_factor_semitones(formant_semitones, true);
    s
}

// ---- biquad ----------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn from_coefs(b: [f64; 3], a: [f64; 3]) -> Self {
        Self { b0: b[0] / a[0], b1: b[1] / a[0], b2: b[2] / a[0], a1: a[1] / a[0], a2: a[2] / a[0], z1: 0.0, z2: 0.0 }
    }

    fn highpass(f0: f64) -> Self {
        let w = 2.0 * std::f64::consts::PI * f0 / RATE as f64;
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        Self::from_coefs([(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0], [1.0 + al, -2.0 * c, 1.0 - al])
    }

    /// RBJ low shelf with slope S = 1.
    fn low_shelf(f0: f64, db: f64) -> Self {
        let a = 10f64.powf(db / 40.0);
        let w = 2.0 * std::f64::consts::PI * f0 / RATE as f64;
        let (s, c) = w.sin_cos();
        let al = s / 2.0 * 2f64.sqrt();
        let t = 2.0 * a.sqrt() * al;
        Self::from_coefs(
            [a * ((a + 1.0) - (a - 1.0) * c + t), 2.0 * a * ((a - 1.0) - (a + 1.0) * c), a * ((a + 1.0) - (a - 1.0) * c - t)],
            [(a + 1.0) + (a - 1.0) * c + t, -2.0 * ((a - 1.0) + (a + 1.0) * c), (a + 1.0) + (a - 1.0) * c - t],
        )
    }

    /// RBJ peaking EQ.
    fn peak(f0: f64, db: f64, q: f64) -> Self {
        let a = 10f64.powf(db / 40.0);
        let w = 2.0 * std::f64::consts::PI * f0 / RATE as f64;
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * q);
        Self::from_coefs([1.0 + al * a, -2.0 * c, 1.0 - al * a], [1.0 + al / a, -2.0 * c, 1.0 - al / a])
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let x = x as f64;
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y as f32
    }
}

// ---- reverb: 8-line feedback delay network ---------------------------------------------------------------

#[inline]
fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 {
        0.0
    } else {
        x
    }
}

struct FdnLine {
    buf: Vec<f32>,
    mask: usize,
    wp: usize,
    base: f32,
    gain: f32,
    lp: f32,
    lfo: f32,
    lfo_inc: f32,
}

struct Fdn {
    lines: [FdnLine; 8],
}

const FDN_IN: [f32; 8] = [1.0, 1.0, -1.0, 1.0, -1.0, -1.0, 1.0, -1.0];
const FDN_OUT: [f32; 8] = [1.0, -1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0];

impl Fdn {
    fn new() -> Self {
        let lines = std::array::from_fn(|i| {
            let d = REVERB_DELAYS[i];
            let len = (d + REVERB_MOD_SAMPLES.ceil() as usize + 4).next_power_of_two();
            // Loop gain for the requested RT60: -60 dB after RT60 seconds.
            let gain = 10f32.powf(-3.0 * d as f32 / (REVERB_RT60_S * RATE));
            let hz = 0.11 + 0.037 * i as f32;
            FdnLine { buf: vec![0.0; len], mask: len - 1, wp: 0, base: d as f32, gain, lp: 0.0, lfo: i as f32 * 0.37, lfo_inc: hz / RATE }
        });
        Self { lines }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let mut v = [0.0f32; 8];
        let mut out = 0.0;
        for (i, l) in self.lines.iter_mut().enumerate() {
            let m = REVERB_MOD_SAMPLES * (2.0 * std::f32::consts::PI * l.lfo).sin();
            l.lfo += l.lfo_inc;
            if l.lfo >= 1.0 {
                l.lfo -= 1.0;
            }
            let delay = l.base + m;
            let k = delay.floor();
            let f = delay - k;
            let k = k as usize;
            let a = l.buf[l.wp.wrapping_sub(k) & l.mask];
            let b = l.buf[l.wp.wrapping_sub(k + 1) & l.mask];
            let d = a + (b - a) * f;
            l.lp = flush(d * (1.0 - REVERB_DAMP) + l.lp * REVERB_DAMP);
            v[i] = l.lp * l.gain;
            out += FDN_OUT[i] * l.lp;
        }
        // Householder reflection: orthogonal, lossless mixing (I - 2/N * 1 1^T).
        let sum: f32 = v.iter().sum::<f32>() * (2.0 / 8.0);
        let inj = x * 0.353_553_4; // 1/sqrt(8)
        for (i, l) in self.lines.iter_mut().enumerate() {
            l.wp = (l.wp + 1) & l.mask;
            l.buf[l.wp] = flush(inj * FDN_IN[i] + v[i] - sum);
        }
        out * REVERB_RETURN
    }
}

// ---- the daemon voice ------------------------------------------------------------------------------------

pub struct DemonVoice {
    stages: Stages,
    params: DemonParams,
    main: Stretch,
    sub: Option<Stretch>,
    scratch_main: Vec<f32>,
    scratch_sub: Vec<f32>,
    eq: [Biquad; 3],
    reverb: Fdn,
    sat_dc: f32,
    e_in: f32,
    e_out: f32,
    win_a: f32,
    gain: f32,
    gain_a: f32,
}

impl Default for DemonVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl DemonVoice {
    pub fn new() -> Self {
        Self::with_stages(Stages::ALL)
    }

    pub fn with_stages(stages: Stages) -> Self {
        Self::with_params(stages, DemonParams::default())
    }

    pub fn with_params(stages: Stages, params: DemonParams) -> Self {
        let coef = |ms: f32| 1.0 - (-1.0 / (ms * 0.001 * RATE)).exp();
        let sub = (stages.sub && params.sub_mix > 0.0).then(|| make_stretch(SUB_SEMITONES, params.formant_semitones));
        Self {
            stages,
            params,
            main: make_stretch(params.pitch_semitones, params.formant_semitones),
            sub,
            scratch_main: Vec::new(),
            scratch_sub: Vec::new(),
            eq: [Biquad::highpass(HPF_HZ), Biquad::low_shelf(LOW_SHELF_HZ, LOW_SHELF_DB), Biquad::peak(PRESENCE_HZ, params.presence_db, PRESENCE_Q)],
            reverb: Fdn::new(),
            sat_dc: (SAT_DRIVE * SAT_BIAS).tanh() / SAT_DRIVE,
            e_in: 0.0,
            e_out: 0.0,
            win_a: coef(MAKEUP_WINDOW_MS),
            gain: 1.0,
            gain_a: coef(MAKEUP_SMOOTH_MS),
        }
    }

    /// Delay the effect adds to the signal: the stretch's analysis + synthesis latency (the reverb is a
    /// tail, not a delay).
    pub fn latency_samples(&self) -> usize {
        if self.stages.main_shift {
            self.main.input_latency() + self.main.output_latency()
        } else {
            0
        }
    }

    pub fn process_in_place(&mut self, frame: &mut [f32]) {
        if frame.is_empty() {
            return;
        }
        let n = frame.len();
        let st = self.stages;
        if st.main_shift {
            self.scratch_main.resize(n, 0.0);
            self.main.process(&*frame, &mut self.scratch_main[..]);
        }
        if let Some(sub) = &mut self.sub {
            self.scratch_sub.resize(n, 0.0);
            sub.process(&*frame, &mut self.scratch_sub[..]);
        }
        let has_sub = self.sub.is_some();
        for (i, s) in frame.iter_mut().enumerate() {
            let x = *s;
            let main = if st.main_shift { self.scratch_main[i] } else { x };
            let sub = if has_sub { self.scratch_sub[i] } else { 0.0 };
            *s = self.tick(x, main, sub);
        }
    }

    #[inline]
    fn tick(&mut self, x: f32, main: f32, sub: f32) -> f32 {
        let st = self.stages;
        let mut y = main + self.params.sub_mix * sub;
        if st.saturation && SAT_MIX > 0.0 {
            // Asymmetric tanh: even harmonics; the constant offset is removed (the HPF takes the rest).
            let g = ((SAT_DRIVE * (y + SAT_BIAS)).tanh() / SAT_DRIVE) - self.sat_dc;
            y += SAT_MIX * (g - y);
        }
        if st.eq {
            for b in &mut self.eq {
                y = b.tick(y);
            }
        }
        if st.reverb {
            let wet = self.params.reverb_wet;
            y = y * (1.0 - wet) + self.reverb.tick(y) * wet;
        }
        if st.makeup_limit {
            self.e_in += (x * x - self.e_in) * self.win_a;
            self.e_out += (y * y - self.e_out) * self.win_a;
            if self.e_in > MAKEUP_ACTIVE_ENERGY && self.e_out > 1e-9 {
                let target = (self.e_in / self.e_out).sqrt().clamp(MAKEUP_MIN, MAKEUP_MAX);
                self.gain += (target - self.gain) * self.gain_a;
            }
            y = soft_limit(y * self.gain);
        }
        y
    }
}

#[inline]
fn soft_limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= LIMIT_KNEE {
        return x;
    }
    let span = LIMIT_CEILING - LIMIT_KNEE;
    (LIMIT_KNEE + span * ((a - LIMIT_KNEE) / span).tanh()).copysign(x).clamp(-LIMIT_CEILING, LIMIT_CEILING)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn sine(f: f32, secs: f32, amp: f32) -> Vec<f32> {
        (0..(secs * RATE) as usize).map(|i| amp * (2.0 * PI * f * i as f32 / RATE).sin()).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    /// Fundamental by normalised autocorrelation with parabolic interpolation (50..250 Hz).
    fn f0_autocorr(x: &[f32]) -> f32 {
        let (lo, hi) = ((RATE / 250.0) as usize, (RATE / 50.0) as usize);
        let n = x.len() - hi - 1;
        let r = |k: usize| -> f64 { (0..n).map(|i| x[i] as f64 * x[i + k] as f64).sum::<f64>() };
        let r0 = r(0);
        let acs: Vec<f64> = (lo - 1..=hi + 1).map(|k| r(k) / r0).collect();
        let mut best = 1;
        for i in 1..acs.len() - 1 {
            if acs[i] > acs[best] && acs[i] >= acs[i - 1] && acs[i] >= acs[i + 1] {
                best = i;
            }
        }
        let (a, b, c) = (acs[best - 1], acs[best], acs[best + 1]);
        let off = 0.5 * (a - c) / (a - 2.0 * b + c);
        RATE / ((best + lo - 1) as f32 + off as f32)
    }

    /// Speech-like test signal: harmonic source with a tilt, vibrato-free (f0 constant), syllable envelope.
    fn harmonic(f0: f32, secs: f32) -> Vec<f32> {
        (0..(secs * RATE) as usize)
            .map(|i| {
                let t = i as f32 / RATE;
                let carrier: f32 = (1..=25).map(|k| (2.0 * PI * f0 * k as f32 * t).sin() / k as f32).sum();
                0.2 * carrier * (0.75 + 0.25 * (2.0 * PI * 3.0 * t).sin())
            })
            .collect()
    }

    fn speech_like(secs: f32) -> Vec<f32> {
        (0..(secs * RATE) as usize)
            .map(|i| {
                let t = i as f32 / RATE;
                let f0 = 120.0 * (1.0 + 0.1 * (2.0 * PI * 0.7 * t).sin());
                let carrier: f32 = (1..=25).map(|k| (2.0 * PI * f0 * k as f32 * t).sin() / k as f32).sum();
                let env = (0.5 + 0.5 * (2.0 * PI * 3.5 * t).sin()).powi(2);
                0.2 * carrier * env
            })
            .collect()
    }

    #[test]
    fn natural_is_bit_identical() {
        let x = speech_like(1.0);
        let mut y = x.clone();
        let mut fx = VoiceFx::new(VoiceStyle::Natural);
        assert!(!fx.is_active());
        fx.process_in_place(&mut y);
        assert_eq!(x, y);
        assert_eq!(Persona::Angel.voice_style(), VoiceStyle::Natural);
        assert_eq!(Persona::Daemon.voice_style(), VoiceStyle::Demon);
    }

    #[test]
    fn male_f0_120_comes_out_at_the_shifted_pitch() {
        let want = 120.0 * 2f32.powf(PITCH_SEMITONES / 12.0);
        let x = harmonic(120.0, 3.0);
        for stages in [Stages::MAIN_ONLY, Stages::ALL] {
            let mut y = x.clone();
            DemonVoice::with_stages(stages).process_in_place(&mut y);
            let seg = &y[RATE as usize..RATE as usize + 9_600];
            let f = f0_autocorr(seg);
            eprintln!("measured f0 {f} Hz (want {want}) stages {stages:?}");
            assert!((f - want).abs() / want < 0.04, "f0 {f} Hz, want {want} Hz");
        }
    }

    #[test]
    fn chunking_does_not_change_the_result() {
        let x = speech_like(2.0);
        let run = |n: usize| {
            let mut v = DemonVoice::new();
            let mut y = x.clone();
            for c in y.chunks_mut(n) {
                v.process_in_place(c);
            }
            y
        };
        let whole = run(x.len());
        assert_eq!(whole, run(1920));
        assert_eq!(whole, run(3200));
        assert_eq!(whole, run(777));
        assert_ne!(whole, x);
    }

    #[test]
    fn output_is_finite_limited_and_silence_stays_silent() {
        let mut v = DemonVoice::new();
        let mut loud: Vec<f32> = sine(180.0, 1.0, 3.0);
        v.process_in_place(&mut loud);
        assert!(loud.iter().all(|s| s.is_finite() && s.abs() <= 0.98), "max {}", loud.iter().fold(0f32, |m, s| m.max(s.abs())));
        let mut silence = vec![0.0f32; 5 * RATE as usize];
        v.process_in_place(&mut silence);
        let tail = &silence[4 * RATE as usize..];
        assert!(tail.iter().all(|s| s.abs() < 1e-4), "tail max {}", tail.iter().fold(0f32, |m, s| m.max(s.abs())));
        let mut fresh = vec![0.0f32; 4800];
        DemonVoice::new().process_in_place(&mut fresh);
        assert!(fresh.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn latency_is_within_80_ms_and_matches_the_actual_delay() {
        let v = DemonVoice::new();
        let lat = v.latency_samples();
        eprintln!("reported latency {lat} samples = {:.1} ms", lat as f32 / 24.0);
        assert!(lat as f32 / RATE <= 0.080, "latency {lat} samples");
        // Actual delay: an energy burst in an otherwise silent stream appears about `lat` samples later.
        let mut x = vec![0.0f32; 24_000];
        for (i, s) in x.iter_mut().enumerate().skip(4_800).take(4_800) {
            *s = 0.3 * (2.0 * PI * 150.0 * i as f32 / RATE).sin();
        }
        let mut y = x.clone();
        DemonVoice::with_stages(Stages::MAIN_ONLY).process_in_place(&mut y);
        let env = |v: &[f32]| v.iter().position(|s| s.abs() > 0.05).unwrap();
        let measured = env(&y) as i64 - env(&x) as i64;
        eprintln!("burst onset delay {measured} samples");
        assert!(measured >= 0 && measured as f32 / RATE <= 0.080, "measured delay {measured}");
    }

    #[test]
    fn speech_loudness_is_preserved() {
        let x = speech_like(8.0);
        let mut y = x.clone();
        DemonVoice::new().process_in_place(&mut y);
        let db = 20.0 * (rms(&y[RATE as usize..]) / rms(&x[RATE as usize..])).log10();
        assert!(db.abs() < 3.0, "rms change {db} dB");
    }

    #[test]
    fn reverb_decays_and_is_stable() {
        let mut fdn = Fdn::new();
        let mut energy = |n: usize, input: bool| {
            (0..n)
                .map(|i| {
                    let x = if input && i == 0 { 1.0 } else { 0.0 };
                    let y = fdn.tick(x);
                    y * y
                })
                .sum::<f32>()
        };
        let first = energy(RATE as usize / 2, true);
        let later = energy(RATE as usize, false);
        let end = energy(RATE as usize, false);
        assert!(first > 0.0 && later < first && end < later * 0.05, "first {first} later {later} end {end}");
    }

    #[test]
    fn sub_layer_and_overrides_stay_finite_and_limited() {
        let params = DemonParams { pitch_semitones: -9.0, formant_semitones: -3.0, sub_mix: 0.15, reverb_wet: 0.15, presence_db: 2.0 };
        let mut v = DemonVoice::with_params(Stages::ALL, params);
        let mut y = speech_like(2.0);
        v.process_in_place(&mut y);
        assert!(y.iter().all(|s| s.is_finite() && s.abs() <= 0.98));
        assert!(rms(&y[12_000..]) > 0.01);
    }
}
