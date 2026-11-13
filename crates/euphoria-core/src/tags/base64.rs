// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The base64 alphabet of RFC 4648, as Serato's tag data uses it.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `bytes` in base64, with `=` padding when `padded`.
pub(super) fn encode(bytes: &[u8], padded: bool) -> String {
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let first = group[0];
        let second = group.get(1).copied().unwrap_or(0);
        let third = group.get(2).copied().unwrap_or(0);
        let digits = [
            first >> 2,
            ((first & 0x03) << 4) | (second >> 4),
            ((second & 0x0F) << 2) | (third >> 6),
            third & 0x3F,
        ];
        let kept = group.len() + 1;
        for digit in &digits[..kept] {
            text.push(char::from(ALPHABET[usize::from(*digit)]));
        }
        if padded {
            for _ in kept..4 {
                text.push('=');
            }
        }
    }
    text
}

/// The bytes of base64 `text`. Line breaks, `=` padding and a missing
/// padding are accepted, and so is one stray digit at the end, which
/// Serato sometimes writes: it is completed with `A` as Serato's readers
/// do. `None` for any other character.
pub(super) fn decode(text: &[u8]) -> Option<Vec<u8>> {
    let mut digits: Vec<u8> = Vec::with_capacity(text.len());
    for byte in text {
        match byte {
            b'\n' | b'\r' => {}
            b'=' => break,
            _ => digits.push(digit_value(*byte)?),
        }
    }
    if digits.len() % 4 == 1 {
        digits.push(0);
    }
    let mut bytes = Vec::with_capacity(digits.len() / 4 * 3 + 2);
    for group in digits.chunks(4) {
        let value = group
            .iter()
            .enumerate()
            .fold(0_u32, |value, (index, digit)| {
                value | (u32::from(*digit) << (18 - 6 * index))
            });
        let produced = group.len() - 1;
        bytes.extend(&value.to_be_bytes()[1..1 + produced]);
    }
    Some(bytes)
}

fn digit_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values_round_trip() {
        for (bytes, padded, unpadded) in [
            (&b""[..], "", ""),
            (b"f", "Zg==", "Zg"),
            (b"fo", "Zm8=", "Zm8"),
            (b"foo", "Zm9v", "Zm9v"),
            (b"foob", "Zm9vYg==", "Zm9vYg"),
            (b"\xff\xfe\x00", "//4A", "//4A"),
        ] {
            assert_eq!(encode(bytes, true), padded);
            assert_eq!(encode(bytes, false), unpadded);
            assert_eq!(decode(padded.as_bytes()).unwrap(), bytes);
            assert_eq!(decode(unpadded.as_bytes()).unwrap(), bytes);
        }
        assert_eq!(decode(b"Zm9v\nYg").unwrap(), b"foob");
        assert_eq!(decode(b"Zm9vY").unwrap(), b"foo\x60");
        assert_eq!(decode(b"Zm9v!"), None);
    }
}
