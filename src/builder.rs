//! [`Builder`]: assemble an ISO 8583 message and write (pack) it into a buffer, without allocating.

use crate::bitmap::{bit, pop_field, u64_to_hex16, B1, HEX_UPPER};
use crate::charset::all_digits;
use crate::error::{too_short, Error};
use crate::length::write_len;
use crate::spec::CompiledSpec;

/// Message builder. Stores references to field values (no copies), so every value passed to
/// `set` must live until the message is packed.
#[derive(Debug, Clone)]
pub struct Builder<'a> {
    /// Spec used for validation and to know each field's length prefix.
    spec: &'a CompiledSpec,
    /// Message Type Indicator, already checked to be 4 digits.
    mti: [u8; 4],
    /// Data fields that have been set (without bit 1; bit 1 is computed at pack time).
    bitmap: u128,
    /// Total bytes of all fields including length prefixes, kept up to date by `set` so
    /// `packed_len` is O(1).
    body_len: usize,
    /// Value per field number; only read for fields whose bit is set in `bitmap`.
    // ponytail: 129 × 16 bytes (~2 KB) on the stack per builder (~70 ns to initialise); switch
    // to a compact list or an in-order writer if that shows up in a profile
    fields: [&'a [u8]; 129],
}

impl<'a> Builder<'a> {
    /// Start a new message with MTI `mti` (4 ASCII digits, e.g. `b"0200"`).
    ///
    /// # Errors
    /// [`Error::BadMti`] if it is not exactly 4 digits.
    pub fn new(spec: &'a CompiledSpec, mti: &[u8]) -> Result<Self, Error> {
        let mti: [u8; 4] = mti.try_into().map_err(|_| Error::BadMti)?;
        if !all_digits(&mti) {
            return Err(Error::BadMti);
        }
        Ok(Builder {
            spec,
            mti,
            bitmap: 0,
            body_len: 0,
            fields: [&[]; 129],
        })
    }

    /// Set field `n` to `v`. Calling it again for the same field overwrites the value.
    ///
    /// Length and content type are validated **here**, once, so packing never needs to
    /// re-check and every outgoing message matches the spec. Returns `&mut Self` so calls can
    /// be chained: `b.set(2, ..)?.set(3, ..)?`.
    ///
    /// # Errors
    /// [`Error::UnknownField`] if `n` is not 2..=128, equals 65 (bitmap marker), or is not in
    /// the spec; [`Error::BadLength`] / [`Error::Invalid`] if the value does not match the spec.
    pub fn set(&mut self, n: usize, v: &'a [u8]) -> Result<&mut Self, Error> {
        if !(2..=128).contains(&n) || n == 65 || !self.spec.fields[n].present {
            return Err(Error::UnknownField(n.min(255) as u8));
        }
        let d = self.spec.fields[n];
        d.validate(n, v)?;
        let p = d.len_type.prefix_len();
        if self.bitmap & bit(n) != 0 {
            self.body_len -= p + self.fields[n].len(); // the old value is being replaced
        }
        self.fields[n] = v;
        self.bitmap |= bit(n);
        self.body_len += p + v.len();
        Ok(self)
    }

    /// `true` if any of fields 65–128 is set, so the secondary bitmap must be written.
    #[inline]
    fn has_secondary(&self) -> bool {
        self.bitmap as u64 != 0
    }

    /// Size of the packed message in bytes, O(1). Useful to size a buffer or a TCP length
    /// header before packing.
    #[inline]
    pub fn packed_len(&self) -> usize {
        4 + if self.has_secondary() { 32 } else { 16 } + self.body_len
    }

    /// Write the message into `out` (e.g. a stack buffer) and return the number of bytes written.
    ///
    /// Order: MTI, hex bitmap (bit 1 is set automatically when any field > 64 is present), then
    /// fields in number order with a length prefix for variable fields. The bitmap is written
    /// in upper case.
    ///
    /// # Errors
    /// [`Error::TooShort`] if `out` is shorter than [`packed_len`](Self::packed_len).
    pub fn pack_to_slice(&self, out: &mut [u8]) -> Result<usize, Error> {
        let need = self.packed_len();
        let out = out.get_mut(..need).ok_or_else(too_short)?;
        let secondary = self.has_secondary();
        let bm = if secondary {
            self.bitmap | B1
        } else {
            self.bitmap
        };

        out[..4].copy_from_slice(&self.mti);
        out[4..20].copy_from_slice(&u64_to_hex16((bm >> 64) as u64, HEX_UPPER));
        let mut w = 20;
        if secondary {
            out[20..36].copy_from_slice(&u64_to_hex16(bm as u64, HEX_UPPER));
            w = 36;
        }

        let mut bits = bm & !B1;
        while bits != 0 {
            let n = pop_field(&mut bits);
            let v = self.fields[n];
            let p = self.spec.fields[n].len_type.prefix_len();
            if p > 0 {
                write_len(&mut out[w..w + p], v.len());
                w += p;
            }
            out[w..w + v.len()].copy_from_slice(v);
            w += v.len();
        }
        Ok(need)
    }

    /// Write the message into `out`, replacing its contents. Reuse the same `Vec` across
    /// messages: once its capacity is large enough, nothing is allocated.
    pub fn pack_into(&self, out: &mut Vec<u8>) {
        // pack_to_slice writes every byte in 0..need, so old contents need no zeroing
        let need = self.packed_len();
        if out.len() < need {
            out.resize(need, 0);
        } else {
            out.truncate(need);
        }
        self.pack_to_slice(out).expect("buffer sized by packed_len");
    }
}
