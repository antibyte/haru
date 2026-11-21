//! Audio I/O (Tasks 1.4-1.6).

pub mod aec;
pub mod capture;
pub mod devices;
pub mod engine;
pub mod features;
pub mod playback;
pub mod resample;
#[cfg(test)]
pub(crate) mod test_signals;
