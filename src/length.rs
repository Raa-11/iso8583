//! Length prefixes for variable-length fields (LLVAR = 2 digits, LLLVAR = 3, LLLLVAR = 4), in ASCII.
//!
//! Example: an LLVAR field containing `"4111"` is sent as `"04" + "4111"`.

/// Read a 2..=4 digit ASCII length prefix into a number.
///
/// Written by hand (instead of `str::parse`) because the input is only 2–4 bytes: no UTF-8
/// validation, no allocation, and the compiler can unroll the loop.
///
/// Returns `None` if any byte is not a digit.
#[inline]
pub(crate) fn parse_len(d: &[u8]) -> Option<usize> {
    let mut v = 0usize;
    for &b in d {
        let x = b.wrapping_sub(b'0');
        if x > 9 {
            return None;
        }
        v = v * 10 + x as usize;
    }
    Some(v)
}

/// Write `len` as right-aligned, zero-padded ASCII digits filling all of `out`.
///
/// Example: `len = 7`, `out.len() = 3` → `"007"`. The caller must make sure `len` fits
/// (already guaranteed by the spec's `max_len` validation).
#[inline]
pub(crate) fn write_len(out: &mut [u8], mut len: usize) {
    for o in out.iter_mut().rev() {
        *o = b'0' + (len % 10) as u8;
        len /= 10;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for (width, len) in [(2, 0), (2, 99), (3, 7), (3, 999), (4, 1234)] {
            let mut b = [0u8; 4];
            write_len(&mut b[..width], len);
            assert_eq!(parse_len(&b[..width]), Some(len));
        }
        assert_eq!(parse_len(b"1a"), None);
        assert_eq!(parse_len(b"+1"), None);
    }
}
