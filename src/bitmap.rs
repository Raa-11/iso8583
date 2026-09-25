//! ISO 8583 bitmap: bit position per field and hex ASCII ↔ number conversion.
//!
//! The primary + secondary bitmap is stored as a single `u128`:
//! bit 127 = field 1, bit 126 = field 2, …, bit 0 = field 128.
//! In this form, "which fields are present" is found with `leading_zeros` (one instruction),
//! and "how many fields come before field n" with `count_ones`.

use crate::error::Error;

/// Field 1 bit: marks that the secondary bitmap (fields 65–128) is present.
pub(crate) const B1: u128 = 1 << 127;

/// Field 65 bit: marks a tertiary bitmap (fields 129–192). Not supported; the message is rejected.
pub(crate) const B65: u128 = 1 << 63;

/// Bit mask for field `n` (1..=128) inside the `u128` bitmap.
///
/// The caller must ensure `n` is in 1..=128; outside that range the shift overflows.
#[inline]
pub(crate) const fn bit(n: usize) -> u128 {
    1u128 << (128 - n)
}

/// Return the lowest field number whose bit is set in `bits`, and clear that bit.
///
/// Used to iterate present fields in order without checking all 128 bits one by one.
/// The caller must ensure `bits != 0`.
#[inline]
pub(crate) fn pop_field(bits: &mut u128) -> usize {
    let n = bits.leading_zeros() as usize + 1;
    *bits ^= bit(n);
    n
}

/// Uppercase hex digits, used when writing the bitmap (common on ISO 8583 hosts).
pub(crate) const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";

/// Lowercase hex digits, used by the legacy API so its output does not change.
pub(crate) const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";

/// Build the ASCII character → nibble value (0..=15) table; non-hex characters map to `0xFF`.
///
/// `const fn` so the table is computed at compile time, not at runtime.
const fn hex_lut() -> [u8; 256] {
    let mut t = [0xFFu8; 256];
    let mut i = 0;
    while i < 10 {
        t[b'0' as usize + i] = i as u8;
        i += 1;
    }
    let mut i = 0;
    while i < 6 {
        t[b'A' as usize + i] = 10 + i as u8;
        t[b'a' as usize + i] = 10 + i as u8;
        i += 1;
    }
    t
}

/// Hex lookup table: one array access per character, no branching.
pub(crate) static HEX_LUT: [u8; 256] = hex_lut();

/// Convert 16 hex characters (upper or lower case) into a `u64`.
///
/// All characters are processed first and validity is checked once at the end: invalid
/// characters map to `0xFF`, which sets bits 4..7 in `bad`. With no per-character branch the
/// CPU has nothing to mispredict.
///
/// # Errors
/// [`Error::BadHex`] if any character is not hex.
#[inline]
pub(crate) fn hex16_to_u64(s: &[u8; 16]) -> Result<u64, Error> {
    let mut v = 0u64;
    let mut bad = 0u8;
    for &c in s {
        let n = HEX_LUT[c as usize];
        bad |= n;
        v = (v << 4) | (n & 0xF) as u64;
    }
    if bad & 0xF0 != 0 {
        Err(Error::BadHex)
    } else {
        Ok(v)
    }
}

/// Convert a `u64` into 16 hex characters using the `digits` table (upper or lower case).
///
/// The result is returned as a stack array, so nothing is allocated.
#[inline]
pub(crate) fn u64_to_hex16(v: u64, digits: &[u8; 16]) -> [u8; 16] {
    let mut b = [0u8; 16];
    for (i, o) in b.iter_mut().enumerate() {
        *o = digits[((v >> (60 - 4 * i)) & 0xF) as usize];
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        for v in [
            0u64,
            1,
            u64::MAX,
            0x7234_0540_0AC0_8000,
            0xDEAD_BEEF_0123_4567,
        ] {
            assert_eq!(hex16_to_u64(&u64_to_hex16(v, HEX_UPPER)).unwrap(), v);
            assert_eq!(hex16_to_u64(&u64_to_hex16(v, HEX_LOWER)).unwrap(), v);
        }
        assert_eq!(hex16_to_u64(b"000000000000000G"), Err(Error::BadHex));
    }

    #[test]
    fn pop_field_iterates_in_order() {
        let mut bits = bit(2) | bit(64) | bit(128);
        assert_eq!(pop_field(&mut bits), 2);
        assert_eq!(pop_field(&mut bits), 64);
        assert_eq!(pop_field(&mut bits), 128);
        assert_eq!(bits, 0);
    }
}
