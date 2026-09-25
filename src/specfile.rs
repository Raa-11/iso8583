//! YAML spec file format, exactly as written in the file.
//!
//! These types only read the file. For parsing and packing, convert to
//! [`CompiledSpec`](crate::CompiledSpec) first (once, at startup) so field lookup is an array
//! access instead of a `HashMap` lookup plus `String` comparison.
//!
//! The format is compatible with the original Go spec ([mofax/iso8583](https://github.com/mofax/iso8583)):
//!
//! ```yaml
//! 2:
//!   ContentType: n        # n | a | an | ans (anything else is not content-checked)
//!   MaxLen: 19
//!   MinLen: 1             # optional, default 0
//!   LenType: llvar        # fixed | llvar | lllvar | llllvar
//!   Label: Primary Account Number   # optional
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Definition of one field as written in the YAML file.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FieldDescription {
    /// Content kind: `n`, `a`, `an`, `ans`; other values (e.g. `b`, `z`) are not content-checked.
    #[serde(rename = "ContentType")]
    pub content_type: String,
    /// Maximum length; for `fixed` this is the exact length.
    #[serde(rename = "MaxLen")]
    pub max_len: usize,
    /// Minimum length for variable fields. Optional (default 0), same as the original Go spec.
    #[serde(rename = "MinLen", default)]
    pub min_len: usize,
    /// `fixed`, `llvar`, `lllvar`, or `llllvar`.
    #[serde(rename = "LenType")]
    pub len_type: String,
    /// Human-readable field name. Optional.
    #[serde(rename = "Label", default)]
    pub label: String,
}

/// Contents of a spec file: field number → definition.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Spec {
    /// Definition per field number. May contain 0 (MTI), 1 and 65 (bitmaps); those entries are
    /// ignored when compiling because the parser handles them directly.
    pub fields: HashMap<i64, FieldDescription>,
}

/// Read and deserialize a YAML spec file.
///
/// Only reads the format; content validation (sane lengths, known `LenType`) is done by
/// [`CompiledSpec::compile`](crate::CompiledSpec::compile).
///
/// # Errors
/// The file cannot be read or the YAML does not match the format.
/// # Examples
/// ```
/// use iso_8583_rs::specfile::spec_from_file;
///
/// let spec = spec_from_file("spec1987.yml")?;
/// let pan = &spec.fields[&2];
/// assert_eq!(pan.len_type, "llvar");
/// assert_eq!(pan.max_len, 19);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn spec_from_file(filename: &str) -> Result<Spec, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(filename)?;
    Ok(Spec {
        fields: serde_yml::from_str(&content)?,
    })
}
