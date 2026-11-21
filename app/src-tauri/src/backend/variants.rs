//! Backend variant discovery (Task 1.3).
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Variant {
    Cuda,
    Vulkan,
    Metal,
    Cpu,
}

impl Variant {
    pub const ALL: [Variant; 4] = [Variant::Cuda, Variant::Vulkan, Variant::Metal, Variant::Cpu];

    pub fn as_str(self) -> &'static str {
        match self {
            Variant::Cuda => "cuda",
            Variant::Vulkan => "vulkan",
            Variant::Metal => "metal",
            Variant::Cpu => "cpu",
        }
    }

    pub fn parse(s: &str) -> Option<Variant> {
        Variant::ALL.into_iter().find(|v| v.as_str().eq_ignore_ascii_case(s.trim()))
    }

    pub fn label(self) -> &'static str {
        match self {
            Variant::Cuda => "CUDA",
            Variant::Vulkan => "Vulkan",
            Variant::Metal => "Metal",
            Variant::Cpu => "CPU",
        }
    }
}

pub fn exe_name() -> &'static str {
    if cfg!(windows) {
        "speech.exe"
    } else {
        "speech"
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VariantExe {
    pub variant: Variant,
    pub exe: PathBuf,
}

/// Variants in `Variant::ALL` order; only folders that contain the exe.
pub fn discover(dir: &Path) -> Vec<VariantExe> {
    Variant::ALL
        .into_iter()
        .filter_map(|v| {
            let exe = dir.join(v.as_str()).join(exe_name());
            exe.is_file().then_some(VariantExe { variant: v, exe })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, variant: &str) {
        std::fs::create_dir_all(dir.join(variant)).unwrap();
        std::fs::write(dir.join(variant).join(exe_name()), b"x").unwrap();
    }

    #[test]
    fn discover_finds_variants_in_fixed_order() {
        let d = tempfile::tempdir().unwrap();
        touch(d.path(), "cpu");
        touch(d.path(), "vulkan");
        let found = discover(d.path());
        let vs: Vec<Variant> = found.iter().map(|v| v.variant).collect();
        assert_eq!(vs, vec![Variant::Vulkan, Variant::Cpu]);
        assert_eq!(found[0].exe, d.path().join("vulkan").join(exe_name()));
    }

    #[test]
    fn discover_ignores_dirs_without_exe() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("cuda")).unwrap();
        touch(d.path(), "metal");
        std::fs::create_dir_all(d.path().join("other")).unwrap();
        let vs: Vec<Variant> = discover(d.path()).iter().map(|v| v.variant).collect();
        assert_eq!(vs, vec![Variant::Metal]);
    }

    #[test]
    fn parse_and_labels() {
        assert_eq!(Variant::parse("Vulkan"), Some(Variant::Vulkan));
        assert_eq!(Variant::parse("auto"), None);
        assert_eq!(Variant::Cuda.label(), "CUDA");
        assert_eq!(Variant::Cpu.as_str(), "cpu");
    }
}
