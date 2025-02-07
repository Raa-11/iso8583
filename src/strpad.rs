pub fn left_pad(s: &str, length: usize, pad: &str) -> String {
    if s.len() >= length {
        return s.to_string();
    }

    let padding = pad.repeat(length - s.len());
    format!("{}{}", padding, s)
}