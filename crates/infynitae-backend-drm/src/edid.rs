//! EDID (Extended Display Identification Data) parser for monitor identity.

/// Parsed monitor identity from an EDID structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Edid {
    pub(crate) make: String,
    pub(crate) model: String,
    pub(crate) serial: Option<String>,
}

const EDID_HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
const DESCRIPTOR_OFFSETS: [usize; 4] = [54, 72, 90, 108];

/// Parse the monitor make, model and serial from an EDID blob.
///
/// Returns `None` if `blob` is shorter than 128 bytes or does not begin with
/// the standard 8-byte EDID header.
pub(crate) fn parse(blob: &[u8]) -> Option<Edid> {
    if blob.len() < 128 || blob[0..8] != EDID_HEADER {
        return None;
    }

    // Bytes 8-9: 3-letter PNP ID (big-endian, three 5-bit letters, 1 = 'A').
    let pnp_raw = u16::from_be_bytes([blob[8], blob[9]]);
    let c1 = ((pnp_raw >> 10) & 0x1F) as u8;
    let c2 = ((pnp_raw >> 5) & 0x1F) as u8;
    let c3 = (pnp_raw & 0x1F) as u8;

    let mut pnp = String::with_capacity(3);
    for c in [c1, c2, c3] {
        if (1..=26).contains(&c) {
            pnp.push((b'A' + c - 1) as char);
        }
    }
    let make = if pnp.is_empty() {
        "Unknown".to_string()
    } else {
        pnp_name(&pnp).to_string()
    };

    // Descriptors at offsets 54, 72, 90, 108.
    // Model: text of 18-byte display descriptor tag 0xFC, else product code (bytes 10-11, little-endian) as 0x%04X.
    let mut model = None;
    for &offset in &DESCRIPTOR_OFFSETS {
        if let Some(text) = text_descriptor(&blob[offset..offset + 18], 0xFC) {
            model = Some(text);
            break;
        }
    }
    let model = model.unwrap_or_else(|| {
        let prod = u16::from_le_bytes([blob[10], blob[11]]);
        format!("0x{prod:04X}")
    });

    // Serial: descriptor tag 0xFF text if present and non-empty; else 32-bit serial at bytes 12-15
    // (little-endian) as decimal if non-zero; else None.
    let mut serial = None;
    for &offset in &DESCRIPTOR_OFFSETS {
        if let Some(text) = text_descriptor(&blob[offset..offset + 18], 0xFF) {
            serial = Some(text);
            break;
        }
    }
    let serial = serial.or_else(|| {
        let sn = u32::from_le_bytes([blob[12], blob[13], blob[14], blob[15]]);
        if sn != 0 { Some(sn.to_string()) } else { None }
    });

    Some(Edid {
        make,
        model,
        serial,
    })
}

/// Extract text from an 18-byte display descriptor with the given tag.
///
/// A descriptor is text when bytes 0..3 are 0 and byte 3 is the tag.
/// The text is in bytes 5..18, ends at 0x0A, non-printable or non-ASCII bytes dropped,
/// and trailing spaces trimmed. Returns `None` if the resulting text is empty.
fn text_descriptor(desc: &[u8], tag: u8) -> Option<String> {
    if desc.len() < 18 {
        return None;
    }
    if desc[0] != 0 || desc[1] != 0 || desc[2] != 0 || desc[3] != tag {
        return None;
    }
    let text_bytes = &desc[5..18];
    let end = text_bytes
        .iter()
        .position(|&b| b == 0x0A)
        .unwrap_or(text_bytes.len());
    let mut s = String::new();
    for &b in &text_bytes[..end] {
        if (0x20..=0x7E).contains(&b) {
            s.push(b as char);
        }
    }
    let trimmed = s.trim_end();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Map a 3-letter PNP ID to a human-readable manufacturer name when common,
/// otherwise returning the 3-letter ID itself.
fn pnp_name(id: &str) -> &str {
    match id {
        "SAM" => "Samsung",
        "DEL" => "Dell",
        "GSM" => "LG",
        "AUS" => "ASUS",
        "ACR" => "Acer",
        "BNQ" => "BenQ",
        "AOC" => "AOC",
        "HWP" | "HPN" => "HP",
        "LEN" => "Lenovo",
        "PHL" => "Philips",
        "VSC" => "ViewSonic",
        "MSI" => "MSI",
        "GBT" => "Gigabyte",
        "SNY" => "Sony",
        "ENC" => "Eizo",
        "IVM" => "Iiyama",
        "BOE" => "BOE",
        "AUO" => "AU Optronics",
        "CMN" => "Chimei Innolux",
        "SHP" => "Sharp",
        _ => id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_pnp(letters: [char; 3]) -> [u8; 2] {
        let mut val = 0u16;
        for (i, &ch) in letters.iter().enumerate() {
            let shift = (2 - i) * 5;
            let code = if ch.is_ascii_alphabetic() {
                (ch.to_ascii_uppercase() as u8 - b'A' + 1) as u16
            } else {
                0
            };
            val |= (code & 0x1F) << shift;
        }
        val.to_be_bytes()
    }

    fn write_descriptor(blob: &mut [u8], offset: usize, tag: u8, text: &[u8]) {
        blob[offset..offset + 18].fill(0);
        blob[offset + 3] = tag;
        let copy_len = text.len().min(13);
        blob[offset + 5..offset + 5 + copy_len].copy_from_slice(&text[..copy_len]);
    }

    fn make_valid_edid() -> [u8; 128] {
        let mut blob = [0u8; 128];
        blob[0..8].copy_from_slice(&EDID_HEADER);
        let pnp = encode_pnp(['D', 'E', 'L']);
        blob[8] = pnp[0];
        blob[9] = pnp[1];
        blob[10..12].copy_from_slice(&0x1234u16.to_le_bytes());
        blob[12..16].copy_from_slice(&987654321u32.to_le_bytes());
        blob
    }

    #[test]
    fn test_real_shaped_edid() {
        let mut blob = make_valid_edid();
        write_descriptor(&mut blob, 54, 0xFC, b"DELL U2720Q\n ");
        write_descriptor(&mut blob, 72, 0xFF, b"CN001234\n ");

        let edid = parse(&blob).expect("valid EDID should parse");
        assert_eq!(edid.make, "Dell");
        assert_eq!(edid.model, "DELL U2720Q");
        assert_eq!(edid.serial, Some("CN001234".to_string()));
    }

    #[test]
    fn test_pnp_mapping() {
        let check_pnp = |letters: [char; 3], expected_make: &str| {
            let mut blob = make_valid_edid();
            let pnp = encode_pnp(letters);
            blob[8] = pnp[0];
            blob[9] = pnp[1];
            let edid = parse(&blob).expect("valid EDID should parse");
            assert_eq!(edid.make, expected_make);
        };

        check_pnp(['S', 'A', 'M'], "Samsung");
        check_pnp(['G', 'S', 'M'], "LG");
        check_pnp(['A', 'U', 'S'], "ASUS");
        check_pnp(['A', 'C', 'R'], "Acer");
        check_pnp(['B', 'N', 'Q'], "BenQ");
        check_pnp(['A', 'O', 'C'], "AOC");
        check_pnp(['H', 'W', 'P'], "HP");
        check_pnp(['H', 'P', 'N'], "HP");
        check_pnp(['L', 'E', 'N'], "Lenovo");
        check_pnp(['P', 'H', 'L'], "Philips");
        check_pnp(['V', 'S', 'C'], "ViewSonic");
        check_pnp(['M', 'S', 'I'], "MSI");
        check_pnp(['G', 'B', 'T'], "Gigabyte");
        check_pnp(['S', 'N', 'Y'], "Sony");
        check_pnp(['E', 'N', 'C'], "Eizo");
        check_pnp(['I', 'V', 'M'], "Iiyama");
        check_pnp(['B', 'O', 'E'], "BOE");
        check_pnp(['A', 'U', 'O'], "AU Optronics");
        check_pnp(['C', 'M', 'N'], "Chimei Innolux");
        check_pnp(['S', 'H', 'P'], "Sharp");
        check_pnp(['X', 'Y', 'Z'], "XYZ");
    }

    #[test]
    fn test_no_name_fallback_to_product_code() {
        let mut blob = make_valid_edid();
        blob[10..12].copy_from_slice(&0xABCDu16.to_le_bytes());
        // Descriptors left with tag 0
        let edid = parse(&blob).expect("valid EDID should parse");
        assert_eq!(edid.model, "0xABCD");

        // Even with non-name descriptor
        write_descriptor(&mut blob, 54, 0xFD, b"dummy range");
        let edid = parse(&blob).expect("valid EDID should parse");
        assert_eq!(edid.model, "0xABCD");
    }

    #[test]
    fn test_numeric_serial() {
        let mut blob = make_valid_edid();
        blob[12..16].copy_from_slice(&12345678u32.to_le_bytes());
        let edid = parse(&blob).expect("valid EDID should parse");
        assert_eq!(edid.serial, Some("12345678".to_string()));
    }

    #[test]
    fn test_zero_serial_returns_none() {
        let mut blob = make_valid_edid();
        blob[12..16].copy_from_slice(&0u32.to_le_bytes());
        let edid = parse(&blob).expect("valid EDID should parse");
        assert_eq!(edid.serial, None);
    }

    #[test]
    fn test_bad_header_returns_none() {
        let mut blob = make_valid_edid();
        blob[0] = 0x01;
        assert_eq!(parse(&blob), None);

        let mut blob2 = make_valid_edid();
        blob2[7] = 0xFF;
        assert_eq!(parse(&blob2), None);
    }

    #[test]
    fn test_truncated_blob_returns_none() {
        let blob = make_valid_edid();
        for len in 0..128 {
            assert_eq!(
                parse(&blob[..len]),
                None,
                "length {len} should fail to parse"
            );
        }
    }

    #[test]
    fn test_random_bytes_never_panic() {
        let valid = make_valid_edid();
        // Every prefix length of a valid blob and beyond
        for len in 0..=valid.len() {
            let _ = parse(&valid[..len]);
        }

        let mut extended = vec![0u8; 256];
        extended[..128].copy_from_slice(&valid);
        for len in 0..=extended.len() {
            let _ = parse(&extended[..len]);
        }

        // Deterministic pseudo-random generation to test various garbage patterns
        let mut seed: u32 = 0x12345678;
        let mut pseudo_rand = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 16) as u8
        };

        for size in [0, 1, 7, 8, 16, 64, 127, 128, 129, 256, 512] {
            let mut buf = vec![0u8; size];
            for _ in 0..20 {
                for b in &mut buf {
                    *b = pseudo_rand();
                }
                let _ = parse(&buf);
            }
        }
    }

    #[test]
    fn test_text_descriptor_formatting() {
        let mut blob = make_valid_edid();
        // 13 characters exact, no newline
        write_descriptor(&mut blob, 54, 0xFC, b"1234567890123");
        let edid = parse(&blob).expect("valid EDID should parse");
        assert_eq!(edid.model, "1234567890123");

        // Non-printable characters filtered out, trailing spaces trimmed
        let text_with_control = [
            b'H', 0x01, b'e', 0x02, b'l', b'l', b'o', b' ', b' ', 0x0A, b'x', b'y', b'z',
        ];
        write_descriptor(&mut blob, 54, 0xFC, &text_with_control);
        let edid = parse(&blob).expect("valid EDID should parse");
        assert_eq!(edid.model, "Hello");
    }
}
