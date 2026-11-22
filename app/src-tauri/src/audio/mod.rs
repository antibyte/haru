//! Audio I/O (Tasks 1.4-1.6).

pub mod aec;
pub mod capture;
pub mod devices;
pub mod echo_sim;
pub mod engine;
pub mod features;
pub mod playback;
pub mod resample;
pub mod voice_fx;
#[cfg(test)]
pub(crate) mod test_signals;
