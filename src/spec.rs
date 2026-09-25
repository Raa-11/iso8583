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
            fields: serde_yml::from_str(yml).unwrap(),
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
                fields: serde_yml::from_str(yml).unwrap(),
            };
            assert!(
                matches!(CompiledSpec::compile(&spec), Err(Error::BadSpec(_))),
                "{}",
                yml
            );
        }
    }
}
