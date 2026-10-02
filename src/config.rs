// SPDX-FileCopyrightText: Hadi Cherkaoui <contact@hide.cherkaoui.ch>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::error::ConfigError;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

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
    /// `/usr/lib/dinit.d/user/` is the standard location for
    /// distribution-provided user services; all users' dinit instances
    /// pick it up automatically alongside `~/.config/dinit.d/`.
    pub user_output_dir: PathBuf,

    pub ignored_units: Vec<String>,

    /// systemd unit name → dinit service name, taking precedence over lookup.
    ///
    /// Mapped names are emitted even when no such dinit service exists yet.
    pub dependency_map: HashMap<String, String>,

    /// Directories searched for the dinit services system units may depend on.
    pub service_dirs: Vec<PathBuf>,

    /// Directories searched for the dinit services user units may depend on.
    pub user_service_dirs: Vec<PathBuf>,
}

/// Where the system dinit instance looks for service files, per dinit(8).
const SYSTEM_SERVICE_DIRS: &[&str] = &[
    "/etc/dinit.d",
    "/run/dinit.d",
    "/usr/local/lib/dinit.d",
    "/lib/dinit.d",
];

/// The shared directories a user dinit instance searches, per dinit(8).
///
/// Per-user `~/.config/dinit.d` is left out: the pacman hook runs as root.
const USER_SERVICE_DIRS: &[&str] = &[
    "/etc/dinit.d/user",
    "/usr/lib/dinit.d/user",
    "/usr/local/lib/dinit.d/user",
];

impl Default for Config {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("/etc/dinit.d"),
            user_output_dir: PathBuf::from("/usr/lib/dinit.d/user"),
            ignored_units: Vec::new(),
            dependency_map: HashMap::new(),
            service_dirs: SYSTEM_SERVICE_DIRS.iter().map(PathBuf::from).collect(),
            user_service_dirs: USER_SERVICE_DIRS.iter().map(PathBuf::from).collect(),
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

        toml::from_str(&content).map_err(|e| ConfigError::ParseError { source: e })
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
