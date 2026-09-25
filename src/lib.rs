//! Fast, zero-copy, zero-allocation ISO 8583 parser and builder driven by a YAML spec.
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
//! | `bitmap`, `length` | internal: hex bitmap and length prefixes |
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
