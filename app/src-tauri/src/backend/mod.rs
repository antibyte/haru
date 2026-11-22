//! Backend detection: variants, devices, CUDA (Task 1.3).

pub mod cuda_probe;
pub mod devices;
pub mod plan;
pub mod variants;
pub mod vram;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{ErrorCode, HaruError};
use crate::messages::MsgKey;
use crate::ipc::{BackendOptions, DeviceInfo};
use devices::Device;
use variants::{Variant, VariantExe};

pub struct Detection {
    pub dir: PathBuf,
    pub variants: Vec<VariantExe>,
    pub devices: HashMap<Variant, Vec<Device>>,
    pub cuda_ok: bool,
}

/// Discovers variants in `dir`, probes CUDA and queries the device list of each variant.
/// The CUDA variant is only queried when the probe succeeded; a failed query leaves no entry.
pub fn detect(dir: Option<&Path>) -> Result<Detection, HaruError> {
    let no_backend = |detail: String| {
        HaruError::new(
            ErrorCode::NoBackend,
            MsgKey::NoBackendBuild,
        )
        .with_detail(detail)
    };
    let Some(dir) = dir else {
        return Err(no_backend("no backends directory found".into()));
    };
    let variants = variants::discover(dir);
    if variants.is_empty() {
        return Err(no_backend(format!("searched: {}", dir.display())));
    }
    let cuda_ok = cuda_probe::probe_cuda();
    let mut devs = HashMap::new();
    for v in &variants {
        if v.variant == Variant::Cuda && !cuda_ok {
            continue;
        }
        match devices::query_devices(&v.exe) {
            Ok(list) => {
                devs.insert(v.variant, list);
            }
            Err(e) => eprintln!("haru: device query for {} failed: {} ({:?})", v.variant.as_str(), e.message, e.detail),
        }
    }
    Ok(Detection { dir: dir.to_path_buf(), variants, devices: devs, cuda_ok })
}

pub fn to_backend_options(d: &Detection) -> BackendOptions {
    let mut devices = Vec::new();
    for v in &d.variants {
        if let Some(list) = d.devices.get(&v.variant) {
            for dev in list {
                devices.push(DeviceInfo {
                    variant: v.variant.as_str().to_string(),
                    name: dev.name.clone(),
                    description: dev.description.clone(),
                    kind: dev.kind.as_str().to_string(),
                    memory_total: dev.memory_total,
                });
            }
        }
    }
    BackendOptions {
        variants: d.variants.iter().map(|v| v.variant.as_str().to_string()).collect(),
        devices,
        cuda_available: d.cuda_ok,
        backends_dir: Some(d.dir.to_string_lossy().into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_without_dir_is_no_backend() {
        let e = detect(None).err().unwrap();
        assert_eq!(e.code, ErrorCode::NoBackend);
    }

    #[test]
    fn detect_empty_dir_is_no_backend_with_detail() {
        let d = tempfile::tempdir().unwrap();
        let e = detect(Some(d.path())).err().unwrap();
        assert_eq!(e.code, ErrorCode::NoBackend);
        assert!(e.detail.unwrap().contains(&d.path().display().to_string()));
    }
}
