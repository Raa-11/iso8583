//! Convert numeric ASCII fields (amount, STAN, dates) into numbers without `str::parse`.

use crate::charset::all_digits;

/// Convert exactly 8 ASCII digits into a number in one go using SWAR (Daniel Lemire's technique).
///
/// Three multiplications replace an 8-iteration loop. The input **must** already be checked
/// with `all_digits`; this function does not validate. Correct only on little-endian CPUs
/// (x86_64, ARM64).
#[inline]
fn parse_8_digits(b: &[u8]) -> u64 {
    let mut v = u64::from_le_bytes(b[..8].try_into().unwrap());
    v = ((v & 0x0F0F_0F0F_0F0F_0F0F).wrapping_mul(2561)) >> 8; // digit pairs → 0..99
    v = ((v & 0x00FF_00FF_00FF_00FF).wrapping_mul(6_553_601)) >> 16; // 4 digits → 0..9999
    ((v & 0x0000_FFFF_0000_FFFF).wrapping_mul(42_949_672_960_001)) >> 32 // 8 digits
}

/// Convert ASCII digits (1..=19 characters) into a `u64`.
///
/// 19 digits is the limit that always fits in a `u64`. Processed 8 digits per step with SWAR,
/// the rest one digit at a time. Suited for field 4 (amount, 12 digits), 11 (STAN), and
/// 7/12/13 (date/time).
///
/// Returns `None` if empty, longer than 19 characters, or any byte is not a digit.
///
/// ```
/// use iso_8583_rs::parse_digits_u64;
/// assert_eq!(parse_digits_u64(b"000000010000"), Some(10_000));
/// assert_eq!(parse_digits_u64(b"12a"), None);
/// ```
#[inline]
pub fn parse_digits_u64(d: &[u8]) -> Option<u64> {
    if d.is_empty() || d.len() > 19 || !all_digits(d) {
        return None;
    }
    let mut v = 0u64;
    let mut chunks = d.chunks_exact(8);
    for c in &mut chunks {
        v = v * 100_000_000 + parse_8_digits(c);
    }
    for &b in chunks.remainder() {
        v = v * 10 + (b - b'0') as u64;
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compare against `str::parse` for 200k random numbers of length 1..=19.
    #[test]
    fn swar_matches_std() {
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..200_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let len = (x % 19 + 1) as usize;
            let s = format!("{:019}", x % 10_000_000_000_000_000_000);
            let s = &s[19 - len..];
            assert_eq!(
                parse_digits_u64(s.as_bytes()),
                s.parse::<u64>().ok(),
                "{}",
                s
            );
        }
        assert_eq!(
            parse_digits_u64(b"9999999999999999999"),
            Some(9_999_999_999_999_999_999)
        );
        assert_eq!(parse_digits_u64(b"12a"), None);
        assert_eq!(parse_digits_u64(b""), None);
    }
}
