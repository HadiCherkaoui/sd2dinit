use std::collections::HashMap;
use std::path::PathBuf;
use serde::Deserialize;
use crate::error::ConfigError;

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub output_dir: PathBuf,
    pub ignored_units: Vec<String>,
    pub dependency_map: HashMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        let mut dependency_map = HashMap::new();
        dependency_map.insert("network-online.target".into(), "network".into());
        dependency_map.insert("network.target".into(), "network".into());
        dependency_map.insert("multi-user.target".into(), "boot".into());
        dependency_map.insert("sysinit.target".into(), "boot".into());
        dependency_map.insert("default.target".into(), "boot".into());
        Self {
            output_dir: PathBuf::from("/etc/dinit.d"),
            ignored_units: Vec::new(),
            dependency_map,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        todo!()
    }
}
