//! Character validation per ISO 8583 content type (`n`, `a`, `an`, `ans`).
//!
//! ISO 8583 fields are typically 3–20 bytes: too short for SIMD. SWAR (SIMD within a register)
//! is used instead: 8 bytes are checked at once inside one `u64`, and the < 8 byte remainder is
//! checked byte by byte. `a` and `an` use a lookup table because their character ranges are not
//! contiguous.

/// High bit (0x80) of every byte in a `u64`; used to detect invalid bytes.
const HI: u64 = 0x8080_8080_8080_8080;

/// Repeat byte `b` into all eight bytes of a `u64` (e.g. `0x30` → `0x3030_3030_3030_3030`).
#[inline(always)]
const fn splat(b: u8) -> u64 {
    b as u64 * 0x0101_0101_0101_0101
}

/// Shared SWAR skeleton: check `v` 8 bytes at a time with `word_bad`, the rest byte by byte with
/// `byte_ok`.
///
/// `word_bad` must set the high bit of every invalid byte; bytes ≥ 0x80 are always invalid
/// (every ISO 8583 content type is 7-bit ASCII). Per-word results are OR-ed together without
/// early exit so the loop stays straight-line.
#[inline(always)]
fn swar_all(v: &[u8], word_bad: impl Fn(u64) -> u64, byte_ok: impl Fn(u8) -> bool) -> bool {
    let mut chunks = v.chunks_exact(8);
    let mut bad = 0u64;
    for c in &mut chunks {
        bad |= word_bad(u64::from_le_bytes(c.try_into().unwrap()));
    }
    bad & HI == 0 && chunks.remainder().iter().all(|&b| byte_ok(b))
}

/// `true` if every byte is a digit `0`–`9` (content type `n`). An empty slice is `true`.
#[inline]
pub fn all_digits(v: &[u8]) -> bool {
    // x < 0x30 → x - 0x30 sets the high bit; x > 0x39 → x + 0x46 ≥ 0x80.
    // A borrow/carry between bytes only happens when some byte is already invalid.
    swar_all(
        v,
        |x| x | x.wrapping_sub(splat(0x30)) | x.wrapping_add(splat(0x46)),
        |b| b.is_ascii_digit(),
    )
}

/// `true` if every byte is printable ASCII `0x20`–`0x7E` (content type `ans`).
#[inline]
pub fn all_printable(v: &[u8]) -> bool {
    // x < 0x20 → x - 0x20 sets the high bit; x = 0x7F → x + 1 = 0x80.
    swar_all(
        v,
        |x| x | x.wrapping_sub(splat(0x20)) | x.wrapping_add(splat(0x01)),
        |b| (0x20..=0x7E).contains(&b),
    )
}

/// Flag in the [`CLASS`] table: letter or space.
const ALPHA: u8 = 1;
/// Flag in the [`CLASS`] table: letter, digit, or space.
const ALNUM: u8 = 2;

/// Build the character class table at compile time. Space counts as valid for `a`/`an`
/// because fixed-length ISO 8583 fields are often space-padded.
const fn class_lut() -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        let b = i as u8;
        if b.is_ascii_alphabetic() || b == b' ' {
            t[i] |= ALPHA | ALNUM;
        }
        if b.is_ascii_digit() {
            t[i] |= ALNUM;
        }
        i += 1;
    }
    t
}

/// Character class table: one lookup per byte, no branching.
static CLASS: [u8; 256] = class_lut();

/// `true` if every byte is a letter or space (content type `a`).
#[inline]
pub fn all_alpha(v: &[u8]) -> bool {
    v.iter().fold(ALPHA, |acc, &b| acc & CLASS[b as usize]) == ALPHA
}

/// `true` if every byte is a letter, digit, or space (content type `an`).
#[inline]
pub fn all_alphanumeric(v: &[u8]) -> bool {
    v.iter().fold(ALNUM, |acc, &b| acc & CLASS[b as usize]) & ALNUM == ALNUM
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes() {
        assert!(all_digits(b"0123456789"));
        assert!(!all_digits(b"12a4"));
        assert!(!all_digits(b"+123"));
        assert!(all_alpha(b"AB cd"));
        assert!(!all_alpha(b"AB1"));
        assert!(all_alphanumeric(b"AB 12"));
        assert!(!all_alphanumeric(b"AB-12"));
        assert!(all_printable(b"~ !A1"));
        assert!(!all_printable(b"A\n"));
        assert!(all_digits(b""));
    }

    /// Every byte value at every position (including the < 8 remainder) must match a
    /// byte-by-byte check.
    #[test]
    fn swar_matches_bytewise() {
        for len in 1..=17 {
            for pos in 0..len {
                for b in 0..=255u8 {
                    let mut d = vec![b'5'; len];
                    d[pos] = b;
                    assert_eq!(
                        all_digits(&d),
                        b.is_ascii_digit(),
                        "digits len={} pos={} b={}",
                        len,
                        pos,
                        b
                    );
                    let mut p = vec![b'~'; len];
                    p[pos] = b;
                    assert_eq!(
                        all_printable(&p),
                        (0x20..=0x7E).contains(&b),
                        "printable {} {} {}",
                        len,
                        pos,
                        b
                    );
                    let mut a = vec![b'A'; len];
                    a[pos] = b;
                    let ok = b.is_ascii_alphanumeric() || b == b' ';
                    assert_eq!(all_alphanumeric(&a), ok, "alnum {} {} {}", len, pos, b);
                }
            }
        }
        // two invalid bytes in one word (exercises borrow/carry)
        for a in 0..=255u8 {
            for b in [0u8, 0x2F, 0x3A, 0x7F, 0x80, 0xFF] {
                let d = [b'1', a, b'2', b, b'3', b'4', b'5', b'6'];
                assert_eq!(all_digits(&d), a.is_ascii_digit() && b.is_ascii_digit());
            }
        }
    }
}
