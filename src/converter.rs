use std::collections::HashMap;

pub fn bitmap_array_to_hex(arr: &[i64]) -> Result<String, Box<dyn std::error::Error>> {
    let length = arr.len();
    if length % 4 != 0 {
        return Err(String::from("invalid iso8583 bitmap array").into())
    }
    
    if length / 4 % 2 != 0 {
        return Err(String::from("invalid iso8583 bitmap array").into())
    }

    let mut hex_string = String::new();
    let mut buf = 0;
    let mut exp = 3i32;
    
    let mut m= HashMap::new();
    m.insert(0, "0");
    m.insert(1, "1");
    m.insert(2, "2");
    m.insert(3, "3");
    m.insert(4, "4");
    m.insert(5, "5");
    m.insert(6, "6");
    m.insert(7, "7");
    m.insert(8, "8");
    m.insert(9, "9");
    m.insert(10, "a");
    m.insert(11, "b");
    m.insert(12, "c");
    m.insert(13, "d");
    m.insert(14, "e");
    m.insert(15, "f");

    for &bit in arr.iter() {
        if bit == 0 {
            exp -= 1;
        } else {
            buf += 2f32.powf(exp as f32) as i32;
            exp -= 1;
        }

        if exp < 0 {
            exp = 3;
            if let Some(&hex_char) = m.get(&buf) {
                hex_string.push_str(hex_char);
            }
            buf = 0;
        }
    }

    Ok(hex_string)
}

pub fn hex_to_bitmap_array(hex_string: &str) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
    let mut bit_string = String::new();

    let result = const_hex::decode(hex_string);
    if result.is_err() {
        return Err(result.unwrap_err().into())
    }
    let bytes = result.unwrap();
    for byte in bytes {
        bit_string.push_str(&format!("{:08b}", byte));
    }
    
    let bit_array_strings: Vec<_> = bit_string.chars().collect();
    let mut bit_array= Vec::with_capacity(bit_array_strings.len());
    for bit in bit_array_strings {
        bit_array.push(bit.to_digit(10).unwrap() as i64);
    }
    
    Ok(bit_array)
}