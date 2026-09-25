//! **Legacy API.** String padding. No longer used inside the library; kept because it is public.

/// Prepend `pad` to `s` until it is `length` long (e.g. `left_pad("7", 3, "0")` → `"007"`).
///
/// If `s` is already `length` or longer, it is returned unchanged.
pub fn left_pad(s: &str, length: usize, pad: &str) -> String {
    if s.len() >= length {
        return s.to_string();
    }
    let padding = pad.repeat(length - s.len());
    format!("{}{}", padding, s)
}
