//! Base64 i16-LE mono PCM codec (F10).
use base64::{engine::general_purpose::STANDARD, Engine as _};

use crate::error::{ErrorCode, HaruError};
use crate::messages::MsgKey;

pub fn encode_i16(samples: &[i16]) -> String {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    STANDARD.encode(bytes)
}

pub fn encode_f32(samples: &[f32]) -> String {
    let v: Vec<i16> = samples
        .iter()
        .map(|&x| (x.clamp(-1.0, 1.0) * 32767.0).round() as i16)
        .collect();
    encode_i16(&v)
}

pub fn decode_i16(s: &str) -> Result<Vec<i16>, HaruError> {
    let bytes = STANDARD
        .decode(s)
        .map_err(|e| HaruError::new(ErrorCode::Worker, MsgKey::WorkerBadAudio).with_detail(e.to_string()))?;
    if bytes.len() % 2 != 0 {
        return Err(HaruError::new(ErrorCode::Worker, MsgKey::WorkerBadAudio)
            .with_detail(format!("odd PCM byte length {}", bytes.len())));
    }
    Ok(bytes.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect())
}

pub fn decode_f32(s: &str) -> Result<Vec<f32>, HaruError> {
    Ok(decode_i16(s)?.into_iter().map(|x| x as f32 / 32768.0).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_f32_rounds_and_clamps() {
        let enc = encode_f32(&[0.0, 1.0, -1.0, 0.5, 2.0, -2.0]);
        assert_eq!(decode_i16(&enc).unwrap(), vec![0, 32767, -32767, 16384, 32767, -32767]);
    }

    #[test]
    fn encode_known_bytes() {
        assert_eq!(encode_i16(&[0, 32767]), "AAD/fw==");
    }

    #[test]
    fn decode_known_bytes() {
        assert_eq!(decode_f32("AAD/fw==").unwrap(), vec![0.0, 32767.0 / 32768.0]);
    }

    #[test]
    fn decode_rejects_odd_length() {
        assert_eq!(decode_i16("AA==").unwrap_err().code, ErrorCode::Worker);
    }

    #[test]
    fn decode_rejects_invalid_base64() {
        assert_eq!(decode_i16("not base64!").unwrap_err().code, ErrorCode::Worker);
    }

    #[test]
    fn roundtrip_i16() {
        let mut x: u32 = 12345;
        let v: Vec<i16> = (0..1000)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                (x >> 16) as i16
            })
            .collect();
        assert_eq!(decode_i16(&encode_i16(&v)).unwrap(), v);
    }
}
