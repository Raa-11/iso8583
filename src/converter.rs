//! **Legacy API.** Convert a bitmap in bit-array form (`Vec<i64>` of 0/1) ↔ hex.
//!
//! Used by [`IsoStruct`](crate::iso8583::IsoStruct). The new API stores the bitmap as a `u128`
//! (see [`Message::bitmap`](crate::Message::bitmap)).

use crate::bitmap::{HEX_LOWER, HEX_LUT};

/// Convert a bit array (0/1, length a multiple of 8) into lower-case hex, 4 bits per character.
///
/// Any non-zero value counts as 1, matching the first version's behaviour.
///
/// # Errors
/// If the array length is not a multiple of 8.
pub fn bitmap_array_to_hex(arr: &[i64]) -> Result<String, Box<dyn std::error::Error>> {
    if !arr.len().is_multiple_of(8) {
        return Err("invalid iso8583 bitmap array".into());
    }
    let mut out = String::with_capacity(arr.len() / 4);
    for nibble in arr.chunks_exact(4) {
        let v = nibble
            .iter()
            .fold(0usize, |acc, &b| (acc << 1) | (b != 0) as usize);
        out.push(HEX_LOWER[v] as char);
    }
    Ok(out)
}

/// Convert hex (upper or lower case) into a 0/1 bit array, 4 bits per character.
///
/// # Errors
/// If the hex length is odd or a character is not hex.
pub fn hex_to_bitmap_array(hex_string: &str) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
    if !hex_string.len().is_multiple_of(2) {
        return Err("odd hex length".into());
    }
    let mut bits = Vec::with_capacity(hex_string.len() * 4);
    for c in hex_string.bytes() {
        let n = HEX_LUT[c as usize];
        if n > 0xF {
            return Err(format!("invalid hex character {:?}", c as char).into());
        }
        bits.extend((0..4).rev().map(|i| ((n >> i) & 1) as i64));
    }
    Ok(bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let bits = hex_to_bitmap_array("F23A0000000000FF").unwrap();
        assert_eq!(bits.len(), 64);
        assert_eq!(&bits[..8], &[1, 1, 1, 1, 0, 0, 1, 0]);
        assert_eq!(bitmap_array_to_hex(&bits).unwrap(), "f23a0000000000ff");
        assert!(hex_to_bitmap_array("0G").is_err());
        assert!(bitmap_array_to_hex(&[1, 0, 1]).is_err());
    }
}
