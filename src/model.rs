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
    pub user: Option<String>,
    pub group: Option<String>,
    pub working_dir: Option<PathBuf>,
    pub env_files: Vec<PathBuf>,
    pub pid_file: Option<PathBuf>,
    pub restart: RestartPolicy,
    pub smooth_recovery: bool,
    pub restart_delay: Option<f64>,
    pub depends_on: Vec<String>,
    pub depends_ms: Vec<String>,
    pub waits_for: Vec<String>,
    pub logfile: Option<PathBuf>,
}

#[derive(Debug)]
pub struct ConversionResult {
    pub main_service: DinitService,
    pub pre_service: Option<DinitService>,
    pub post_service: Option<DinitService>,
    pub pre_script: Option<String>,
    pub post_script: Option<String>,
    pub stop_script: Option<String>,
    pub env_file_content: Option<String>,
    /// Shell wrapper script content, generated when `EnvironmentFile=` entries
    /// use shell quoting that dinit's env-file parser cannot handle. The script
    /// sources the env-files via sh and execs the real command, letting the
    /// shell do all quoting and variable expansion.
    pub env_wrapper_script: Option<String>,
    pub warnings: Vec<Warning>,
    pub should_enable: bool,
}
