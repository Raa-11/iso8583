use std::collections::HashMap;
use std::io::Read;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FieldDescription {
    #[serde(rename = "ContentType")]
    pub content_type: String,
    #[serde(rename = "MaxLen")]
    pub max_len: usize,
    #[serde(rename = "MinLen")]
    pub min_len: usize,
    #[serde(rename = "LenType")]
    pub len_type: String,
    #[serde(rename = "Label")]
    pub label: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Spec {
    pub fields: HashMap<i64, FieldDescription>
}

impl Spec {
    fn read_from_file(&mut self, filename: &str) -> Result<(), Box<dyn std::error::Error>> {
        let mut f = std::fs::File::open(filename)?;
        let mut content = String::new();
        f.read_to_string(&mut content)?;
        self.fields = serde_yml::from_str(&content)?;
        Ok(())
    }
}

pub fn spec_from_file(filename: &str) -> Result<Spec, Box<dyn std::error::Error>> {
    let mut s = Spec {
        fields: HashMap::new(),
    };
    s.read_from_file(filename)?;
    Ok(s)
}

