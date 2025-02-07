use std::env::var;
use std::fmt::format;
use crate::iso8583::MtiType;

pub fn mti_validator(mti: &MtiType) -> Result<bool, Box<dyn std::error::Error>> {
    let mti_string = mti.mti.clone();
    let len = mti_string.len();
    if len != 4 {
        return Err(String::from("MTI must be length (4)").into());
    }

    match mti_string.parse::<i64>() {
        Ok(_) => Ok(true),
        Err(_) => Err(String::from("MTI can only contain integers").into()),
    }
}

pub fn fixed_length_integer_validator(field: usize, length: usize, data: &str) -> Result<bool, Box<dyn std::error::Error>> {
   let verify: bool = length == data.len();
    if !verify {
        return Err(String::from(format!("field {}: expected length {} found {} instead", field, length, data.len())).into());
    }
    
    Ok(true)
}

pub fn variable_length_integer_validator(field: usize, min: usize, max: usize, data: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let verify: bool = (data.len() >= min) && (data.len() <= max);
    if !verify {
        return Err(String::from(format!("field {}: expected max length {} and min length {} found {}", field, max, min, data.len() )).into());
    }
    Ok(true)
}