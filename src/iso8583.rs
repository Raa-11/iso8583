use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use crate::converter::{bitmap_array_to_hex, hex_to_bitmap_array};
use crate::specfile::{spec_from_file, Spec};
use crate::strpad::left_pad;
use crate::validators::mti_validator;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MtiType {
    pub mti: String,
}

impl MtiType {
    pub fn to_string(&self) -> &str {
        &self.mti
    }
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ElementsType {
    elements: HashMap<i64, String>,
}

impl ElementsType {
    pub fn get_elements(&self) -> &HashMap<i64, String> {
        &self.elements
    }
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IsoStruct {
    pub spec: Spec,
    pub mti: MtiType,
    pub bitmap: Vec<i64>,
    pub elements: ElementsType,
}

impl IsoStruct {
    pub fn to_string(&self) -> Result<String, Box<dyn std::error::Error>> {
        let mut str = String::new();

        let bitmap_string = bitmap_array_to_hex(&self.bitmap)?;
        let elements_str = self.pack_elements()?;
        str.push_str(self.mti.to_string());
        str.push_str(&bitmap_string);
        str.push_str(&elements_str);
        Ok(str)
    }

    pub fn add_mti(&mut self, data: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mti = MtiType{ mti: data.to_string() };
        let result = mti_validator(&mti);
        match result {
            Ok(_) => {
                self.mti = mti;
                Ok(())
            },
            Err(e) => {
                Err(e)
            }
        }
    }

    pub fn add_field(&mut self, field: i64, value: &str) -> Result<(), Box<dyn std::error::Error>> {
        let bitmap_len = self.bitmap.len() as i64;

        if field < 2 || field > bitmap_len {
            return  Err(String::from(format!(
                "expected field to be between {} and {} found {} instead",
                2,
                self.bitmap.len(),
                field
            )).into());
        }
        self.bitmap[(field - 1) as usize] = 1;
        self.elements.elements.insert(field, String::from(value));
        Ok(())
    }

    pub fn parse(&self, i: &str) -> Result<IsoStruct, Box<dyn std::error::Error>> {
        let mut q = IsoStruct{
            spec: Spec { fields: HashMap::new() },
            mti: MtiType { mti: String::new() },
            bitmap: Vec::new(),
            elements: ElementsType { elements: HashMap::new() },
        };
        
        let (mti, rest) = extract_mti(i);
        
        let result = extract_bitmap(rest);
        if result.is_err() {
            return Err(result.err().unwrap());
        }
        let (bitmap, element_string) = result.unwrap();
        
        let result2 = mti_validator(&mti);
        if result2.is_err() {
            return Err(result2.err().unwrap());
        }
        
        let result3 = unpack_elements(&bitmap, &element_string, &self.spec);
        if result3.is_err() {
            return Err(result3.err().unwrap());
        }
        let elements = result3?;
        
        q.spec = self.spec.clone();
        q.mti = mti;
        q.bitmap = bitmap;
        q.elements = elements;
        
        Ok(q)
    }

    pub fn pack_elements(&self) -> Result<String, Box<dyn std::error::Error>> {
        let mut str= String::new();
        let bitmap = &self.bitmap;
        let elements_map = self.elements.get_elements();
        let elements_spec = &self.spec;
        
        for index in 1..bitmap.len() {
            if bitmap[index] == 1 {
                let field = (index + 1) as i64;
                let field_description = &elements_spec.fields[&field];
                if field_description.len_type == "fixed" {
                    str.push_str(elements_map.get(&field).unwrap());
                } else {
                    let length_type = get_variable_length_from_string(&field_description.len_type);
                    if length_type.is_err() {
                        return Err(length_type.err().unwrap());
                    }
                    let actual_length = elements_map.get(&field).unwrap().len();
                    let padded_length = left_pad(&actual_length.to_string(), length_type?, "0");
                    str.push_str(&padded_length);
                    str.push_str(&elements_map.get(&field).unwrap());
                }
            }
        }
        
        Ok(str)
    }
}

fn extract_mti(s: &str) -> (MtiType, &str) {
    let mti = s[..4].to_string();
    let rest = &s[4..];

    (MtiType{ mti: mti.to_string() }, rest)
}

fn extract_bitmap(rest: &str) -> Result<(Vec<i64>, String), Box<dyn std::error::Error>> {
    let front_hex = &rest[0..2];
    let in_dec = u8::from_str_radix(front_hex, 16);
    if let Err(e) = in_dec {
        return Err(e.into())
    }

    let in_binary = format!("{:08b}", in_dec.unwrap());
    let compare = "1";
    let bitmap_hex_length: i64;

    if in_binary.chars().next().unwrap() == compare.chars().next().unwrap() {
        bitmap_hex_length = 32;
    } else {
        bitmap_hex_length = 16;
    }

    let bitmap_hex_string = &rest[0..bitmap_hex_length as usize];
    let element_string = rest[bitmap_hex_length as usize..].to_string();

    let result= hex_to_bitmap_array(bitmap_hex_string);
    match result {
        Ok(bitmap) => {
            Ok((bitmap, element_string))
        }
        Err(e) => {Err(e.into())}
    }
}

fn get_variable_length_from_string(str: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let num: i64;
    match str {
        "llvar" => Ok(2),
        "lllvar" => Ok(3),
        "llllvar" => Ok(4),
        _ => Err(String::from(format!("{} is an invalid LenType", str)).into()),
    }
}

fn extract_field_from_elements(spec: &Spec, field: i64, str :&str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let field_description = &spec.fields[&field];

    let (extracted_field, substr) = if field_description.len_type == "fixed" {
        let extracted_field = str[0..field_description.max_len].to_string();
        let substr = str[field_description.max_len..].to_string();
        (extracted_field, substr)
    } else {
        let length = get_variable_length_from_string(field_description.len_type.as_str())?;
        let field_length = &str[0..length];
        let temp_substr = &str[length..];
        let field_length_int: usize = field_length.parse()?;

        let extracted_field = temp_substr[0..field_length_int].to_string();
        let substr = temp_substr[field_length_int..].to_string();
        (extracted_field, substr)
    };

    Ok((extracted_field, substr))
}

fn unpack_elements(bitmap: &[i64], elements: &str, spec: &Spec) -> Result<ElementsType, Box<dyn std::error::Error>> {
    let mut m = HashMap::new();
    let mut current_string = elements.to_string();
    
    for index in 1..bitmap.len() {
        let bit = bitmap[index];
        if bit == 1 {
            let field = (index + 1) as i64;
            let result = extract_field_from_elements(spec, field, &current_string);
            if result.is_err() {
                return Err(result.err().unwrap());
            }
            let (extracted_field, substr) = result?;
            m.insert(field, extracted_field);
            current_string = substr;
        }
    }

    let elem = ElementsType{elements: m};
    Ok(elem)
}

pub fn new_iso_struct(filename: &str, secondary_bitmap: bool) -> Result<IsoStruct, Box<dyn std::error::Error>> {
    let mut bitmap : Vec<i64>;
    let mti = MtiType{ mti: String::new() };
    
    if secondary_bitmap {
        bitmap = vec![0; 128];
        bitmap[0] = 1;
    } else {
        bitmap = vec![0; 64];
    }
    
    let emap: HashMap<i64, String> = HashMap::new();
    let elements = ElementsType{elements: emap};
    let result = spec_from_file(filename);
    if result.is_err() {
        return Err(result.err().unwrap());
    }
    let spec = result.unwrap();
    let iso = IsoStruct { spec, mti, bitmap, elements };
    Ok(iso)
}








