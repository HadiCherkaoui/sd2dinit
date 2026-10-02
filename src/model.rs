// SPDX-FileCopyrightText: Hadi Cherkaoui <contact@hide.cherkaoui.ch>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum DinitType {
    Process,
    BgProcess,
    Scripted,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RestartPolicy {
    Never,
    Always,
    OnFailure,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Severity {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct Warning {
    pub directive: String,
    pub message: String,
    pub severity: Severity,
}

#[derive(Debug, Clone)]
pub struct DinitService {
    pub name: String,
    pub source_path: PathBuf,
    pub service_type: DinitType,
    pub command: Option<String>,
    pub stop_command: Option<String>,
    /// Signal dinit stops the process with when no stop command is set; `None` is TERM.
    pub term_signal: Option<String>,
    /// dinit's `run-as` takes only a user; the group is that user's primary group.
    pub user: Option<String>,
    pub working_dir: Option<PathBuf>,
    pub env_files: Vec<PathBuf>,
    pub pid_file: Option<PathBuf>,
    pub restart: RestartPolicy,
    pub smooth_recovery: bool,
    pub restart_delay: Option<f64>,
    pub depends_on: Vec<String>,
    pub waits_for: Vec<String>,
    /// Ordering only: wait for these if they are starting, but never start them.
    pub after: Vec<String>,
    pub before: Vec<String>,
    pub logfile: Option<PathBuf>,
}

#[derive(Debug)]
pub struct ConversionResult {
    pub main_service: DinitService,
    pub pre_service: Option<DinitService>,
    pub post_service: Option<DinitService>,
    pub pre_script: Option<String>,
    pub post_script: Option<String>,
    /// Runs several oneshot `ExecStart=` lines, or one that may fail.
    pub start_script: Option<String>,
    pub stop_script: Option<String>,
    pub env_file_content: Option<String>,
    pub warnings: Vec<Warning>,
    pub should_enable: bool,
}
