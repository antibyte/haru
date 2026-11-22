//! The ORIGINAL (v1) daemon voice chain, kept only so the offline example can render `*-v1.wav`
//! for A/B listening: granular two-tap shifter (-5 st), sub-octave, 37 Hz AM growl, tanh saturation,
//! HPF/low-shelf/high-shelf(-6 dB), 4-comb Schroeder reverb. Not used by the app.
#![allow(dead_code)]

use std::f32::consts::PI;

pub const RATE: f32 = 24_000.0;

// ---- tunables --------------------------------------------------------------------------------------------
/// Main pitch shift in semitones (formants move with the pitch: bigger, hollower).
pub const PITCH_SEMITONES: f32 = -5.0;
/// Granular shifter window (two taps half a window apart, Hann crossfade).
pub const SHIFT_WINDOW_MS: f32 = 46.0;
/// Sub-octave layer ratio and mix level (-10 dB).
pub const SUB_RATIO: f32 = 0.5;
pub const SUB_MIX: f32 = 0.316;
/// Growl: 1 - d + d*|sin(2 pi f t)|, depth faded in by a fast envelope so silence stays silent.
pub const GROWL_HZ: f32 = 37.0;
pub const GROWL_DEPTH: f32 = 0.2;
pub const GROWL_ENV_FULL: f32 = 0.02;
pub const GROWL_ENV_RELEASE_MS: f32 = 20.0;
/// Saturation: tanh(drive * x) / drive (unity small-signal gain).
pub const SAT_DRIVE: f32 = 1.6;
/// EQ (RBJ biquads).
pub const HPF_HZ: f64 = 45.0;
pub const LOW_SHELF_HZ: f64 = 160.0;
pub const LOW_SHELF_DB: f64 = 4.0;
pub const HIGH_SHELF_HZ: f64 = 3000.0;
pub const HIGH_SHELF_DB: f64 = -6.0;
/// Reverb ("stone cellar"): 4 damped combs, 2 all-passes.
pub const REVERB_COMB_MS: [f32; 4] = [31.0, 35.3, 39.7, 43.9];
pub const REVERB_ALLPASS_MS: [f32; 2] = [12.6, 5.0];
pub const REVERB_FEEDBACK: f32 = 0.80;
pub const REVERB_DAMP: f32 = 0.45;
pub const REVERB_WET: f32 = 0.14;
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

/// Which stages are on (everything in production; subsets for tests).
#[derive(Clone, Copy, Debug)]
pub struct Stages {
    pub main_shift: bool,
    pub sub: bool,
    pub growl: bool,
    pub saturation: bool,
    pub eq: bool,
    pub reverb: bool,
    pub makeup_limit: bool,
}

impl Stages {
    pub const ALL: Stages = Stages { main_shift: true, sub: true, growl: true, saturation: true, eq: true, reverb: true, makeup_limit: true };
    pub const MAIN_ONLY: Stages = Stages { main_shift: true, sub: false, growl: false, saturation: false, eq: false, reverb: false, makeup_limit: false };
}

// ---- pitch shifter ---------------------------------------------------------------------------------------

struct Shifter {
    buf: Vec<f32>,
    mask: usize,
    wp: usize,
    window: f32,
    inc: f32,
    phase: f32,
}

impl Shifter {
    fn new(ratio: f32) -> Self {
        let window = (SHIFT_WINDOW_MS * 0.001 * RATE).round();
        let len = (window as usize + 4).next_power_of_two();
        Self { buf: vec![0.0; len], mask: len - 1, wp: 0, window, inc: 1.0 - ratio, phase: 0.0 }
    }

    #[inline]
    fn read(&self, delay: f32) -> f32 {
        let i = delay.floor();
        let f = delay - i;
        let i = i as usize;
        let a = self.buf[self.wp.wrapping_sub(i) & self.mask];
        let b = self.buf[self.wp.wrapping_sub(i + 1) & self.mask];
        a + (b - a) * f
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        self.wp = (self.wp + 1) & self.mask;
        self.buf[self.wp] = x;
        let p1 = self.phase;
        let mut p2 = p1 + self.window * 0.5;
        if p2 >= self.window {
            p2 -= self.window;
        }
        let w1 = 0.5 - 0.5 * (2.0 * PI * p1 / self.window).cos();
        let y = self.read(p1) * w1 + self.read(p2) * (1.0 - w1);
        self.phase += self.inc;
        if self.phase >= self.window {
            self.phase -= self.window;
        }
        y
    }

    fn latency(&self) -> usize {
        (self.window * 0.5).round() as usize
    }
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

    /// RBJ shelf with slope S = 1.
    fn shelf(f0: f64, db: f64, high: bool) -> Self {
        let a = 10f64.powf(db / 40.0);
        let w = 2.0 * std::f64::consts::PI * f0 / RATE as f64;
        let (s, c) = w.sin_cos();
        let al = s / 2.0 * 2f64.sqrt();
        let t = 2.0 * a.sqrt() * al;
        if high {
            Self::from_coefs(
                [a * ((a + 1.0) + (a - 1.0) * c + t), -2.0 * a * ((a - 1.0) + (a + 1.0) * c), a * ((a + 1.0) + (a - 1.0) * c - t)],
                [(a + 1.0) - (a - 1.0) * c + t, 2.0 * ((a - 1.0) - (a + 1.0) * c), (a + 1.0) - (a - 1.0) * c - t],
            )
        } else {
            Self::from_coefs(
                [a * ((a + 1.0) - (a - 1.0) * c + t), 2.0 * a * ((a - 1.0) - (a + 1.0) * c), a * ((a + 1.0) - (a - 1.0) * c - t)],
                [(a + 1.0) + (a - 1.0) * c + t, -2.0 * ((a - 1.0) + (a + 1.0) * c), (a + 1.0) + (a - 1.0) * c - t],
            )
        }
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

// ---- reverb ----------------------------------------------------------------------------------------------

#[inline]
fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 {
        0.0
    } else {
        x
    }
}

struct Comb {
    buf: Vec<f32>,
    pos: usize,
    lp: f32,
}

impl Comb {
    fn new(ms: f32) -> Self {
        Self { buf: vec![0.0; (ms * 0.001 * RATE).round() as usize], pos: 0, lp: 0.0 }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.buf[self.pos];
        self.lp = flush(y * (1.0 - REVERB_DAMP) + self.lp * REVERB_DAMP);
        self.buf[self.pos] = flush(x + self.lp * REVERB_FEEDBACK);
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
        y
    }
}

struct AllPass {
    buf: Vec<f32>,
    pos: usize,
}

impl AllPass {
    fn new(ms: f32) -> Self {
        Self { buf: vec![0.0; (ms * 0.001 * RATE).round() as usize], pos: 0 }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let d = self.buf[self.pos];
        let y = d - x;
        self.buf[self.pos] = flush(x + d * 0.5);
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
        y
    }
}

struct Reverb {
    combs: [Comb; 4],
    allpass: [AllPass; 2],
}

impl Reverb {
    fn new() -> Self {
        Self { combs: REVERB_COMB_MS.map(Comb::new), allpass: REVERB_ALLPASS_MS.map(AllPass::new) }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let mut s = 0.0;
        for c in &mut self.combs {
            s += c.tick(x);
        }
        let mut y = s * 0.25;
        for a in &mut self.allpass {
            y = a.tick(y);
        }
        x * (1.0 - REVERB_WET) + y * REVERB_WET
    }
}

// ---- the daemon voice ------------------------------------------------------------------------------------

pub struct DemonVoice {
    stages: Stages,
    main: Shifter,
    sub: Shifter,
    growl_phase: f32,
    growl_env: f32,
    growl_release: f32,
    eq: [Biquad; 3],
    reverb: Reverb,
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
        let coef = |ms: f32| 1.0 - (-1.0 / (ms * 0.001 * RATE)).exp();
        Self {
            stages,
            main: Shifter::new(2f32.powf(PITCH_SEMITONES / 12.0)),
            sub: Shifter::new(SUB_RATIO),
            growl_phase: 0.0,
            growl_env: 0.0,
            growl_release: (-1.0 / (GROWL_ENV_RELEASE_MS * 0.001 * RATE)).exp(),
            eq: [Biquad::highpass(HPF_HZ), Biquad::shelf(LOW_SHELF_HZ, LOW_SHELF_DB, false), Biquad::shelf(HIGH_SHELF_HZ, HIGH_SHELF_DB, true)],
            reverb: Reverb::new(),
            e_in: 0.0,
            e_out: 0.0,
            win_a: coef(MAKEUP_WINDOW_MS),
            gain: 1.0,
            gain_a: coef(MAKEUP_SMOOTH_MS),
        }
    }

    /// Delay the effect adds to the signal (the shifter's mean delay; the reverb is a tail, not a delay).
    pub fn latency_samples(&self) -> usize {
        if self.stages.main_shift {
            self.main.latency()
        } else {
            0
        }
    }

    pub fn process_in_place(&mut self, frame: &mut [f32]) {
        for s in frame.iter_mut() {
            *s = self.tick(*s);
        }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let st = self.stages;
        let main = self.main.tick(x);
        let sub = self.sub.tick(x);
        let mut y = if st.main_shift { main } else { x };
        if st.sub {
            y += SUB_MIX * sub;
        }
        if st.growl {
            self.growl_env = (self.growl_env * self.growl_release).max(y.abs());
            let depth = GROWL_DEPTH * (self.growl_env / GROWL_ENV_FULL).min(1.0);
            let m = 1.0 - depth + depth * (2.0 * PI * self.growl_phase).sin().abs();
            self.growl_phase += GROWL_HZ / RATE;
            if self.growl_phase >= 1.0 {
                self.growl_phase -= 1.0;
            }
            y *= m;
        }
        if st.saturation {
            y = (SAT_DRIVE * y).tanh() / SAT_DRIVE;
        }
        if st.eq {
            for b in &mut self.eq {
                y = b.tick(y);
            }
        }
        if st.reverb {
            y = self.reverb.tick(y);
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

