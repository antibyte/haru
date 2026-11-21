//! Cache, model and backends paths, mirroring cache.cpp (Task 1.1).
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::backend::variants::exe_name;
use crate::error::{ErrorCode, HaruError};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Os {
    Windows,
    Linux,
    MacOs,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

pub struct ModelSpec {
    pub repo: &'static str,
    pub revision: &'static str,
    pub file: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

/// Q8_0 is closest to the official model (docs/models/haru.md).
pub const HARU_MODEL: ModelSpec = ModelSpec {
    repo: "sakasegawa/Haru-4B-GGUF",
    revision: "fc026182f7bbfaa0400b2d44358a5e9e327b957f",
    file: "Haru-4B-Q8_0.gguf",
    size: 6_623_102_720,
    sha256: "c372a527e63ca4251429a48863535b8bca03ebd6f414bbf31f28a9d175349934",
};

impl ModelSpec {
    pub fn url(&self, base: &str) -> String {
        format!("{}/{}/resolve/{}/{}", base.trim_end_matches('/'), self.repo, self.revision, self.file)
    }
}

fn path_error(message: &str, detail: String) -> HaruError {
    HaruError::new(ErrorCode::Io, message).with_detail(detail)
}

fn is_absolute_for(os: Os, p: &Path) -> bool {
    match os {
        Os::Windows => p.is_absolute(),
        _ => p.to_string_lossy().starts_with('/'),
    }
}

/// Models folder per speech.cpp's cache.cpp (F16). An empty variable counts as unset.
pub fn models_dir_with(os: Os, env: &dyn Fn(&str) -> Option<OsString>) -> Result<PathBuf, HaruError> {
    let get = |k: &str| env(k).filter(|v| !v.is_empty());
    if let Some(dir) = get("SPEECH_MODEL_DIR") {
        let p = PathBuf::from(dir);
        if !is_absolute_for(os, &p) {
            return Err(path_error(
                "SPEECH_MODEL_DIR muss ein absoluter Pfad sein.",
                format!("SPEECH_MODEL_DIR={}", p.display()),
            ));
        }
        return Ok(p);
    }
    let unset = |var: &str| {
        path_error("Der Modellordner konnte nicht bestimmt werden.", format!("{var} is not set"))
    };
    let root = match os {
        Os::Windows => {
            let base = get("LOCALAPPDATA").ok_or_else(|| unset("LOCALAPPDATA"))?;
            PathBuf::from(base).join("speech.cpp")
        }
        Os::MacOs => {
            let home = get("HOME").ok_or_else(|| unset("HOME"))?;
            PathBuf::from(home).join("Library").join("Caches").join("speech.cpp")
        }
        Os::Linux => {
            let xdg = get("XDG_CACHE_HOME").map(PathBuf::from).filter(|p| is_absolute_for(os, p));
            match xdg {
                Some(x) => x.join("speech.cpp"),
                None => {
                    let home = get("HOME").ok_or_else(|| unset("HOME"))?;
                    PathBuf::from(home).join(".cache").join("speech.cpp")
                }
            }
        }
    };
    Ok(root.join("models"))
}

pub fn models_dir() -> Result<PathBuf, HaruError> {
    models_dir_with(Os::current(), &|k| std::env::var_os(k))
}

/// `<models>/<owner>--<name>/<sha256>/<file>` (F17).
pub fn model_path(models_dir: &Path, spec: &ModelSpec) -> PathBuf {
    models_dir.join(spec.repo.replace('/', "--")).join(spec.sha256).join(spec.file)
}

/// `<file>.part`
pub fn partial_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

fn has_variant_exe(dir: &Path) -> bool {
    let exe = exe_name();
    ["cuda", "vulkan", "metal", "cpu"].iter().any(|v| dir.join(v).join(exe).is_file())
}

/// `HARU_BACKENDS_DIR` (when set) is authoritative: if it holds no `<variant>/speech[.exe]` there is
/// no backend (so the noBackend error can be provoked). Otherwise the first of resource dir and dev
/// dir that contains at least one variant.
pub fn find_backends_dir(
    env_override: Option<PathBuf>,
    resource_dir: Option<PathBuf>,
    dev_dir: Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(d) = env_override {
        return has_variant_exe(&d).then_some(d);
    }
    [resource_dir, dev_dir].into_iter().flatten().find(|d| has_variant_exe(d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).map(OsString::from)
    }

    #[test]
    fn windows_uses_localappdata() {
        let e = env_of(&[("LOCALAPPDATA", r"C:\L")]);
        assert_eq!(
            models_dir_with(Os::Windows, &e).unwrap(),
            PathBuf::from(r"C:\L").join("speech.cpp").join("models")
        );
    }

    #[test]
    fn windows_without_localappdata_errors() {
        let e = env_of(&[]);
        assert!(models_dir_with(Os::Windows, &e).is_err());
    }

    #[test]
    fn linux_uses_absolute_xdg_cache_home() {
        let e = env_of(&[("XDG_CACHE_HOME", "/x/c"), ("HOME", "/h")]);
        assert_eq!(models_dir_with(Os::Linux, &e).unwrap(), PathBuf::from("/x/c").join("speech.cpp").join("models"));
    }

    #[test]
    fn linux_ignores_relative_xdg_cache_home() {
        let e = env_of(&[("XDG_CACHE_HOME", "rel/c"), ("HOME", "/h")]);
        assert_eq!(
            models_dir_with(Os::Linux, &e).unwrap(),
            PathBuf::from("/h").join(".cache").join("speech.cpp").join("models")
        );
    }

    #[test]
    fn linux_without_home_errors() {
        let e = env_of(&[]);
        assert!(models_dir_with(Os::Linux, &e).is_err());
    }

    #[test]
    fn macos_uses_library_caches() {
        let e = env_of(&[("HOME", "/Users/a")]);
        assert_eq!(
            models_dir_with(Os::MacOs, &e).unwrap(),
            PathBuf::from("/Users/a").join("Library").join("Caches").join("speech.cpp").join("models")
        );
    }

    #[test]
    fn speech_model_dir_absolute_overrides() {
        let e = env_of(&[("SPEECH_MODEL_DIR", "/m/dir"), ("HOME", "/h")]);
        assert_eq!(models_dir_with(Os::Linux, &e).unwrap(), PathBuf::from("/m/dir"));
    }

    #[test]
    fn speech_model_dir_relative_errors() {
        let e = env_of(&[("SPEECH_MODEL_DIR", "rel"), ("HOME", "/h")]);
        assert!(models_dir_with(Os::Linux, &e).is_err());
    }

    #[test]
    fn empty_values_count_as_unset() {
        let e = env_of(&[("SPEECH_MODEL_DIR", ""), ("XDG_CACHE_HOME", ""), ("HOME", "/h")]);
        assert_eq!(
            models_dir_with(Os::Linux, &e).unwrap(),
            PathBuf::from("/h").join(".cache").join("speech.cpp").join("models")
        );
        let e = env_of(&[("LOCALAPPDATA", "")]);
        assert!(models_dir_with(Os::Windows, &e).is_err());
    }

    #[test]
    fn model_path_layout() {
        let p = model_path(Path::new("/m"), &HARU_MODEL);
        assert_eq!(
            p,
            Path::new("/m")
                .join("sakasegawa--Haru-4B-GGUF")
                .join("c372a527e63ca4251429a48863535b8bca03ebd6f414bbf31f28a9d175349934")
                .join("Haru-4B-Q8_0.gguf")
        );
    }

    #[test]
    fn partial_path_appends_part() {
        assert_eq!(partial_path(Path::new("/m/a.gguf")), PathBuf::from("/m/a.gguf.part"));
    }

    #[test]
    fn model_url_matches_spec() {
        assert_eq!(
            HARU_MODEL.url("https://huggingface.co"),
            "https://huggingface.co/sakasegawa/Haru-4B-GGUF/resolve/fc026182f7bbfaa0400b2d44358a5e9e327b957f/Haru-4B-Q8_0.gguf"
        );
    }

    #[test]
    fn backends_dir_search_order() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let c = tempfile::tempdir().unwrap();
        for d in [&b, &c] {
            std::fs::create_dir_all(d.path().join("vulkan")).unwrap();
            std::fs::write(d.path().join("vulkan").join(exe_name()), b"x").unwrap();
        }
        let got = find_backends_dir(None, Some(a.path().into()), Some(c.path().into()));
        assert_eq!(got, Some(c.path().to_path_buf()));
        let got = find_backends_dir(None, Some(b.path().into()), Some(c.path().into()));
        assert_eq!(got, Some(b.path().to_path_buf()));
        let got = find_backends_dir(Some(c.path().into()), Some(b.path().into()), None);
        assert_eq!(got, Some(c.path().to_path_buf()));
        // an override without variants is not silently replaced by the other locations
        assert_eq!(find_backends_dir(Some(a.path().into()), Some(b.path().into()), Some(c.path().into())), None);
    }
}
