//! **Legacy API.** First-version MTI and field-length validators, kept for compatibility.
//!
//! The new API validates automatically in [`Builder::set`](crate::Builder::set) and
//! [`Message::parse`](crate::Message::parse); fast character validators live in [`charset`](crate::charset).

use crate::charset::all_digits;
use crate::iso8583::MtiType;

/// Check the MTI: exactly 4 characters, all digits (`+`/`-` signs are rejected).
///
/// # Errors
/// A text error if the length is not 4 or a character is not a digit.
pub fn mti_validator(mti: &MtiType) -> Result<bool, Box<dyn std::error::Error>> {
    if mti.mti.len() != 4 {
        return Err("MTI must be length (4)".into());
    }
    if !all_digits(mti.mti.as_bytes()) {
        return Err("MTI can only contain integers".into());
    }
    Ok(true)
}

/// Check that `data` is exactly `length` long (for fixed fields).
///
/// # Errors
/// A text error with the field number and the expected length.
pub fn fixed_length_integer_validator(
    field: usize,
    length: usize,
    data: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    if length != data.len() {
        return Err(format!(
            "field {}: expected length {} found {} instead",
            field,
            length,
            data.len()
        )
        .into());
    }
    Ok(true)
}

/// Check that `data`'s length is within `min..=max` (for variable fields).
///
/// # Errors
/// A text error with the field number and the expected range.
pub fn variable_length_integer_validator(
    field: usize,
    min: usize,
    max: usize,
    data: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    if data.len() < min || data.len() > max {
        return Err(format!(
            "field {}: expected max length {} and min length {} found {}",
            field,
            max,
            min,
            data.len()
        )
        .into());
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mti() {
        assert!(mti_validator(&MtiType { mti: "+123".into() }).is_err());
        assert!(mti_validator(&MtiType { mti: "020".into() }).is_err());
        assert!(mti_validator(&MtiType { mti: "0200".into() }).is_ok());
    }
}
