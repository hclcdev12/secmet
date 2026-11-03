//! Base45 (RFC 9285) : 2 octets → 3 caractères de l'alphabet alphanumérique
//! des QR codes, ce qui permet d'encoder un flux binaire en mode
//! alphanumérique (plus dense que le mode octet).

use crate::{AttestError, Result};

const ALPHABET: &[u8; 45] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

fn value_of(c: u8) -> Option<u32> {
    ALPHABET.iter().position(|&a| a == c).map(|p| p as u32)
}

/// Encode `bytes`.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() / 2 * 3 + 2);
    let mut chunks = bytes.chunks_exact(2);
    for pair in &mut chunks {
        let n = u32::from(pair[0]) * 256 + u32::from(pair[1]);
        let (c, rest) = (n % 45, n / 45);
        let (d, e) = (rest % 45, rest / 45);
        for v in [c, d, e] {
            out.push(ALPHABET[v as usize] as char);
        }
    }
    if let [last] = chunks.remainder() {
        let n = u32::from(*last);
        out.push(ALPHABET[(n % 45) as usize] as char);
        out.push(ALPHABET[(n / 45) as usize] as char);
    }
    out
}

/// Décode `text` ; refuse un caractère hors alphabet, une longueur ≡ 1 mod 3
/// ou un triplet qui dépasse 65 535.
pub fn decode(text: &str) -> Result<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.len() % 3 == 1 {
        return Err(AttestError::Encoding(
            "Base45: longueur invalide (reste 1 modulo 3)".into(),
        ));
    }
    let mut values = Vec::with_capacity(bytes.len());
    for &c in bytes {
        values.push(
            value_of(c)
                .ok_or_else(|| AttestError::Encoding(format!("Base45: caractère {c:#x}")))?,
        );
    }
    let mut out = Vec::with_capacity(values.len() / 3 * 2 + 1);
    let mut chunks = values.chunks_exact(3);
    for t in &mut chunks {
        let n = t[0] + t[1] * 45 + t[2] * 45 * 45;
        if n > 0xffff {
            return Err(AttestError::Encoding("Base45: triplet hors plage".into()));
        }
        out.push((n >> 8) as u8);
        out.push((n & 0xff) as u8);
    }
    if let [c, d] = chunks.remainder() {
        let n = c + d * 45;
        if n > 0xff {
            return Err(AttestError::Encoding(
                "Base45: paire finale hors plage".into(),
            ));
        }
        out.push(n as u8);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn rfc_9285_vectors() {
        assert_eq!(encode(b"AB"), "BB8");
        assert_eq!(encode(b"Hello!!"), "%69 VD92EX0");
        assert_eq!(encode(b"base-45"), "UJCLQE7W581");
        assert_eq!(encode(b"ietf!"), "QED8WEX0");
        assert_eq!(decode("QED8WEX0").unwrap(), b"ietf!");
        assert_eq!(decode("").unwrap(), b"");
        assert_eq!(encode(b""), "");
    }

    #[test]
    fn round_trips_every_length_and_rejects_garbage() {
        for len in 0..40usize {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let text = encode(&data);
            assert!(text.bytes().all(|c| ALPHABET.contains(&c)));
            assert_eq!(decode(&text).unwrap(), data, "len {len}");
        }
        assert!(decode("A").is_err());
        assert!(decode("abc").is_err());
        assert!(decode(":::").is_err(), "44 + 44*45 + 44*2025 > 65535");
        assert!(decode("FGW").is_ok(), "15 + 16*45 + 32*2025 = 65535");
        assert!(decode("ZZ").is_err(), "35 + 35*45 > 255");
    }
}
