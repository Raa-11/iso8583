//! **Legacy API.** [`IsoStruct`]: a message in owned form (`String`, `HashMap`, `Vec<i64>` bitmap).
//!
//! Kept so code written against the first version keeps working; it is now built on the new
//! parser, so it no longer panics on malformed input. For performance use
//! [`CompiledSpec`] + [`Message`] / [`Builder`](crate::Builder).

use crate::converter::bitmap_array_to_hex;
use crate::error::Error;
use crate::message::Message;
use crate::spec::{CompiledSpec, LenType};
use crate::specfile::{spec_from_file, Spec};
use crate::validators::mti_validator;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Write;

/// Owned Message Type Indicator, e.g. `"0200"`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MtiType {
    /// The MTI value.
    pub mti: String,
}

impl MtiType {
    /// The MTI as `&str`. (The `to_string` name is kept from the first version.)
    #[allow(
        clippy::inherent_to_string_shadow_display,
        clippy::wrong_self_convention
    )]
    pub fn to_string(&self) -> &str {
        &self.mti
    }
}

/// Field values: field number → value.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ElementsType {
    /// Value per field number.
    elements: HashMap<i64, String>,
}

impl ElementsType {
    /// Read access to all field values.
    pub fn get_elements(&self) -> &HashMap<i64, String> {
        &self.elements
    }
}

/// Legacy ISO 8583 message together with its spec.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IsoStruct {
    /// Spec as read from YAML (not compiled).
    pub spec: Spec,
    /// Message MTI.
    pub mti: MtiType,
    /// Bitmap as an array of 0/1 bits: index 0 = field 1. Length 64 or 128.
    pub bitmap: Vec<i64>,
    /// Field values.
    pub elements: ElementsType,
}

impl IsoStruct {
    /// Build the full message: MTI + hex bitmap (lower case) + fields.
    ///
    /// # Errors
    /// See [`pack_elements`](Self::pack_elements); also if the bitmap length is invalid.
    #[allow(clippy::inherent_to_string_shadow_display)]
    pub fn to_string(&self) -> Result<String, Box<dyn std::error::Error>> {
        let elements = self.pack_elements()?;
        let mut out = String::with_capacity(4 + self.bitmap.len() / 4 + elements.len());
        out.push_str(&self.mti.mti);
        out.push_str(&bitmap_array_to_hex(&self.bitmap)?);
        out.push_str(&elements);
        Ok(out)
    }

    /// Set the MTI after checking it is 4 digits.
    ///
    /// # Errors
    /// If the MTI is not exactly 4 digits.
    pub fn add_mti(&mut self, data: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mti = MtiType {
            mti: data.to_string(),
        };
        mti_validator(&mti)?;
        self.mti = mti;
        Ok(())
    }

    /// Set field `field` and turn on its bit. The value is validated in `to_string`.
    ///
    /// # Errors
    /// If `field` is outside 2..=bitmap length, or is 65 (tertiary bitmap marker).
    pub fn add_field(&mut self, field: i64, value: &str) -> Result<(), Box<dyn std::error::Error>> {
        let bitmap_len = self.bitmap.len() as i64;
        if field < 2 || field > bitmap_len || field == 65 {
            return Err(format!(
                "expected field to be between 2 and {} (excluding 65) found {} instead",
                bitmap_len, field
            )
            .into());
        }
        self.bitmap[(field - 1) as usize] = 1;
        self.elements.elements.insert(field, value.to_string());
        Ok(())
    }

    /// Parse message `i` using `self`'s spec, returning a new `IsoStruct`.
    ///
    /// Performance note: the spec is compiled and cloned on every call (legacy behaviour).
    /// Fast path: build a `CompiledSpec` once and use `Message::parse`.
    ///
    /// # Errors
    /// Every error from [`Message::parse_lazy`](crate::Message::parse_lazy), an invalid spec,
    /// or a field that is not UTF-8.
    pub fn parse(&self, i: &str) -> Result<IsoStruct, Box<dyn std::error::Error>> {
        let spec = CompiledSpec::compile(&self.spec)?;
        let msg = Message::parse_lazy(&spec, i.as_bytes())?;

        let width = if msg.bitmap >> 127 == 1 { 128 } else { 64 };
        let bitmap = (0..width)
            .map(|k| ((msg.bitmap >> (127 - k)) & 1) as i64)
            .collect();
        let mut elements = HashMap::new();
        for (n, v) in msg.fields() {
            elements.insert(n as i64, std::str::from_utf8(v)?.to_string());
        }

        Ok(IsoStruct {
            spec: self.spec.clone(),
            mti: MtiType {
                mti: i[..4].to_string(),
            }, // safe: parse_lazy already checked 4 ASCII digits
            bitmap,
            elements: ElementsType { elements },
        })
    }

    /// Build the field section only (no MTI or bitmap), in field-number order.
    ///
    /// Each value's length is checked against the spec, because a wrong length produces a
    /// corrupt message on the receiving side.
    ///
    /// # Errors
    /// Field 65 is set, a field is not in the spec, a bit is set with no value, `LenType` is
    /// unknown, or a value's length does not match the spec.
    pub fn pack_elements(&self) -> Result<String, Box<dyn std::error::Error>> {
        let mut out = String::new();
        for (index, &b) in self.bitmap.iter().enumerate().skip(1) {
            if b != 1 {
                continue;
            }
            let field = (index + 1) as i64;
            if field == 65 {
                return Err(Error::TertiaryUnsupported.into());
            }
            let n = field.min(255) as u8;
            let desc = self.spec.fields.get(&field).ok_or(Error::UnknownField(n))?;
            let value = self
                .elements
                .elements
                .get(&field)
                .ok_or(Error::Invalid(n))?;
            let len_type = LenType::parse(&desc.len_type).ok_or(Error::BadSpec(n))?;
            let p = len_type.prefix_len();
            let ok = if p == 0 {
                value.len() == desc.max_len
            } else {
                value.len() <= desc.max_len && value.len() < 10usize.pow(p as u32)
            };
            if !ok {
                return Err(Error::BadLength(n).into());
            }
            if p > 0 {
                write!(out, "{:0width$}", value.len(), width = p)?;
            }
            out.push_str(value);
        }
        Ok(out)
    }
}

/// Create an empty `IsoStruct` using the spec in file `filename`.
///
/// `secondary_bitmap = true` prepares a 128-bit bitmap (fields 65–128 usable) and sets bit 1.
///
/// # Errors
/// The spec file cannot be read or parsed.
pub fn new_iso_struct(
    filename: &str,
    secondary_bitmap: bool,
) -> Result<IsoStruct, Box<dyn std::error::Error>> {
    let mut bitmap = vec![0; if secondary_bitmap { 128 } else { 64 }];
    if secondary_bitmap {
        bitmap[0] = 1;
    }
    Ok(IsoStruct {
        spec: spec_from_file(filename)?,
        mti: MtiType { mti: String::new() },
        bitmap,
        elements: ElementsType {
            elements: HashMap::new(),
        },
    })
}
