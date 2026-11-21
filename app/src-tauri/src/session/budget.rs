//! Conversation length from free device memory (the KV cache grows with every second of dialogue).
use std::path::{Path, PathBuf};

use crate::backend::devices::{query_devices, Device, DeviceKind};
use crate::backend::vram;
use crate::error::HaruError;

/// KV-cache bytes one second of dialogue needs (estimate; the E2E check measures the real value).
pub const KV_BYTES_PER_SEC: u64 = 500_000;
pub const GPU_MARGIN: u64 = 512 << 20;
pub const CPU_MARGIN: u64 = 1 << 30;
pub const MIN_SECONDS: u32 = 120;
pub const MAX_SECONDS: u32 = 3600;
pub const FALLBACK_SECONDS: u32 = 600;
/// Debug override: seconds per segment (clamped to 10..=3600).
pub const OVERRIDE_ENV: &str = "HARU_MAX_SECONDS";
const OVERRIDE_MIN: f64 = 10.0;

pub fn margin_for(kind: DeviceKind) -> u64 {
    if kind == DeviceKind::Cpu {
        CPU_MARGIN
    } else {
        GPU_MARGIN
    }
}

/// Raw seconds before clamping: `floor((free - margin) / KV_BYTES_PER_SEC)`, 0 when below the margin.
fn raw_seconds(memory_free: u64, kind: DeviceKind) -> u64 {
    memory_free.saturating_sub(margin_for(kind)) / KV_BYTES_PER_SEC
}

/// clamp(floor((free - margin) / KV_BYTES_PER_SEC), 120, 3600).
pub fn max_seconds_for(memory_free: u64, kind: DeviceKind) -> u32 {
    raw_seconds(memory_free, kind).clamp(MIN_SECONDS as u64, MAX_SECONDS as u64) as u32
}

/// A usable `HARU_MAX_SECONDS` value: finite and > 0, clamped to 10..=3600.
pub fn override_seconds(raw: Option<&str>) -> Option<f64> {
    let v: f64 = raw?.trim().parse().ok()?;
    (v.is_finite() && v > 0.0).then(|| v.clamp(OVERRIDE_MIN, MAX_SECONDS as f64))
}

/// Finds a device by name (case-insensitive); "cpu" also matches a device of kind Cpu.
pub fn find_device<'a>(list: &'a [Device], name: &str) -> Option<&'a Device> {
    list.iter().find(|d| d.name.eq_ignore_ascii_case(name)).or_else(|| {
        if name.eq_ignore_ascii_case("cpu") {
            list.iter().find(|d| d.kind == DeviceKind::Cpu)
        } else {
            None
        }
    })
}

/// Which worker executable and device to ask for free memory.
#[derive(Clone, Debug)]
pub struct Probe {
    pub exe: PathBuf,
    pub device: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Override,
    Memory { free: u64, kind: DeviceKind, system_wide: bool },
    Fallback,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Budget {
    pub seconds: f64,
    pub source: Source,
}

impl Budget {
    /// The seconds if they were really measured (not the fallback), to reuse as a later fallback.
    pub fn measured(&self) -> Option<f64> {
        (self.source != Source::Fallback).then_some(self.seconds)
    }

    pub fn describe(&self) -> String {
        match self.source {
            Source::Override => format!("{OVERRIDE_ENV} override -> {} s", self.seconds),
            Source::Memory { free, kind, system_wide } => format!(
                "free {free} B{} - margin {} B = {} s -> {} s",
                if system_wide { " (system-wide)" } else { "" },
                margin_for(kind),
                raw_seconds(free, kind),
                self.seconds
            ),
            Source::Fallback => format!("fallback -> {} s", self.seconds),
        }
    }
}

/// Override > free memory of the probed device (minimum of the device list and, for discrete GPUs, the
/// system-wide figure) > `fallback` (or 600 s).
pub fn measure_with(
    env: &dyn Fn(&str) -> Option<String>,
    query: &dyn Fn(&Path) -> Result<Vec<Device>, HaruError>,
    system_free: &dyn Fn(&str) -> Option<u64>,
    probe: Option<&Probe>,
    fallback: Option<f64>,
) -> Budget {
    if let Some(s) = override_seconds(env(OVERRIDE_ENV).as_deref()) {
        return Budget { seconds: s, source: Source::Override };
    }
    let fb = Budget { seconds: fallback.unwrap_or(FALLBACK_SECONDS as f64), source: Source::Fallback };
    let Some(p) = probe else { return fb };
    match query(&p.exe) {
        Ok(list) => match find_device(&list, &p.device) {
            Some(d) => {
                // The device list comes from a separate probe process, so it misses VRAM used by the worker
                // and other apps; for discrete GPUs the system-wide figure is the truth when available.
                let sys = if d.kind == DeviceKind::Gpu { system_free(&d.description) } else { None };
                let (free, system_wide) = match sys {
                    Some(s) if s < d.memory_free => (s, true),
                    _ => (d.memory_free, false),
                };
                Budget {
                    seconds: max_seconds_for(free, d.kind) as f64,
                    source: Source::Memory { free, kind: d.kind, system_wide },
                }
            }
            None => fb,
        },
        Err(_) => fb,
    }
}

/// `measure_with` on the real environment and `speech devices --json`.
pub fn measure(probe: Option<&Probe>, fallback: Option<f64>) -> Budget {
    measure_with(&|k| std::env::var(k).ok(), &|exe| query_devices(exe), &vram::system_free, probe, fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const GIB: u64 = 1 << 30;

    fn dev(name: &str, kind: DeviceKind, free: u64) -> Device {
        Device { name: name.into(), description: name.into(), kind, memory_total: free, memory_free: free }
    }

    #[test]
    fn gpu_budget_uses_512_mib_margin() {
        let free = GPU_MARGIN + 1000 * KV_BYTES_PER_SEC;
        assert_eq!(max_seconds_for(free, DeviceKind::Gpu), 1000);
        assert_eq!(max_seconds_for(free, DeviceKind::Igpu), 1000);
    }

    #[test]
    fn cpu_budget_uses_1_gib_margin() {
        let free = CPU_MARGIN + 1000 * KV_BYTES_PER_SEC;
        assert_eq!(max_seconds_for(free, DeviceKind::Cpu), 1000);
        assert!(max_seconds_for(free, DeviceKind::Gpu) > 1000);
    }

    #[test]
    fn budget_floors_partial_seconds() {
        let free = GPU_MARGIN + 1000 * KV_BYTES_PER_SEC + KV_BYTES_PER_SEC - 1;
        assert_eq!(max_seconds_for(free, DeviceKind::Gpu), 1000);
    }

    #[test]
    fn budget_clamps_to_120_and_3600() {
        assert_eq!(max_seconds_for(0, DeviceKind::Gpu), 120);
        assert_eq!(max_seconds_for(GPU_MARGIN - 1, DeviceKind::Gpu), 120);
        assert_eq!(max_seconds_for(100_000_000_000, DeviceKind::Gpu), 3600);
    }

    #[test]
    fn override_parses_and_clamps() {
        assert_eq!(override_seconds(Some("30")), Some(30.0));
        assert_eq!(override_seconds(Some(" 45.5 ")), Some(45.5));
        assert_eq!(override_seconds(Some("0")), None);
        assert_eq!(override_seconds(Some("-5")), None);
        assert_eq!(override_seconds(Some("abc")), None);
        assert_eq!(override_seconds(Some("")), None);
        assert_eq!(override_seconds(Some("nan")), None);
        assert_eq!(override_seconds(Some("inf")), None);
        assert_eq!(override_seconds(None), None);
        assert_eq!(override_seconds(Some("5")), Some(10.0));
        assert_eq!(override_seconds(Some("99999")), Some(3600.0));
    }

    #[test]
    fn find_device_by_name_and_cpu_kind() {
        let list = vec![dev("Vulkan0", DeviceKind::Gpu, GIB), dev("CPU", DeviceKind::Cpu, GIB)];
        assert_eq!(find_device(&list, "vulkan0").unwrap().name, "Vulkan0");
        assert_eq!(find_device(&list, "cpu").unwrap().name, "CPU");
        let odd = vec![dev("Host", DeviceKind::Cpu, GIB)];
        assert_eq!(find_device(&odd, "cpu").unwrap().name, "Host");
        assert!(find_device(&list, "Vulkan9").is_none());
    }

    #[test]
    fn measure_prefers_override_without_querying() {
        let calls = Cell::new(0);
        let query = |_: &Path| {
            calls.set(calls.get() + 1);
            Ok(vec![])
        };
        let probe = Probe { exe: "x".into(), device: "Vulkan0".into() };
        let env = |k: &str| (k == OVERRIDE_ENV).then(|| "20".to_string());
        let b = measure_with(&env, &query, &|_| None, Some(&probe), None);
        assert_eq!(b, Budget { seconds: 20.0, source: Source::Override });
        assert_eq!(calls.get(), 0);
        assert_eq!(b.measured(), Some(20.0));
    }

    #[test]
    fn measure_reads_free_memory_of_the_probed_device() {
        let free = GPU_MARGIN + 700 * KV_BYTES_PER_SEC;
        let query = |_: &Path| Ok(vec![dev("Vulkan0", DeviceKind::Gpu, free)]);
        let probe = Probe { exe: "x".into(), device: "Vulkan0".into() };
        let b = measure_with(&|_| None, &query, &|_| None, Some(&probe), None);
        assert_eq!(b.seconds, 700.0);
        assert!(b.describe().contains("-> 700 s"));
    }

    #[test]
    fn measure_uses_fallback_on_query_error_or_missing_device() {
        let probe = Probe { exe: "x".into(), device: "Vulkan0".into() };
        let failing = |_: &Path| Err(HaruError::new(crate::error::ErrorCode::NoBackend, "x"));
        let b = measure_with(&|_| None, &failing, &|_| None, Some(&probe), Some(700.0));
        assert_eq!(b, Budget { seconds: 700.0, source: Source::Fallback });
        assert_eq!(b.measured(), None);
        assert_eq!(measure_with(&|_| None, &failing, &|_| None, Some(&probe), None).seconds, 600.0);
        let other = |_: &Path| Ok(vec![dev("Vulkan1", DeviceKind::Gpu, GIB)]);
        assert_eq!(measure_with(&|_| None, &other, &|_| None, Some(&probe), Some(700.0)).seconds, 700.0);
        assert_eq!(measure_with(&|_| None, &other, &|_| None, None, None).seconds, 600.0);
    }

    fn sys_budget(kind: DeviceKind, json_free: u64, sys: Option<u64>) -> Budget {
        let query = move |_: &Path| Ok(vec![dev("Vulkan0", kind, json_free)]);
        let probe = Probe { exe: "x".into(), device: "Vulkan0".into() };
        measure_with(&|_| None, &query, &move |_| sys, Some(&probe), None)
    }

    #[test]
    fn gpu_uses_minimum_of_json_and_system_free() {
        let json = GPU_MARGIN + 3000 * KV_BYTES_PER_SEC;
        let sys = GPU_MARGIN + 500 * KV_BYTES_PER_SEC;
        let b = sys_budget(DeviceKind::Gpu, json, Some(sys));
        assert_eq!(b.seconds, 500.0);
        assert_eq!(b.source, Source::Memory { free: sys, kind: DeviceKind::Gpu, system_wide: true });
        // A larger system-wide value does not raise the budget.
        let b = sys_budget(DeviceKind::Gpu, sys, Some(json));
        assert_eq!(b.seconds, 500.0);
        assert_eq!(b.source, Source::Memory { free: sys, kind: DeviceKind::Gpu, system_wide: false });
    }

    #[test]
    fn system_free_is_ignored_for_igpu_and_cpu() {
        let json = CPU_MARGIN + 3000 * KV_BYTES_PER_SEC;
        for kind in [DeviceKind::Igpu, DeviceKind::Cpu] {
            let b = sys_budget(kind, json, Some(0));
            assert_eq!(b.source, Source::Memory { free: json, kind, system_wide: false });
        }
    }

    #[test]
    fn falls_back_to_json_when_system_free_is_none() {
        let json = GPU_MARGIN + 900 * KV_BYTES_PER_SEC;
        let b = sys_budget(DeviceKind::Gpu, json, None);
        assert_eq!(b.seconds, 900.0);
        assert_eq!(b.source, Source::Memory { free: json, kind: DeviceKind::Gpu, system_wide: false });
    }

    #[test]
    fn describe_mentions_system_wide_source() {
        let free = GPU_MARGIN + 700 * KV_BYTES_PER_SEC;
        let sys = sys_budget(DeviceKind::Gpu, free + GIB, Some(free)).describe();
        assert!(sys.contains(&format!("free {free} B (system-wide) - margin")), "{sys}");
        let plain = sys_budget(DeviceKind::Gpu, free, None).describe();
        assert!(plain.contains(&format!("free {free} B - margin")), "{plain}");
    }
}
