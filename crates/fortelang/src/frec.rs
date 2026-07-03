//! `.frec` — Forte recorded audio: PCM with a mandatory provenance block.
//! Audio without provenance cannot even be referenced (SYS-REC-001/002):
//! the only inputs the system accepts are MIDI and microphone takes, and a
//! take carries where/when/who it was captured. v0 checks structure and
//! presence; ed25519 signature verification arrives with key management.
//!
//! Layout: `FREC1\n` magic, u32-le header length, header JSON, f32-le PCM.

use crate::diag::{Diag, Pos};

pub const MAGIC: &[u8; 6] = b"FREC1\n";

pub struct Frec {
    pub rate: u32,
    pub channels: u16,
    pub pcm: Vec<f32>, // interleaved
    pub provenance: serde_json::Value,
}

pub fn encode(rate: u32, channels: u16, pcm: &[f32], provenance: &serde_json::Value) -> Vec<u8> {
    let header = serde_json::json!({
        "rate": rate, "ch": channels, "provenance": provenance,
    })
    .to_string();
    let mut out = Vec::with_capacity(MAGIC.len() + 4 + header.len() + pcm.len() * 4);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(header.as_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

pub fn decode(bytes: &[u8], pos: Pos) -> Result<Frec, Diag> {
    let err = |msg: &str| Diag::new("E-PROV-001", pos, msg.to_string());
    if bytes.len() < MAGIC.len() + 4 || &bytes[..MAGIC.len()] != MAGIC {
        return Err(err(".frec 形式ではありません"));
    }
    let hlen =
        u32::from_le_bytes(bytes[MAGIC.len()..MAGIC.len() + 4].try_into().unwrap()) as usize;
    let hstart = MAGIC.len() + 4;
    let Some(hbytes) = bytes.get(hstart..hstart + hlen) else {
        return Err(err(".frec ヘッダが壊れています"));
    };
    let header: serde_json::Value =
        serde_json::from_slice(hbytes).map_err(|_| err(".frec ヘッダが JSON として読めません"))?;
    let rate = header["rate"].as_u64().unwrap_or(0) as u32;
    let channels = header["ch"].as_u64().unwrap_or(0) as u16;
    if !(8000..=192_000).contains(&rate) || !(1..=2).contains(&channels) {
        return Err(err(".frec のサンプルレート/チャネル数が不正です"));
    }

    // provenance is not optional — that is the whole point
    let prov = &header["provenance"];
    let device_class = prov["device_class"].as_str().unwrap_or("");
    if device_class != "microphone" && device_class != "midi-render" {
        return Err(Diag::new(
            "E-PROV-001",
            pos,
            "録音来歴がありません(device_class は microphone / midi-render)。外部オーディオの持ち込みは仕様として存在しません(SYS-REC-001)",
        ));
    }
    for field in ["recorded_at", "by", "session", "sig"] {
        if prov[field].as_str().unwrap_or("").is_empty() {
            return Err(Diag::new(
                "E-PROV-001",
                pos,
                format!("録音来歴に {field} がありません"),
            ));
        }
    }

    let pcm_bytes = &bytes[hstart + hlen..];
    let pcm: Vec<f32> = pcm_bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    if pcm.is_empty() {
        return Err(err(".frec に音声データがありません"));
    }
    Ok(Frec { rate, channels, pcm, provenance: prov.clone() })
}
