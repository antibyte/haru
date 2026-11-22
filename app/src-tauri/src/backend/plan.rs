//! Attempt plan and fallback order (Task 1.3).
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::devices::{Device, DeviceKind};
use super::variants::{Variant, VariantExe};
use crate::error::{ErrorCode, HaruError};
use crate::messages::MsgKey;
use crate::settings::Settings;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overrides {
    pub backend: Option<Variant>,
    pub device: Option<String>,
}

impl Overrides {
    /// `HARU_BACKEND` / `HARU_DEVICE` beat the settings; "auto" (or empty) means no override.
    pub fn resolve(env: &dyn Fn(&str) -> Option<String>, s: &Settings) -> Overrides {
        let pick = |key: &str, setting: &str| -> Option<String> {
            let raw = env(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).unwrap_or_else(|| setting.trim().to_string());
            (!raw.is_empty() && !raw.eq_ignore_ascii_case("auto")).then_some(raw)
        };
        Overrides {
            backend: pick("HARU_BACKEND", &s.backend).and_then(|b| Variant::parse(&b)),
            device: pick("HARU_DEVICE", &s.device),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attempt {
    pub variant: Variant,
    pub exe: PathBuf,
    pub device: String,
    pub description: String,
}

/// Smallest device memory Haru accepts for automatic selection: 7 GiB, because some 8 GB cards
/// report less than 7.5 GiB over Vulkan; anything below is a 6 GB class card.
pub const MIN_GPU_MEMORY: u64 = 7 * 1024 * 1024 * 1024;

/// A device automatic selection may pick: not the CPU and with enough memory.
pub fn qualifies(d: &Device) -> bool {
    d.kind != DeviceKind::Cpu && d.memory_total >= MIN_GPU_MEMORY
}

fn largest<'a>(devs: &'a [Device], kind: impl Fn(DeviceKind) -> bool) -> Option<&'a Device> {
    // max_by_key returns the last maximum; iterate reversed so ties keep the first listed device.
    devs.iter().rev().filter(|d| kind(d.kind) && qualifies(d)).max_by_key(|d| d.memory_total)
}

/// Why the chosen attempt deserves a warning beyond the plain CPU one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeviceNote {
    Cpu,
    CpuNoCapableGpu,
    SmallGpu,
}

pub fn device_note(a: &Attempt, devices: &HashMap<Variant, Vec<Device>>, ov: &Overrides) -> Option<DeviceNote> {
    let is_cpu = a.variant == Variant::Cpu || a.device.eq_ignore_ascii_case("cpu");
    if is_cpu {
        let explicit = ov.backend == Some(Variant::Cpu) || ov.device.as_deref().is_some_and(|d| d.eq_ignore_ascii_case("cpu"));
        let capable = devices.values().flatten().any(qualifies);
        return Some(if !explicit && !capable { DeviceNote::CpuNoCapableGpu } else { DeviceNote::Cpu });
    }
    let d = devices.get(&a.variant)?.iter().find(|d| d.name.eq_ignore_ascii_case(&a.device))?;
    (!qualifies(d)).then_some(DeviceNote::SmallGpu)
}

fn attempt_for(v: &VariantExe, d: Option<&Device>) -> Attempt {
    match d {
        Some(d) => Attempt { variant: v.variant, exe: v.exe.clone(), device: d.name.clone(), description: d.description.clone() },
        None => Attempt { variant: v.variant, exe: v.exe.clone(), device: "auto".into(), description: "auto".into() },
    }
}

fn cpu_attempt(variants: &[VariantExe]) -> Option<Attempt> {
    let v = variants.iter().find(|v| v.variant == Variant::Cpu).or_else(|| variants.first())?;
    Some(Attempt { variant: v.variant, exe: v.exe.clone(), device: "cpu".into(), description: "CPU".into() })
}

/// The automatic device choice for one variant. None = skip this variant.
fn auto_attempt(
    variant: Variant,
    variants: &[VariantExe],
    devices: &HashMap<Variant, Vec<Device>>,
) -> Option<Attempt> {
    if variant == Variant::Cpu {
        return cpu_attempt(variants);
    }
    let v = variants.iter().find(|v| v.variant == variant)?;
    let Some(devs) = devices.get(&variant) else {
        return Some(attempt_for(v, None));
    };
    let pick = match variant {
        Variant::Cuda => largest(devs, |k| k == DeviceKind::Gpu),
        Variant::Vulkan => largest(devs, |k| k == DeviceKind::Gpu).or_else(|| largest(devs, |k| k == DeviceKind::Igpu)),
        Variant::Metal => largest(devs, |k| k != DeviceKind::Cpu),
        Variant::Cpu => unreachable!(),
    };
    pick.map(|d| attempt_for(v, Some(d)))
}

pub fn choose_plan(
    cuda_ok: bool,
    variants: &[VariantExe],
    devices: &HashMap<Variant, Vec<Device>>,
    ov: &Overrides,
) -> Vec<Attempt> {
    let mut plan: Vec<Attempt> = Vec::new();
    if variants.is_empty() {
        return plan;
    }
    let find_listed = |variant: Variant, name: &str| -> Option<Device> {
        devices.get(&variant)?.iter().find(|d| d.name.eq_ignore_ascii_case(name)).cloned()
    };

    // 1. Overrides.
    match (&ov.backend, &ov.device) {
        (Some(b), dev) => {
            let available = *b == Variant::Cpu || variants.iter().any(|v| v.variant == *b);
            if available {
                let chosen = match dev {
                    Some(name) if *b != Variant::Cpu => variants.iter().find(|v| v.variant == *b).and_then(|v| {
                        find_listed(*b, name).map(|d| attempt_for(v, Some(&d)))
                    }),
                    _ => None,
                };
                if let Some(a) = chosen.or_else(|| auto_attempt(*b, variants, devices)) {
                    plan.push(a);
                }
            }
        }
        (None, Some(name)) => {
            for v in variants {
                if let Some(d) = find_listed(v.variant, name) {
                    plan.push(attempt_for(v, Some(&d)));
                    break;
                }
            }
        }
        (None, None) => {}
    }

    // 2-5. Default order.
    if cuda_ok {
        plan.extend(auto_attempt(Variant::Cuda, variants, devices));
    }
    plan.extend(auto_attempt(Variant::Vulkan, variants, devices));
    plan.extend(auto_attempt(Variant::Metal, variants, devices));
    plan.extend(auto_attempt(Variant::Cpu, variants, devices));

    let mut seen = HashSet::new();
    plan.retain(|a| seen.insert((a.exe.clone(), a.device.to_lowercase())));
    plan
}

/// Tries the plan in order. Stops at the first Ok; a non-backend failure aborts at once.
pub fn run_attempts<T>(
    plan: &[Attempt],
    mut try_one: impl FnMut(&Attempt) -> Result<T, HaruError>,
) -> Result<(Attempt, T), HaruError> {
    let mut lines: Vec<String> = Vec::new();
    for a in plan {
        match try_one(a) {
            Ok(v) => return Ok((a.clone(), v)),
            Err(e) if !e.backend_failure => return Err(e),
            Err(e) => lines.push(format!("{}/{}: {}", a.variant.as_str(), a.device, e.message)),
        }
    }
    Err(HaruError::new(ErrorCode::Worker, MsgKey::NoBackendStarted).with_detail(lines.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str, desc: &str, kind: DeviceKind, mem: u64) -> Device {
        Device { name: name.into(), description: desc.into(), kind, memory_total: mem, memory_free: mem }
    }
    fn uhd() -> Device {
        dev("Vulkan0", "Intel(R) UHD Graphics 770", DeviceKind::Igpu, 16_000_000_000)
    }
    fn b580() -> Device {
        dev("Vulkan1", "Intel(R) Arc(TM) B580 Graphics", DeviceKind::Gpu, 12_000_000_000)
    }
    fn cpu() -> Device {
        dev("CPU", "CPU", DeviceKind::Cpu, 0)
    }
    fn ve(v: Variant) -> VariantExe {
        VariantExe { variant: v, exe: PathBuf::from(format!("/b/{}/speech", v.as_str())) }
    }
    fn vs(list: &[Variant]) -> Vec<VariantExe> {
        list.iter().map(|v| ve(*v)).collect()
    }
    fn map(list: Vec<(Variant, Vec<Device>)>) -> HashMap<Variant, Vec<Device>> {
        list.into_iter().collect()
    }
    /// "variant(exe variant)/device" strings; exe variant shown only when it differs.
    fn show(plan: &[Attempt]) -> Vec<String> {
        plan.iter()
            .map(|a| {
                let exe_variant = a.exe.parent().unwrap().file_name().unwrap().to_string_lossy().into_owned();
                if exe_variant == a.variant.as_str() {
                    format!("{}/{}", a.variant.as_str(), a.device)
                } else {
                    format!("{}({})/{}", a.variant.as_str(), exe_variant, a.device)
                }
            })
            .collect()
    }
    fn none() -> Overrides {
        Overrides::default()
    }

    #[test]
    fn vulkan_only_picks_discrete_over_integrated() {
        let p = choose_plan(false, &vs(&[Variant::Vulkan]), &map(vec![(Variant::Vulkan, vec![uhd(), b580(), cpu()])]), &none());
        assert_eq!(show(&p), ["vulkan/Vulkan1", "vulkan/cpu"]);
        assert_eq!(p[0].description, "Intel(R) Arc(TM) B580 Graphics");
    }

    #[test]
    fn selection_is_by_kind_not_by_name() {
        // This machine: Vulkan0 = discrete, Vulkan1 = integrated.
        let arc = dev("Vulkan0", "Intel(R) Arc(TM) B580 Graphics", DeviceKind::Gpu, 12_000_000_000);
        let igpu = dev("Vulkan1", "Intel(R) UHD Graphics", DeviceKind::Igpu, 16_000_000_000);
        let p = choose_plan(false, &vs(&[Variant::Vulkan]), &map(vec![(Variant::Vulkan, vec![arc, igpu, cpu()])]), &none());
        assert_eq!(show(&p), ["vulkan/Vulkan0", "vulkan/cpu"]);
    }

    #[test]
    fn vulkan_only_integrated_when_no_discrete() {
        let p = choose_plan(false, &vs(&[Variant::Vulkan]), &map(vec![(Variant::Vulkan, vec![uhd(), cpu()])]), &none());
        assert_eq!(show(&p), ["vulkan/Vulkan0", "vulkan/cpu"]);
    }

    fn cuda_vulkan_cpu() -> (Vec<VariantExe>, HashMap<Variant, Vec<Device>>) {
        (
            vs(&[Variant::Cuda, Variant::Vulkan, Variant::Cpu]),
            map(vec![
                (
                    Variant::Cuda,
                    vec![dev("CUDA0", "A", DeviceKind::Gpu, 8_000_000_000), dev("CUDA1", "B", DeviceKind::Gpu, 24_000_000_000)],
                ),
                (
                    Variant::Vulkan,
                    vec![dev("Vulkan0", "A", DeviceKind::Gpu, 24_000_000_000), dev("Vulkan1", "B", DeviceKind::Gpu, 8_000_000_000)],
                ),
            ]),
        )
    }

    #[test]
    fn cuda_first_when_probe_ok() {
        let (v, d) = cuda_vulkan_cpu();
        assert_eq!(show(&choose_plan(true, &v, &d, &none())), ["cuda/CUDA1", "vulkan/Vulkan0", "cpu/cpu"]);
    }

    #[test]
    fn cuda_skipped_when_probe_fails() {
        let (v, d) = cuda_vulkan_cpu();
        assert_eq!(show(&choose_plan(false, &v, &d, &none())), ["vulkan/Vulkan0", "cpu/cpu"]);
    }

    #[test]
    fn cuda_skipped_when_variant_missing() {
        let p = choose_plan(true, &vs(&[Variant::Vulkan]), &map(vec![(Variant::Vulkan, vec![b580()])]), &none());
        assert_eq!(show(&p), ["vulkan/Vulkan1", "vulkan/cpu"]);
    }

    #[test]
    fn override_backend_and_device_first() {
        let ov = Overrides { backend: Some(Variant::Vulkan), device: Some("Vulkan0".into()) };
        let p = choose_plan(false, &vs(&[Variant::Vulkan]), &map(vec![(Variant::Vulkan, vec![uhd(), b580()])]), &ov);
        assert_eq!(show(&p), ["vulkan/Vulkan0", "vulkan/Vulkan1", "vulkan/cpu"]);
    }

    #[test]
    fn override_device_only_case_insensitive() {
        let ov = Overrides { backend: None, device: Some("vulkan0".into()) };
        let p = choose_plan(false, &vs(&[Variant::Vulkan]), &map(vec![(Variant::Vulkan, vec![uhd(), b580()])]), &ov);
        assert_eq!(p[0].device, "Vulkan0");
        assert_eq!(p[0].variant, Variant::Vulkan);
    }

    #[test]
    fn override_backend_cpu_first_without_duplicate() {
        let ov = Overrides { backend: Some(Variant::Cpu), device: None };
        let p = choose_plan(false, &vs(&[Variant::Vulkan]), &map(vec![(Variant::Vulkan, vec![b580()])]), &ov);
        assert_eq!(show(&p), ["vulkan/cpu", "vulkan/Vulkan1"]);
    }

    #[test]
    fn override_unavailable_variant_ignored() {
        let ov = Overrides { backend: Some(Variant::Cuda), device: None };
        let p = choose_plan(false, &vs(&[Variant::Vulkan]), &map(vec![(Variant::Vulkan, vec![b580()])]), &ov);
        assert_eq!(show(&p), ["vulkan/Vulkan1", "vulkan/cpu"]);
    }

    #[test]
    fn no_variants_empty_plan() {
        assert!(choose_plan(true, &[], &HashMap::new(), &none()).is_empty());
    }

    #[test]
    fn metal_variant() {
        let m = dev("MTL0", "Apple GPU", DeviceKind::Gpu, 16_000_000_000);
        let p = choose_plan(false, &vs(&[Variant::Metal, Variant::Cpu]), &map(vec![(Variant::Metal, vec![m])]), &none());
        assert_eq!(show(&p), ["metal/MTL0", "cpu/cpu"]);
    }

    #[test]
    fn missing_device_list_uses_auto() {
        let p = choose_plan(false, &vs(&[Variant::Vulkan]), &HashMap::new(), &none());
        assert_eq!(show(&p), ["vulkan/auto", "vulkan/cpu"]);
        assert_eq!(p[0].description, "auto");
    }

    #[test]
    fn env_overrides_settings() {
        let s = Settings { backend: "cpu".into(), device: "Vulkan1".into(), ..Default::default() };
        let env = |k: &str| match k {
            "HARU_BACKEND" => Some("vulkan".to_string()),
            "HARU_DEVICE" => Some("Vulkan0".to_string()),
            _ => None,
        };
        let ov = Overrides::resolve(&env, &s);
        assert_eq!(ov, Overrides { backend: Some(Variant::Vulkan), device: Some("Vulkan0".into()) });
        let ov = Overrides::resolve(&|_| None, &s);
        assert_eq!(ov, Overrides { backend: Some(Variant::Cpu), device: Some("Vulkan1".into()) });
    }

    #[test]
    fn auto_means_none() {
        let ov = Overrides::resolve(&|_| None, &Settings::default());
        assert_eq!(ov, Overrides::default());
        let ov = Overrides::resolve(&|k| (k == "HARU_BACKEND").then(|| "AUTO".to_string()), &Settings::default());
        assert_eq!(ov, Overrides::default());
    }

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn auto_skips_gpu_below_7_gib() {
        let small = dev("Vulkan0", "Small", DeviceKind::Gpu, 6 * GIB);
        let p = choose_plan(false, &vs(&[Variant::Vulkan, Variant::Cpu]), &map(vec![(Variant::Vulkan, vec![small])]), &none());
        assert_eq!(show(&p), ["cpu/cpu"]);
        let p = choose_plan(false, &vs(&[Variant::Vulkan]), &map(vec![(Variant::Vulkan, vec![dev("Vulkan0", "Small", DeviceKind::Gpu, 6 * GIB)])]), &none());
        assert_eq!(show(&p), ["vulkan/cpu"]);
    }

    #[test]
    fn auto_accepts_7_gib_and_8e9_bytes() {
        for mem in [7 * GIB, 8_000_000_000] {
            let d = dev("Vulkan0", "Card", DeviceKind::Gpu, mem);
            let p = choose_plan(false, &vs(&[Variant::Vulkan]), &map(vec![(Variant::Vulkan, vec![d])]), &none());
            assert_eq!(show(&p), ["vulkan/Vulkan0", "vulkan/cpu"]);
        }
    }

    #[test]
    fn explicit_small_device_is_respected() {
        let small = dev("Vulkan0", "Small", DeviceKind::Gpu, 6 * GIB);
        let ov = Overrides { backend: None, device: Some("Vulkan0".into()) };
        let p = choose_plan(false, &vs(&[Variant::Vulkan]), &map(vec![(Variant::Vulkan, vec![small])]), &ov);
        assert_eq!(show(&p)[0], "vulkan/Vulkan0");
    }

    fn note_for(plan_idx: usize, v: &[VariantExe], d: &HashMap<Variant, Vec<Device>>, ov: &Overrides) -> Option<DeviceNote> {
        let p = choose_plan(false, v, d, ov);
        device_note(&p[plan_idx], d, ov)
    }

    #[test]
    fn device_note_cpu_without_capable_gpu() {
        let d = map(vec![(Variant::Vulkan, vec![dev("Vulkan0", "Small", DeviceKind::Gpu, 6 * GIB), cpu()])]);
        assert_eq!(note_for(0, &vs(&[Variant::Vulkan, Variant::Cpu]), &d, &none()), Some(DeviceNote::CpuNoCapableGpu));
    }

    #[test]
    fn device_note_cpu_explicit_keeps_plain_warning() {
        let d = map(vec![(Variant::Vulkan, vec![dev("Vulkan0", "Small", DeviceKind::Gpu, 6 * GIB)])]);
        let ov = Overrides { backend: Some(Variant::Cpu), device: None };
        assert_eq!(note_for(0, &vs(&[Variant::Vulkan, Variant::Cpu]), &d, &ov), Some(DeviceNote::Cpu));
        let ov = Overrides { backend: None, device: Some("CPU".into()) };
        let a = att(Variant::Cpu, "cpu");
        assert_eq!(device_note(&a, &d, &ov), Some(DeviceNote::Cpu));
    }

    #[test]
    fn device_note_cpu_after_capable_gpu_failed_is_plain() {
        let d = map(vec![(Variant::Vulkan, vec![b580()])]);
        let a = att(Variant::Cpu, "cpu");
        assert_eq!(device_note(&a, &d, &none()), Some(DeviceNote::Cpu));
    }

    #[test]
    fn device_note_small_explicit_gpu() {
        let small = dev("Vulkan0", "Small", DeviceKind::Gpu, 6 * GIB);
        let d = map(vec![(Variant::Vulkan, vec![small])]);
        let ov = Overrides { backend: None, device: Some("Vulkan0".into()) };
        assert_eq!(note_for(0, &vs(&[Variant::Vulkan]), &d, &ov), Some(DeviceNote::SmallGpu));
    }

    #[test]
    fn device_note_auto_large_gpu_is_none() {
        let d = map(vec![(Variant::Vulkan, vec![b580()])]);
        assert_eq!(note_for(0, &vs(&[Variant::Vulkan]), &d, &none()), None);
    }

    #[test]
    fn device_note_unlisted_auto_attempt_is_none() {
        let a = att(Variant::Vulkan, "auto");
        assert_eq!(device_note(&a, &HashMap::new(), &none()), None);
    }

    fn att(v: Variant, dev: &str) -> Attempt {
        Attempt { variant: v, exe: PathBuf::from("x"), device: dev.into(), description: dev.into() }
    }
    fn plan3() -> Vec<Attempt> {
        vec![att(Variant::Cuda, "CUDA0"), att(Variant::Vulkan, "Vulkan0"), att(Variant::Cpu, "cpu")]
    }

    #[test]
    fn first_success_wins() {
        let mut calls = 0;
        let (a, v) = run_attempts(&plan3(), |_| {
            calls += 1;
            Ok(7)
        })
        .unwrap();
        assert_eq!((calls, v, a.device.as_str()), (1, 7, "CUDA0"));
    }

    #[test]
    fn falls_through_backend_failures() {
        let (a, v) = run_attempts(&plan3(), |a| {
            if a.variant == Variant::Cpu {
                Ok("ok")
            } else {
                Err(HaruError::new(ErrorCode::Worker, MsgKey::WorkerStartFailed).backend_failure())
            }
        })
        .unwrap();
        assert_eq!((a.variant, v), (Variant::Cpu, "ok"));
    }

    #[test]
    fn stops_on_config_error() {
        let mut calls = 0;
        let e = run_attempts::<()>(&plan3(), |_| {
            calls += 1;
            Err(HaruError::new(ErrorCode::Settings, MsgKey::LanguageUnsupported))
        })
        .unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(e.code, ErrorCode::Settings);
    }

    #[test]
    fn all_failed_collects_messages() {
        let e = run_attempts::<()>(&plan3(), |_| {
            Err(HaruError::new(ErrorCode::Worker, MsgKey::WorkerStartFailed).backend_failure())
        })
        .unwrap_err();
        assert_eq!(e.code, ErrorCode::Worker);
        assert_eq!(e.key, MsgKey::NoBackendStarted);
        assert_eq!(e.detail.unwrap(), "cuda/CUDA0: Der Sprachmodell-Worker konnte nicht gestartet werden.\nvulkan/Vulkan0: Der Sprachmodell-Worker konnte nicht gestartet werden.\ncpu/cpu: Der Sprachmodell-Worker konnte nicht gestartet werden.");
    }
}
