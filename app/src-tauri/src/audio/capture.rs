//! Microphone capture (Task 1.5).

use super::aec::{store_latency, LATENCY_UNKNOWN};
use crate::error::{ErrorCode, HaruError};
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

pub struct CaptureHandle {
    pub rate: u32,
    pub consumer: rtrb::Consumer<f32>,
    pub overruns: Arc<AtomicU64>,
    /// Set by the stream error callback (device unplugged etc.).
    pub failed: Arc<AtomicBool>,
    /// Last input latency (callback - capture timestamp) in microseconds; `LATENCY_UNKNOWN` until seen.
    pub input_latency_us: Arc<AtomicU32>,
}

fn err(msg: &str, detail: impl ToString) -> HaruError {
    HaruError::new(ErrorCode::AudioDevice, msg).with_detail(detail.to_string())
}

fn build_typed<T>(
    dev: &cpal::Device,
    config: &cpal::StreamConfig,
    mut producer: rtrb::Producer<f32>,
    overruns: Arc<AtomicU64>,
    failed: Arc<AtomicBool>,
    latency: Arc<AtomicU32>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels.max(1) as usize;
    dev.build_input_stream(
        config,
        move |data: &[T], info: &cpal::InputCallbackInfo| {
            let ts = info.timestamp();
            store_latency(&latency, ts.callback.duration_since(&ts.capture));
            let inv = 1.0 / channels as f32;
            for frame in data.chunks_exact(channels) {
                let mut sum = 0.0f32;
                for &s in frame {
                    sum += <f32 as FromSample<T>>::from_sample_(s);
                }
                if producer.push(sum * inv).is_err() {
                    overruns.fetch_add(1, Ordering::Relaxed);
                }
            }
        },
        move |e| {
            eprintln!("capture stream error: {e}");
            failed.store(true, Ordering::Release);
        },
        None,
    )
}

pub fn build_capture(dev: &cpal::Device) -> Result<(cpal::Stream, CaptureHandle), HaruError> {
    let supported = dev
        .default_input_config()
        .map_err(|e| err("Das Mikrofon konnte nicht geöffnet werden.", e))?;
    let rate = supported.sample_rate().0;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let (producer, consumer) = rtrb::RingBuffer::new(rate as usize * 2);
    let overruns = Arc::new(AtomicU64::new(0));
    let o = overruns.clone();
    let failed = Arc::new(AtomicBool::new(false));
    let latency = Arc::new(AtomicU32::new(LATENCY_UNKNOWN));
    let stream = match format {
        SampleFormat::I8 => build_typed::<i8>(dev, &config, producer, o, failed.clone(), latency.clone()),
        SampleFormat::I16 => build_typed::<i16>(dev, &config, producer, o, failed.clone(), latency.clone()),
        SampleFormat::I32 => build_typed::<i32>(dev, &config, producer, o, failed.clone(), latency.clone()),
        SampleFormat::I64 => build_typed::<i64>(dev, &config, producer, o, failed.clone(), latency.clone()),
        SampleFormat::U8 => build_typed::<u8>(dev, &config, producer, o, failed.clone(), latency.clone()),
        SampleFormat::U16 => build_typed::<u16>(dev, &config, producer, o, failed.clone(), latency.clone()),
        SampleFormat::U32 => build_typed::<u32>(dev, &config, producer, o, failed.clone(), latency.clone()),
        SampleFormat::U64 => build_typed::<u64>(dev, &config, producer, o, failed.clone(), latency.clone()),
        SampleFormat::F32 => build_typed::<f32>(dev, &config, producer, o, failed.clone(), latency.clone()),
        SampleFormat::F64 => build_typed::<f64>(dev, &config, producer, o, failed.clone(), latency.clone()),
        other => {
            return Err(err("Das Mikrofon-Format wird nicht unterstützt.", format!("sample format {other:?}")));
        }
    }
    .map_err(|e| err("Das Mikrofon konnte nicht geöffnet werden.", e))?;
    stream
        .play()
        .map_err(|e| err("Das Mikrofon konnte nicht gestartet werden.", e))?;
    Ok((stream, CaptureHandle { rate, consumer, overruns, failed, input_latency_us: latency }))
}
