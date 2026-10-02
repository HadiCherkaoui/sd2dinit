// SPDX-FileCopyrightText: Hadi Cherkaoui <contact@hide.cherkaoui.ch>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("failed to read {path}")]
    IoError {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("unit file has no sections: {path}")]
    NoSections { path: PathBuf },
}

#[derive(Error, Debug)]
pub enum ConvertError {
    #[error("no ExecStart directive found in {unit}")]
    NoExecStart { unit: String },
    #[error("unsupported unit type: {unit_type}")]
    UnsupportedType { unit_type: String },
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("failed to read config at {path}")]
    IoError {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse config")]
    ParseError { source: toml::de::Error },
}
