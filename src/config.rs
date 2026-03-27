use std::collections::HashMap;
use std::path::PathBuf;
use serde::Deserialize;
use crate::error::ConfigError;

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    /// Output directory for converted system-scope services.
    ///
    /// Written by the root dinit instance. Corresponds to
    /// `usr/lib/systemd/system/` on the systemd side.
    pub output_dir: PathBuf,

    /// Output directory for converted user-scope services.
    ///
    /// Written when the source path is under `usr/lib/systemd/user/`.
    /// `/usr/share/dinit.d/` is the conventional location for
    /// distribution-provided user services that should be available to all
    /// users' dinit sessions.
    pub user_output_dir: PathBuf,

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
            user_output_dir: PathBuf::from("/usr/share/dinit.d"),
            ignored_units: Vec::new(),
            dependency_map,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = config_file_path();

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&config_path).map_err(|e| ConfigError::IoError {
            path: config_path.clone(),
            source: e,
        })?;

        let mut config: Config = toml::from_str(&content).map_err(|e| ConfigError::ParseError {
            source: e,
        })?;

        // Merge built-in defaults — user entries take precedence
        let defaults = Self::default();
        for (k, v) in defaults.dependency_map {
            config.dependency_map.entry(k).or_insert(v);
        }

        Ok(config)
    }
}

fn config_file_path() -> std::path::PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        std::path::PathBuf::from(xdg).join("sd2dinit/config.toml")
    } else if let Ok(home) = std::env::var("HOME") {
        std::path::PathBuf::from(home).join(".config/sd2dinit/config.toml")
    } else {
        std::path::PathBuf::from("/etc/sd2dinit/config.toml")
    }
}
