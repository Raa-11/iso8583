//! [`Message`]: parse an ISO 8583 message and read its fields, without copying or allocating.
//!
//! Wire format: `MTI (4 ASCII digits) | hex ASCII bitmap (16 or 32 characters) | fields...`

use crate::bitmap::{bit, hex16_to_u64, pop_field, B1, B65};
use crate::charset::all_digits;
use crate::error::{too_short, Error};
use crate::length::parse_len;
use crate::numeric::parse_digits_u64;
use crate::spec::CompiledSpec;

/// A parsed message. Borrows the input buffer (`'a`): field values are slices into it.
///
/// The trade-off: the input buffer must outlive the `Message`; in return no `String`/`Vec` is
/// created per field.
#[derive(Debug, Clone)]
pub struct Message<'a> {
    /// The original message buffer; every field value points into it.
    buf: &'a [u8],
    /// Message Type Indicator, e.g. `*b"0200"`.
    pub mti: [u8; 4],
    /// Primary + secondary bitmap: bit 127 = field 1 … bit 0 = field 128.
    pub bitmap: u128,
    /// `(offset, len)` for each present field, ordered by field number.
    /// The slot index of field `n` = number of present fields before `n` (via `count_ones`).
    slots: [(u16, u16); 128],
}

impl<'a> Message<'a> {
    /// Parse the structure **and** validate every field's content (minimum length, content type).
    ///
    /// The default choice for input from outside. Validation runs in the same loop as parsing,
    /// while the field data is still in cache.
    ///
    /// # Errors
    /// Every structural error from [`parse_lazy`](Self::parse_lazy), plus
    /// [`Error::BadLength`] (shorter than `MinLen`) and [`Error::Invalid`] (wrong characters).
    pub fn parse(spec: &CompiledSpec, buf: &'a [u8]) -> Result<Self, Error> {
        Self::parse_impl::<true>(spec, buf)
    }

    /// Parse the structure only: MTI, bitmap, length prefixes, and buffer bounds.
    ///
    /// Field content is not checked; use [`get_validated`](Self::get_validated) for the fields
    /// you read, or [`validate`](Self::validate) for all of them. Useful for routers that only
    /// read a few fields. Buffer bounds are always checked, so it is safe for untrusted input.
    ///
    /// # Errors
    /// [`Error::TooShort`], [`Error::TooLong`] (> 65535 bytes), [`Error::BadMti`],
    /// [`Error::BadHex`], [`Error::TertiaryUnsupported`], [`Error::UnknownField`],
    /// [`Error::BadLength`] (prefix is not digits or exceeds `MaxLen`), [`Error::TrailingBytes`].
    pub fn parse_lazy(spec: &CompiledSpec, buf: &'a [u8]) -> Result<Self, Error> {
        Self::parse_impl::<false>(spec, buf)
    }

    /// Shared implementation of `parse` and `parse_lazy`.
    ///
    /// `STRICT` is a const generic, so the compiler emits two separate versions and the
    /// `if STRICT` branch disappears at compile time; lazy mode pays nothing for validation.
    #[inline(always)]
    fn parse_impl<const STRICT: bool>(spec: &CompiledSpec, buf: &'a [u8]) -> Result<Self, Error> {
        // offsets are stored as u16 in `slots`
        if buf.len() > u16::MAX as usize {
            return Err(Error::TooLong);
        }
        let mti: [u8; 4] = buf.get(..4).ok_or_else(too_short)?.try_into().unwrap();
        if !all_digits(&mti) {
            return Err(Error::BadMti);
        }

        // Bitmap: 16 hex primary, plus 16 hex secondary when the field 1 bit is set.
        let primary = hex16_to_u64(buf.get(4..20).ok_or_else(too_short)?.try_into().unwrap())?;
        let (bitmap, mut off) = if primary >> 63 == 1 {
            let sec = hex16_to_u64(buf.get(20..36).ok_or_else(too_short)?.try_into().unwrap())?;
            (((primary as u128) << 64) | sec as u128, 36)
        } else {
            ((primary as u128) << 64, 20)
        };
        if bitmap & B65 != 0 {
            return Err(Error::TertiaryUnsupported);
        }

        // Fields are read in field-number order; each one shifts the offset for the next.
        let mut slots = [(0u16, 0u16); 128];
        let mut i = 0;
        let mut bits = bitmap & !B1;
        while bits != 0 {
            let n = pop_field(&mut bits);
            let d = spec.fields[n];
            if !d.present {
                return Err(Error::UnknownField(n as u8));
            }
            let p = d.len_type.prefix_len();
            let len = if p == 0 {
                d.max_len as usize
            } else {
                let l = parse_len(buf.get(off..off + p).ok_or_else(too_short)?)
                    .ok_or(Error::BadLength(n as u8))?;
                if l > d.max_len as usize {
                    return Err(Error::BadLength(n as u8));
                }
                off += p;
                l
            };
            let end = off + len;
            if end > buf.len() {
                return Err(too_short());
            }
            if STRICT {
                // max_len and the fixed length are already guaranteed by the structure above
                if len < d.min_len as usize {
                    return Err(Error::BadLength(n as u8));
                }
                if !d.content.check(&buf[off..end]) {
                    return Err(Error::Invalid(n as u8));
                }
            }
            slots[i] = (off as u16, len as u16);
            i += 1;
            off = end;
        }
        if off != buf.len() {
            return Err(Error::TrailingBytes);
        }
        Ok(Message {
            buf,
            mti,
            bitmap,
            slots,
        })
    }

    /// Validate every field's content. For a `parse_lazy` result that turns out to need a
    /// full check.
    ///
    /// # Errors
    /// The first error found; see [`FieldDef::validate`](crate::FieldDef::validate).
    pub fn validate(&self, spec: &CompiledSpec) -> Result<(), Error> {
        for (n, v) in self.fields() {
            spec.fields[n].validate(n, v)?;
        }
        Ok(())
    }

    /// `true` if data field `n` (2..=128) is present. Field 1 is always `false` because it is
    /// a bitmap marker, not data.
    #[inline]
    pub fn has(&self, n: usize) -> bool {
        (2..=128).contains(&n) && self.bitmap & bit(n) != 0
    }

    /// Raw value of field `n`, without content validation. `None` if the field is absent.
    ///
    /// O(1): the slot position is computed with `count_ones` (number of fields before `n`),
    /// not by searching.
    #[inline]
    pub fn get(&self, n: usize) -> Option<&'a [u8]> {
        if !self.has(n) {
            return None;
        }
        let before = ((self.bitmap & !B1) >> (129 - n)).count_ones() as usize;
        let (off, len) = self.slots[before];
        Some(&self.buf[off as usize..off as usize + len as usize])
    }

    /// Like [`get`](Self::get), but validates the field's content first. The companion to
    /// `parse_lazy`.
    ///
    /// # Errors
    /// [`Error::BadLength`] or [`Error::Invalid`] if the content does not match the spec.
    pub fn get_validated(&self, spec: &CompiledSpec, n: usize) -> Result<Option<&'a [u8]>, Error> {
        match self.get(n) {
            Some(v) => spec.fields[n].validate(n, v).map(|_| Some(v)),
            None => Ok(None),
        }
    }

    /// Field `n` as `&str`. `None` if absent or not valid UTF-8.
    #[inline]
    pub fn get_str(&self, n: usize) -> Option<&'a str> {
        std::str::from_utf8(self.get(n)?).ok()
    }

    /// Numeric field `n` as `u64` (e.g. amount in field 4, STAN in field 11).
    ///
    /// `None` if absent, not all digits, or longer than 19 digits.
    /// See [`parse_digits_u64`].
    #[inline]
    pub fn get_u64(&self, n: usize) -> Option<u64> {
        parse_digits_u64(self.get(n)?)
    }

    /// Every present data field as `(number, value)`, ordered by field number.
    pub fn fields(&self) -> impl Iterator<Item = (usize, &'a [u8])> + '_ {
        let mut bits = self.bitmap & !B1;
        let mut i = 0;
        std::iter::from_fn(move || {
            if bits == 0 {
                return None;
            }
            let n = pop_field(&mut bits);
            let (off, len) = self.slots[i];
            i += 1;
            Some((n, &self.buf[off as usize..off as usize + len as usize]))
        })
    }
}
