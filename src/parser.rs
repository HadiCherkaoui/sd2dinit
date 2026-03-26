use std::collections::HashMap;
use std::path::PathBuf;
use crate::error::ParseError;

#[derive(Debug, Clone)]
pub struct SystemdUnit {
    pub sections: HashMap<String, Vec<(String, String)>>,
    pub source_path: PathBuf,
    pub drop_in_paths: Vec<PathBuf>,
    pub parse_warnings: Vec<String>,
}

impl SystemdUnit {
    pub fn parse(_input: &str, source_path: PathBuf) -> Result<Self, ParseError> {
        todo!()
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections.get(section).and_then(|pairs| {
            pairs.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
        })
    }

    pub fn get_all(&self, section: &str, key: &str) -> Vec<&str> {
        self.sections
            .get(section)
            .map(|pairs| {
                pairs.iter().filter(|(k, _)| k == key).map(|(_, v)| v.as_str()).collect()
            })
            .unwrap_or_default()
    }
}
