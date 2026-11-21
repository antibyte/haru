//! `speech devices --json` parsing (Task 1.3).
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::error::{ErrorCode, HaruError};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Cpu,
    Gpu,
    Igpu,
}

impl DeviceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DeviceKind::Cpu => "cpu",
            DeviceKind::Gpu => "gpu",
            DeviceKind::Igpu => "igpu",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Device {
    pub name: String,
    pub description: String,
    pub kind: DeviceKind,
    #[serde(default)]
    pub memory_total: u64,
    #[serde(default)]
    pub memory_free: u64,
}

#[derive(Deserialize)]
struct DevicesDoc {
    devices: Vec<Device>,
}

pub fn parse_devices_json(s: &str) -> Result<Vec<Device>, HaruError> {
    serde_json::from_str::<DevicesDoc>(s.trim()).map(|d| d.devices).map_err(|e| {
        HaruError::new(ErrorCode::NoBackend, "Die Geräteliste des Backends ist ungültig.")
            .with_detail(format!("{e}: {}", s.chars().take(300).collect::<String>()))
    })
}

const QUERY_TIMEOUT: Duration = Duration::from_secs(20);

/// Runs `<exe> devices --json` (no console window, update check off, 20 s timeout).
pub fn query_devices(exe: &Path) -> Result<Vec<Device>, HaruError> {
    let mut cmd = Command::new(exe);
    cmd.args(["devices", "--json"])
        .env("SPEECH_NO_UPDATE_CHECK", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let fail = |detail: String| {
        HaruError::new(ErrorCode::NoBackend, "Die Geräte des Backends konnten nicht abgefragt werden.")
            .with_detail(format!("{}: {detail}", exe.display()))
    };
    let mut child = cmd.spawn().map_err(|e| fail(e.to_string()))?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let out_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {
                if start.elapsed() > QUERY_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(fail("timeout after 20 s".into()));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(fail(e.to_string())),
        }
    };
    let out = out_thread.join().unwrap_or_default();
    let err = err_thread.join().unwrap_or_default();
    if !status.success() {
        return Err(fail(format!("exit {status}: {}", err.trim())));
    }
    parse_devices_json(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cli_doc_example() {
        let s = r#"{"devices":[{"name":"Vulkan0","description":"Intel(R) Arc(TM) B580 Graphics","kind":"gpu","memory_total":12000000000,"memory_free":11000000000}]}"#;
        let d = parse_devices_json(s).unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].name, "Vulkan0");
        assert_eq!(d[0].kind, DeviceKind::Gpu);
        assert_eq!(d[0].memory_total, 12_000_000_000);
        assert_eq!(d[0].memory_free, 11_000_000_000);
    }

    #[test]
    fn parses_cpu_and_igpu_kinds() {
        let s = r#"{"devices":[{"name":"Vulkan1","description":"UHD","kind":"igpu","memory_total":1,"memory_free":1},
            {"name":"CPU","description":"CPU","kind":"cpu","memory_total":0,"memory_free":0}]}"#;
        let d = parse_devices_json(s).unwrap();
        assert_eq!(d[0].kind, DeviceKind::Igpu);
        assert_eq!(d[1].kind, DeviceKind::Cpu);
    }

    #[test]
    fn rejects_malformed_devices_json() {
        assert!(parse_devices_json("not json").is_err());
        assert!(parse_devices_json(r#"{"devices":[{"name":"x"}]}"#).is_err());
        assert!(parse_devices_json(r#"{"foo":[]}"#).is_err());
    }

    /// Run with: HARU_TEST_SPEECH=<path to speech.exe> cargo test --lib query_real -- --ignored --nocapture
    #[test]
    #[ignore]
    fn query_real_vulkan_variant() {
        let exe = std::env::var("HARU_TEST_SPEECH").expect("set HARU_TEST_SPEECH");
        let devices = query_devices(Path::new(&exe)).unwrap();
        println!("{devices:#?}");
        assert!(!devices.is_empty());
    }
}
