//! "Compiled" spec: the `[FieldDef; 129]` table used by the parser and the builder.
//!
//! Built once from the YAML file ([`Spec`]) and reused for every message.
//! No `HashMap` and no `String`: a field lookup is an array access, and the whole table
//! (~1 KB) fits in L1 cache.

use crate::charset::{all_alpha, all_alphanumeric, all_digits, all_printable};
use crate::error::Error;
use crate::specfile::{spec_from_file, Spec};

/// How a field's length is determined inside the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LenType {
    /// Fixed length = `max_len`, no prefix.
    Fixed,
    /// 2-digit length prefix (max 99).
    Llvar,
    /// 3-digit length prefix (max 999).
    Lllvar,
    /// 4-digit length prefix (max 9999).
    Llllvar,
}

impl LenType {
    /// Convert the YAML `LenType` text into the enum. `None` if unknown.
    /// The text is case-sensitive and must be lower case, as written in the spec file.
    ///
    /// # Examples
    /// ```
    /// use iso_8583_rs::LenType;
    ///
    /// assert_eq!(LenType::parse("llvar"), Some(LenType::Llvar));
    /// assert_eq!(LenType::parse("fixed"), Some(LenType::Fixed));
    /// assert_eq!(LenType::parse("LLVAR"), None); // case-sensitive
    /// assert_eq!(LenType::parse("xvar"), None);  // unknown
    /// ```
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "fixed" => Some(LenType::Fixed),
            "llvar" => Some(LenType::Llvar),
            "lllvar" => Some(LenType::Lllvar),
            "llllvar" => Some(LenType::Llllvar),
            _ => None,
        }
    }

    /// Number of length-prefix digits in front of the field value (0 for fixed).
    /// # Examples
    /// ```
    /// use iso_8583_rs::LenType;
    ///
    /// assert_eq!(LenType::Fixed.prefix_len(), 0);
    /// assert_eq!(LenType::Llvar.prefix_len(), 2);
    /// assert_eq!(LenType::Lllvar.prefix_len(), 3);
    /// assert_eq!(LenType::Llllvar.prefix_len(), 4);
    /// ```
    #[inline]
    pub fn prefix_len(self) -> usize {
        match self {
            LenType::Fixed => 0,
            LenType::Llvar => 2,
            LenType::Lllvar => 3,
            LenType::Llllvar => 4,
        }
    }
}

/// Which characters a field may contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ContentType {
    /// Not validated (e.g. `b`, `z`, `ns`, or any other unknown kind).
    Any,
    /// `n`: digits only.
    Numeric,
    /// `a`: letters and space.
    Alpha,
    /// `an`: letters, digits, and space.
    AlphaNumeric,
    /// `ans`: any printable ASCII.
    Printable,
}

impl ContentType {
    /// Convert the YAML `ContentType` text into the enum (case-insensitive).
    ///
    /// Unknown kinds become [`ContentType::Any`] rather than an error, so specs from other
    /// networks with custom kinds still load.
    /// # Examples
    /// ```
    /// use iso_8583_rs::ContentType;
    ///
    /// assert_eq!(ContentType::parse("n"), ContentType::Numeric);
    /// assert_eq!(ContentType::parse("ANS"), ContentType::Printable); // case-insensitive
    /// assert_eq!(ContentType::parse("b"), ContentType::Any);         // unknown: not checked
    /// ```
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "n" => ContentType::Numeric,
            "a" => ContentType::Alpha,
            "an" => ContentType::AlphaNumeric,
            "ans" => ContentType::Printable,
            _ => ContentType::Any,
        }
    }

    /// `true` if every byte of `v` matches this kind.
    /// # Examples
    /// ```
    /// use iso_8583_rs::ContentType;
    ///
    /// assert!(ContentType::Numeric.check(b"0123"));
    /// assert!(!ContentType::Numeric.check(b"01a3"));
    /// assert!(ContentType::AlphaNumeric.check(b"ATM 01"));
    /// assert!(ContentType::Any.check(&[0xFF, 0x00])); // never fails
    /// ```
    #[inline]
    pub fn check(self, v: &[u8]) -> bool {
        match self {
            ContentType::Any => true,
            ContentType::Numeric => all_digits(v),
            ContentType::Alpha => all_alpha(v),
            ContentType::AlphaNumeric => all_alphanumeric(v),
            ContentType::Printable => all_printable(v),
        }
    }
}

/// Compact definition of one field (8 bytes, `Copy`).
/// You normally do not build these yourself: they come from
/// [`CompiledSpec::fields`](CompiledSpec::fields), indexed by field number.
///
/// # Examples
/// ```
/// use iso_8583_rs::{CompiledSpec, ContentType, LenType};
///
/// let spec = CompiledSpec::from_file("spec1987.yml")?;
/// let pan = spec.fields[2]; // field 2: primary account number
/// assert!(pan.present);
/// assert_eq!(pan.len_type, LenType::Llvar);
/// assert_eq!(pan.content, ContentType::Numeric);
/// assert_eq!((pan.min_len, pan.max_len), (12, 19));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct FieldDef {
    /// Maximum length; for `Fixed` this is the exact length.
    pub max_len: u16,
    /// Minimum length (only meaningful for variable fields).
    pub min_len: u16,
    /// How the length is determined.
    pub len_type: LenType,
    /// Allowed characters.
    pub content: ContentType,
    /// `false` if this field is not in the spec; messages using it are rejected.
    pub present: bool,
}

impl FieldDef {
    /// Empty entry for field numbers not defined in the spec.
    const EMPTY: FieldDef = FieldDef {
        max_len: 0,
        min_len: 0,
        len_type: LenType::Fixed,
        content: ContentType::Any,
        present: false,
    };

    /// Validate value `v` for field number `n`: length must match `LenType` and content must
    /// match `ContentType`. Shared by [`Builder::set`](crate::Builder::set),
    /// [`Message::validate`](crate::Message::validate), and
    /// [`Message::get_validated`](crate::Message::get_validated).
    ///
    /// # Errors
    /// [`Error::BadLength`] if the length is wrong, [`Error::Invalid`] if a character is invalid.
    /// # Examples
    /// ```
    /// use iso_8583_rs::{CompiledSpec, Error};
    ///
    /// let spec = CompiledSpec::from_file("spec1987.yml")?;
    ///
    /// let terminal = spec.fields[41]; // fixed length 8, printable ASCII
    /// assert_eq!(terminal.validate(41, b"TERM0001"), Ok(()));
    /// assert_eq!(terminal.validate(41, b"TERM1"), Err(Error::BadLength(41)));
    ///
    /// let pan = spec.fields[2]; // digits only, 12 to 19 long
    /// assert_eq!(pan.validate(2, b"4111111111111111"), Ok(()));
    /// assert_eq!(pan.validate(2, b"41111111111111AB"), Err(Error::Invalid(2)));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[inline]
    pub fn validate(&self, n: usize, v: &[u8]) -> Result<(), Error> {
        let len_ok = match self.len_type {
            LenType::Fixed => v.len() == self.max_len as usize,
            _ => (self.min_len as usize..=self.max_len as usize).contains(&v.len()),
        };
        if !len_ok {
            return Err(Error::BadLength(n as u8));
        }
        if !self.content.check(v) {
            return Err(Error::Invalid(n as u8));
        }
        Ok(())
    }
}

/// Ready-to-use spec. Build once at startup, then share it (e.g. `&'static` or `Arc`).
/// A `CompiledSpec` is immutable, so one instance can be shared by every thread (for example
/// behind an [`Arc`](std::sync::Arc) or a `static`).
///
/// # Examples
/// ```
/// use std::sync::Arc;
/// use iso_8583_rs::CompiledSpec;
///
/// let spec = Arc::new(CompiledSpec::from_file("spec1987.yml")?);
///
/// let worker = {
///     let spec = Arc::clone(&spec);
///     std::thread::spawn(move || spec.fields[2].max_len)
/// };
/// assert_eq!(worker.join().unwrap(), 19);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct CompiledSpec {
    /// Index = field number (0 and 1 unused). Undefined fields have `present == false`.
    pub fields: [FieldDef; 129],
}

impl CompiledSpec {
    /// Turn a [`Spec`] read from YAML into a ready-to-use table, rejecting nonsensical
    /// definitions so spec mistakes show up at startup rather than under live traffic.
    ///
    /// Entries 0 (MTI), 1 and 65 (bitmap markers) are ignored because the parser handles them
    /// directly.
    ///
    /// # Errors
    /// [`Error::BadSpec`] if a field number is outside 0..=128, `LenType` is unknown,
    /// `MaxLen` exceeds the prefix capacity (e.g. `llvar` > 99), `MinLen > MaxLen`,
    /// or a `fixed` field has `MaxLen` 0.
    /// # Examples
    /// ```
    /// use std::collections::HashMap;
    /// use iso_8583_rs::specfile::{FieldDescription, Spec};
    /// use iso_8583_rs::{CompiledSpec, Error};
    ///
    /// let field = |len_type: &str, max_len, min_len| FieldDescription {
    ///     content_type: "n".into(),
    ///     len_type: len_type.into(),
    ///     max_len,
    ///     min_len,
    ///     label: String::new(),
    /// };
    ///
    /// // A valid spec: field 2 is LLVAR up to 19 digits, field 3 is a fixed 6 digits.
    /// let spec = Spec {
    ///     fields: HashMap::from([(2, field("llvar", 19, 12)), (3, field("fixed", 6, 0))]),
    /// };
    /// let compiled = CompiledSpec::compile(&spec)?;
    /// assert!(compiled.fields[3].present);
    ///
    /// // A nonsensical definition is rejected up front: LLVAR cannot hold more than 99.
    /// let bad = Spec { fields: HashMap::from([(2, field("llvar", 100, 0))]) };
    /// assert_eq!(CompiledSpec::compile(&bad).unwrap_err(), Error::BadSpec(2));
    /// # Ok::<(), Error>(())
    /// ```
    pub fn compile(spec: &Spec) -> Result<Self, Error> {
        let mut fields = [FieldDef::EMPTY; 129];
        for (&key, d) in &spec.fields {
            let n = key.clamp(0, 255) as u8;
            if key == 0 || key == 1 || key == 65 {
                continue;
            }
            if !(2..=128).contains(&key) {
                return Err(Error::BadSpec(n));
            }
            let len_type = LenType::parse(&d.len_type).ok_or(Error::BadSpec(n))?;
            let cap = match len_type.prefix_len() {
                0 => u16::MAX as usize,
                p => 10usize.pow(p as u32) - 1,
            };
            let bad_fixed = len_type == LenType::Fixed && d.max_len == 0;
            if d.max_len > cap || d.min_len > d.max_len || bad_fixed {
                return Err(Error::BadSpec(n));
            }
            fields[key as usize] = FieldDef {
                max_len: d.max_len as u16,
                min_len: d.min_len as u16,
                len_type,
                content: ContentType::parse(&d.content_type),
                present: true,
            };
        }
        Ok(CompiledSpec { fields })
    }

    /// Read a YAML file, then [`compile`](Self::compile) it. The usual way to create a spec.
    ///
    /// # Errors
    /// The file cannot be read or parsed, or the spec is invalid.
    /// # Examples
    /// ```
    /// use iso_8583_rs::CompiledSpec;
    ///
    /// let spec = CompiledSpec::from_file("spec1987.yml")?;
    /// assert!(spec.fields[41].present);
    ///
    /// // A missing file is an error, not a panic.
    /// assert!(CompiledSpec::from_file("does-not-exist.yml").is_err());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn from_file(filename: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::compile(&spec_from_file(filename)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Original Go spec format (mofax/iso8583): `MinLen` is optional, and entries 0 (MTI) and
    /// 1 (bitmap) exist.
    #[test]
    fn accepts_mofax_style_spec() {
        let yml = "0:\n  ContentType: \"n\"\n  Label: MTI\n  LenType: fixed\n  MaxLen: 4\n\
1:\n  ContentType: \"b\"\n  Label: Bitmap\n  LenType: fixed\n  MaxLen: 8\n\
3:\n  ContentType: \"n\"\n  Label: Processing code\n  LenType: fixed\n  MaxLen: 6\n";
        let spec = Spec {
            fields: serde_saphyr::from_str(yml).unwrap(),
        };
        let c = CompiledSpec::compile(&spec).unwrap();
        assert!(!c.fields[0].present && !c.fields[1].present);
        assert!(c.fields[3].present);
        assert_eq!(c.fields[3].min_len, 0);
    }

    #[test]
    fn rejects_invalid_definitions() {
        let bad = [
            "2:\n  ContentType: n\n  MaxLen: 100\n  LenType: llvar\n", // > 99
            "2:\n  ContentType: n\n  MaxLen: 5\n  MinLen: 6\n  LenType: llvar\n",
            "2:\n  ContentType: n\n  MaxLen: 5\n  LenType: xvar\n",
            "200:\n  ContentType: n\n  MaxLen: 5\n  LenType: fixed\n",
        ];
        for yml in bad {
            let spec = Spec {
                fields: serde_saphyr::from_str(yml).unwrap(),
            };
            assert!(
                matches!(CompiledSpec::compile(&spec), Err(Error::BadSpec(_))),
                "{}",
                yml
            );
        }
    }
}
