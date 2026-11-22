//! cpal device enumeration (Task 1.5).

use crate::error::{ErrorCode, HaruError};
use crate::messages::MsgKey;
use crate::ipc::AudioDevices;
use cpal::traits::{DeviceTrait, HostTrait};

fn audio_err(msg: MsgKey, detail: impl ToString) -> HaruError {
    HaruError::new(ErrorCode::AudioDevice, msg).with_detail(detail.to_string())
}

pub fn list() -> Result<AudioDevices, HaruError> {
    let host = cpal::default_host();
    let inputs = host
        .input_devices()
        .map_err(|e| audio_err(MsgKey::AudioDevicesUnreadable, e))?
        .filter_map(|d| d.name().ok())
        .collect();
    let outputs = host
        .output_devices()
        .map_err(|e| audio_err(MsgKey::AudioDevicesUnreadable, e))?
        .filter_map(|d| d.name().ok())
        .collect();
    Ok(AudioDevices {
        inputs,
        outputs,
        default_input: host.default_input_device().and_then(|d| d.name().ok()),
        default_output: host.default_output_device().and_then(|d| d.name().ok()),
    })
}

pub fn input_device(name: Option<&str>) -> Result<cpal::Device, HaruError> {
    let host = cpal::default_host();
    match name {
        None => host
            .default_input_device()
            .ok_or_else(|| HaruError::new(ErrorCode::NoMicrophone, MsgKey::NoMicrophone)),
        Some(n) => {
            let mut any = false;
            let devices = host
                .input_devices()
                .map_err(|e| audio_err(MsgKey::AudioDevicesUnreadable, e))?;
            for d in devices {
                any = true;
                if d.name().map(|dn| dn == n).unwrap_or(false) {
                    return Ok(d);
                }
            }
            if !any {
                return Err(HaruError::new(ErrorCode::NoMicrophone, MsgKey::NoMicrophone));
            }
            Err(HaruError::new(ErrorCode::AudioDevice, MsgKey::InputDeviceNotFound).with_param("name", n)
                .with_detail(format!("unknown input device: {n}")))
        }
    }
}

pub fn output_device(name: Option<&str>) -> Result<cpal::Device, HaruError> {
    let host = cpal::default_host();
    match name {
        None => host
            .default_output_device()
            .ok_or_else(|| HaruError::new(ErrorCode::AudioDevice, MsgKey::NoOutputDevice)),
        Some(n) => {
            let devices = host
                .output_devices()
                .map_err(|e| audio_err(MsgKey::AudioDevicesUnreadable, e))?;
            for d in devices {
                if d.name().map(|dn| dn == n).unwrap_or(false) {
                    return Ok(d);
                }
            }
            Err(HaruError::new(ErrorCode::AudioDevice, MsgKey::OutputDeviceNotFound).with_param("name", n)
                .with_detail(format!("unknown output device: {n}")))
        }
    }
}
