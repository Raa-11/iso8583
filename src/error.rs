//! Error type for parsing, building, and loading specs.

use std::fmt;

/// Every error this library can return.
///
/// `Copy` and small (2 bytes), so creating and returning an error never allocates. This matters
/// because malformed messages from the network can arrive in large volumes.
/// Field numbers are stored as `u8` (0..=128).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Message is truncated: data ran out before the MTI, bitmap, or a field was complete.
    /// Also returned when the output buffer is too small to pack into.
    TooShort,
    /// Message is longer than 65535 bytes.
    TooLong,
    /// MTI is not exactly 4 ASCII digits.
    BadMti,
    /// Bitmap contains a non-hex character.
    BadHex,
    /// Wrong field length: prefix is not digits, exceeds `MaxLen`, is below `MinLen`, or does
    /// not equal the fixed length.
    BadLength(u8),
    /// Field is present in the bitmap but not defined in the spec (or is not a valid field number).
    UnknownField(u8),
    /// Field 65 bit is set (tertiary bitmap, fields 129–192); not supported.
    TertiaryUnsupported,
    /// Bytes remain after the last field; usually means the spec does not match the sender's.
    TrailingBytes,
    /// Field content does not match its content type (e.g. a letter in an `n` field), or a
    /// value is missing.
    Invalid(u8),
    /// A field definition in the spec makes no sense (see [`CompiledSpec::compile`](crate::CompiledSpec::compile)).
    BadSpec(u8),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TooShort => f.write_str("message too short"),
            Error::TooLong => f.write_str("message longer than 65535 bytes"),
            Error::BadMti => f.write_str("MTI must be 4 digits"),
            Error::BadHex => f.write_str("invalid hex in bitmap"),
            Error::BadLength(n) => write!(f, "field {}: invalid length", n),
            Error::UnknownField(n) => write!(f, "field {}: not in spec", n),
            Error::TertiaryUnsupported => f.write_str("tertiary bitmap (field 65) not supported"),
            Error::TrailingBytes => f.write_str("trailing bytes after last field"),
            Error::Invalid(n) => write!(f, "field {}: invalid value", n),
            Error::BadSpec(n) => write!(f, "spec field {}: invalid definition", n),
        }
    }
}

impl std::error::Error for Error {}

/// Build [`Error::TooShort`] outside the hot path.
///
/// `#[cold]` + `#[inline(never)]` tell the compiler this is rare, so the error code is placed
/// away from the parse loop and the loop stays small.
#[cold]
#[inline(never)]
pub(crate) fn too_short() -> Error {
    Error::TooShort
}
