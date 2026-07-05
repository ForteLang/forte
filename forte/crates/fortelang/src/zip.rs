//! Minimal ZIP writer/reader (store-only, no deps) for `forte export`.
//! Deterministic on purpose: entries are written in the order given with a
//! fixed DOS timestamp, so the same sources produce byte-identical archives —
//! an export is a build artifact like any other.

/// CRC-32 (IEEE), bitwise — archives are small, simplicity wins.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn u16le(v: u16, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn u32le(v: u32, out: &mut Vec<u8>) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Build a ZIP from `(path, bytes)` entries (order preserved).
pub fn write(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let name_b = name.as_bytes();

        // local file header
        u32le(0x0403_4b50, &mut out);
        u16le(20, &mut out); // version needed
        u16le(0x0800, &mut out); // UTF-8 names
        u16le(0, &mut out); // method: store
        u16le(0, &mut out); // time (fixed: determinism)
        u16le(0x0021, &mut out); // date 1980-01-01
        u32le(crc, &mut out);
        u32le(data.len() as u32, &mut out);
        u32le(data.len() as u32, &mut out);
        u16le(name_b.len() as u16, &mut out);
        u16le(0, &mut out);
        out.extend_from_slice(name_b);
        out.extend_from_slice(data);

        // central directory record
        u32le(0x0201_4b50, &mut central);
        u16le(20, &mut central);
        u16le(20, &mut central);
        u16le(0x0800, &mut central);
        u16le(0, &mut central);
        u16le(0, &mut central);
        u16le(0x0021, &mut central);
        u32le(crc, &mut central);
        u32le(data.len() as u32, &mut central);
        u32le(data.len() as u32, &mut central);
        u16le(name_b.len() as u16, &mut central);
        u16le(0, &mut central);
        u16le(0, &mut central);
        u16le(0, &mut central);
        u16le(0, &mut central);
        u32le(0, &mut central);
        u32le(offset, &mut central);
        central.extend_from_slice(name_b);
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    // end of central directory
    u32le(0x0605_4b50, &mut out);
    u16le(0, &mut out);
    u16le(0, &mut out);
    u16le(entries.len() as u16, &mut out);
    u16le(entries.len() as u16, &mut out);
    u32le(central.len() as u32, &mut out);
    u32le(cd_offset, &mut out);
    u16le(0, &mut out);
    out
}

/// Read a store-only ZIP back into `(path, bytes)` entries (round-trip
/// verification and future `forte import`).
pub fn read(zip: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let eocd = zip
        .windows(4)
        .rposition(|w| w == [0x50, 0x4b, 0x05, 0x06])
        .ok_or("EOCD がありません")?;
    let count = u16::from_le_bytes([zip[eocd + 10], zip[eocd + 11]]) as usize;
    let mut pos =
        u32::from_le_bytes([zip[eocd + 16], zip[eocd + 17], zip[eocd + 18], zip[eocd + 19]])
            as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if zip[pos..pos + 4] != [0x50, 0x4b, 0x01, 0x02] {
            return Err("central directory が壊れています".into());
        }
        let method = u16::from_le_bytes([zip[pos + 10], zip[pos + 11]]);
        if method != 0 {
            return Err("store 形式のみ対応です".into());
        }
        let crc = u32::from_le_bytes(zip[pos + 16..pos + 20].try_into().unwrap());
        let size = u32::from_le_bytes(zip[pos + 20..pos + 24].try_into().unwrap()) as usize;
        let name_len = u16::from_le_bytes([zip[pos + 28], zip[pos + 29]]) as usize;
        let extra = u16::from_le_bytes([zip[pos + 30], zip[pos + 31]]) as usize;
        let comment = u16::from_le_bytes([zip[pos + 32], zip[pos + 33]]) as usize;
        let lho = u32::from_le_bytes(zip[pos + 42..pos + 46].try_into().unwrap()) as usize;
        let name = String::from_utf8_lossy(&zip[pos + 46..pos + 46 + name_len]).into_owned();

        // hop to the local header to find the data
        let lh_name = u16::from_le_bytes([zip[lho + 26], zip[lho + 27]]) as usize;
        let lh_extra = u16::from_le_bytes([zip[lho + 28], zip[lho + 29]]) as usize;
        let start = lho + 30 + lh_name + lh_extra;
        let data = zip[start..start + size].to_vec();
        if crc32(&data) != crc {
            return Err(format!("{name}: CRC 不一致(壊れたアーカイブ)"));
        }
        out.push((name, data));
        pos += 46 + name_len + extra + comment;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_determinism() {
        let entries = vec![
            ("song.forte".to_string(), b"song \"X\" {}".to_vec()),
            ("assets/take.frec".to_string(), vec![0u8, 255, 7, 42]),
        ];
        let a = write(&entries);
        let b = write(&entries);
        assert_eq!(a, b, "same entries must produce byte-identical archives");
        assert_eq!(read(&a).unwrap(), entries);
    }
}
