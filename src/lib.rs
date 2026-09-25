//! Fast, zero-copy, zero-allocation ISO 8583 parser and builder driven by a YAML spec.
//!
//! ISO 8583 is the message format behind card payments (ATM, POS, switch-to-switch). Every host
//! defines its own layout, so here the layout is a YAML file: you load it once, then parse and
//! build messages against it without allocating.
//!
//! # How it fits together
//!
//! | Step | Type | What it does |
//! |---|---|---|
//! | 1 | [`CompiledSpec`] | Reads the YAML spec once at startup and turns it into a flat lookup table. |
//! | 2 | [`Builder`] | Assembles an outgoing message, validates each field, and writes it to a buffer. |
//! | 3 | [`Message`] | Parses an incoming message and lets you read its fields without copying. |
//!
//! # Quick start
//!
//! ```
//! use iso_8583_rs::{Builder, CompiledSpec, Message};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Load and compile the spec once at startup.
//! let spec = CompiledSpec::from_file("spec1987.yml")?;
//!
//! // Build: values are borrowed and validated once on `set`.
//! let mut req = Builder::new(&spec, b"0200")?;
//! req.set(2, b"4111111111111111")?.set(4, b"000000010000")?;
//! let mut buf = Vec::new();
//! req.pack_into(&mut buf);
//!
//! // Parse: no copies, field values are slices into `buf`.
//! let msg = Message::parse(&spec, &buf)?;
//! assert_eq!(msg.get(2), Some(&b"4111111111111111"[..]));
//! assert_eq!(msg.get_u64(4), Some(10_000));
//! # Ok(())
//! # }
//! ```
//!
//! # Reading fields
//!
//! | Method | Returns |
//! |---|---|
//! | [`Message::get`] | the raw bytes, `Option<&[u8]>` |
//! | [`Message::get_str`] | `Option<&str>` |
//! | [`Message::get_u64`] | `Option<u64>` for numeric fields (amount, STAN, dates) |
//! | [`Message::get_validated`] | the raw bytes after checking them against the spec |
//! | [`Message::fields`] | every present field, in field-number order |
//!
//! # Strict or lazy parsing
//!
//! [`Message::parse`] checks the structure **and** every field's content in a single pass; use
//! it for input from outside. [`Message::parse_lazy`] checks only the structure and leaves
//! content checks to [`Message::get_validated`], which suits a router that reads a few fields.
//! Buffer bounds are checked in both modes, so neither can panic on malformed input.
//!
//! # Handling errors
//!
//! Every failure is a small `Copy` [`Error`] you can match on. Errors never allocate, and
//! malformed messages return an error instead of panicking.
//!
//! # Limitations
//!
//! ASCII encoding only (no BCD/EBCDIC), hex ASCII bitmap, fields 1–128, messages up to 65 535
//! bytes. See the [README](https://github.com/Raa-11/iso8583#readme) for the full list and
//! for a usage guide with more examples.
//!
//! # Modules
//!
//! | Module | Contents |
//! |---|---|
//! | [`message`] | [`Message`]: parse and read messages |
//! | [`builder`] | [`Builder`]: build and pack messages |
//! | [`spec`] | [`CompiledSpec`]: ready-to-use spec, [`FieldDef`], [`LenType`], [`ContentType`] |
//! | [`specfile`] | YAML file format and [`spec_from_file`](specfile::spec_from_file) |
//! | [`charset`] | `n` / `a` / `an` / `ans` character validation (SWAR) |
//! | [`numeric`] | ASCII digits → `u64` (SWAR) |
//! | [`error`] | [`Error`] |
//! | [`iso8583`], [`converter`], [`validators`], [`strpad`] | legacy API, kept for compatibility |

#![warn(missing_docs)]

mod bitmap;
pub mod builder;
pub mod charset;
pub mod error;
mod length;
pub mod message;
pub mod numeric;
pub mod spec;
pub mod specfile;

// Legacy API (first version), kept so existing code keeps compiling.
pub mod converter;
pub mod iso8583;
pub mod strpad;
pub mod validators;

// Compile and run every ```rust block in README.md as a doctest, so the README cannot drift
// from the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use builder::Builder;
pub use error::Error;
pub use message::Message;
pub use numeric::parse_digits_u64;
pub use spec::{CompiledSpec, ContentType, FieldDef, LenType};
