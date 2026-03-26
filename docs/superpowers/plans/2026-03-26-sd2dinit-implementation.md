# sd2dinit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust CLI tool that converts systemd .service unit files into dinit service files, runnable standalone or as a pacman hook.

**Architecture:** Two-phase pipeline (Parser → SystemdUnit → Converter → DinitService → Generator → output). Single crate with lib.rs (library) + main.rs (CLI). Hand-rolled INI parser, struct-based IR, simple text generator.

**Tech Stack:** Rust, clap (derive), serde + toml, colored, anyhow, thiserror

**Spec:** `docs/superpowers/specs/2026-03-26-sd2dinit-design.md`

---

## File Map

| File | Responsibility |
|------|---------------|
| `Cargo.toml` | Crate metadata + dependencies |
| `src/lib.rs` | Re-exports public API from submodules |
| `src/model.rs` | DinitService, DinitType, RestartPolicy, ConversionResult, Warning, Severity |
| `src/error.rs` | ParseError, ConvertError, ConfigError with thiserror |
| `src/parser.rs` | SystemdUnit struct, INI parsing, drop-in merging |
| `src/converter.rs` | SystemdUnit → ConversionResult (all mapping logic) |
| `src/generator.rs` | DinitService → String serialization |
| `src/config.rs` | Config struct, TOML loading, built-in defaults |
| `src/hook.rs` | Pacman hook stdin reading + batch conversion |
| `src/main.rs` | CLI (clap subcommands), colored output, orchestration |
| `tests/parser_tests.rs` | Parser unit tests |
| `tests/converter_tests.rs` | Converter unit tests |
| `tests/generator_tests.rs` | Generator unit tests |
| `tests/integration_tests.rs` | Full pipeline integration tests |
| `hooks/sd2dinit.hook` | Alpm hook file |

---

### Task 1: Project Scaffold

**Files:**
- Create: `Cargo.toml`, `src/lib.rs`, `src/main.rs`, `src/model.rs`, `src/error.rs`, `src/parser.rs`, `src/converter.rs`, `src/generator.rs`, `src/config.rs`, `src/hook.rs`

- [ ] **Step 1: Initialize Cargo project**

Run: `cargo init --name sd2dinit`

- [ ] **Step 2: Add dependencies**

Run:
```bash
cargo add clap --features derive
cargo add serde --features derive
cargo add toml
cargo add colored
cargo add anyhow
cargo add thiserror
```

- [ ] **Step 3: Create source file stubs**

Create `src/model.rs`:
```rust
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
    pub warnings: Vec<Warning>,
    pub should_enable: bool,
}
```

Create `src/error.rs`:
```rust
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("failed to read {path}: {source}")]
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
    #[error("failed to read config at {path}: {source}")]
    IoError {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse config: {source}")]
    ParseError { source: toml::de::Error },
}
```

Create `src/parser.rs`:
```rust
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
```

Create `src/converter.rs`:
```rust
use crate::model::ConversionResult;
use crate::parser::SystemdUnit;
use crate::config::Config;
use crate::error::ConvertError;

pub fn convert(unit: &SystemdUnit, config: &Config) -> Result<ConversionResult, ConvertError> {
    todo!()
}
```

Create `src/generator.rs`:
```rust
use crate::model::DinitService;

pub fn generate(service: &DinitService) -> String {
    todo!()
}
```

Create `src/config.rs`:
```rust
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
            output_dir: PathBuf::from("/etc/dinit.d/"),
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
```

Create `src/hook.rs`:
```rust
use crate::config::Config;

pub fn run_hook(_config: &Config) -> anyhow::Result<()> {
    todo!()
}
```

Create `src/lib.rs`:
```rust
pub mod model;
pub mod error;
pub mod parser;
pub mod converter;
pub mod generator;
pub mod config;
pub mod hook;
```

Create `src/main.rs`:
```rust
fn main() {
    println!("sd2dinit - systemd to dinit converter");
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build`
Expected: compiles with no errors (todo!() is fine, it's runtime not compile-time)

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: project scaffold with module stubs and dependencies"
```

---

### Task 2: Parser — Basic INI Parsing (TDD)

**Files:**
- Modify: `src/parser.rs`
- Create: `tests/parser_tests.rs`

- [ ] **Step 1: Write the failing test — basic section and key-value parsing**

Create `tests/parser_tests.rs`:
```rust
use sd2dinit::parser::SystemdUnit;
use std::path::PathBuf;

#[test]
fn test_parse_basic_unit() {
    let input = "\
[Unit]
Description=OpenSSH Daemon
After=network.target

[Service]
Type=simple
ExecStart=/usr/sbin/sshd -D

[Install]
WantedBy=multi-user.target
";
    let unit = SystemdUnit::parse(input, PathBuf::from("sshd.service")).unwrap();

    assert_eq!(unit.get("Unit", "Description"), Some("OpenSSH Daemon"));
    assert_eq!(unit.get("Unit", "After"), Some("network.target"));
    assert_eq!(unit.get("Service", "Type"), Some("simple"));
    assert_eq!(unit.get("Service", "ExecStart"), Some("/usr/sbin/sshd -D"));
    assert_eq!(unit.get("Install", "WantedBy"), Some("multi-user.target"));
}

#[test]
fn test_parse_comments_and_blank_lines() {
    let input = "\
# This is a comment
; This is also a comment
[Unit]

Description=Test Service
# Another comment

[Service]
ExecStart=/bin/true
";
    let unit = SystemdUnit::parse(input, PathBuf::from("test.service")).unwrap();
    assert_eq!(unit.get("Unit", "Description"), Some("Test Service"));
    assert_eq!(unit.get("Service", "ExecStart"), Some("/bin/true"));
}

#[test]
fn test_parse_whitespace_around_equals() {
    let input = "\
[Service]
Type = simple
ExecStart = /usr/bin/foo --bar
";
    let unit = SystemdUnit::parse(input, PathBuf::from("test.service")).unwrap();
    assert_eq!(unit.get("Service", "Type"), Some("simple"));
    assert_eq!(unit.get("Service", "ExecStart"), Some("/usr/bin/foo --bar"));
}

#[test]
fn test_parse_multi_value_keys() {
    let input = "\
[Service]
ExecStartPre=/usr/bin/first
ExecStartPre=/usr/bin/second
ExecStartPre=-/usr/bin/third
ExecStart=/usr/sbin/daemon
";
    let unit = SystemdUnit::parse(input, PathBuf::from("test.service")).unwrap();
    let pres = unit.get_all("Service", "ExecStartPre");
    assert_eq!(pres, vec!["/usr/bin/first", "/usr/bin/second", "-/usr/bin/third"]);
}

#[test]
fn test_parse_continuation_lines() {
    let input = "\
[Service]
ExecStart=/usr/bin/foo \
  --option1 \
  --option2
";
    let unit = SystemdUnit::parse(input, PathBuf::from("test.service")).unwrap();
    assert_eq!(
        unit.get("Service", "ExecStart"),
        Some("/usr/bin/foo --option1 --option2")
    );
}

#[test]
fn test_parse_empty_value_reset() {
    let input = "\
[Service]
ExecStartPre=/usr/bin/original
ExecStartPre=
ExecStartPre=/usr/bin/replacement
ExecStart=/bin/true
";
    let unit = SystemdUnit::parse(input, PathBuf::from("test.service")).unwrap();
    let pres = unit.get_all("Service", "ExecStartPre");
    assert_eq!(pres, vec!["/usr/bin/replacement"]);
}

#[test]
fn test_parse_malformed_line_produces_warning() {
    let input = "\
[Service]
this is not a valid line
ExecStart=/bin/true
";
    let unit = SystemdUnit::parse(input, PathBuf::from("test.service")).unwrap();
    assert_eq!(unit.get("Service", "ExecStart"), Some("/bin/true"));
    assert!(!unit.parse_warnings.is_empty());
    assert!(unit.parse_warnings[0].contains("not a valid"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test parser_tests`
Expected: FAIL — `todo!()` panics

- [ ] **Step 3: Implement the parser**

Replace the `parse` method in `src/parser.rs`:

```rust
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
    pub fn parse(input: &str, source_path: PathBuf) -> Result<Self, ParseError> {
        let mut sections: HashMap<String, Vec<(String, String)>> = HashMap::new();
        let mut current_section: Option<String> = None;
        let mut parse_warnings: Vec<String> = Vec::new();

        // Join continuation lines first
        let joined = Self::join_continuations(input);

        for (line_num, line) in joined.lines().enumerate() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            // Section header
            if line.starts_with('[') && line.ends_with(']') {
                let name = line[1..line.len() - 1].to_string();
                current_section = Some(name.clone());
                sections.entry(name).or_default();
                continue;
            }

            // Key=Value pair
            if let Some(ref section) = current_section {
                if let Some(eq_pos) = line.find('=') {
                    let key = line[..eq_pos].trim().to_string();
                    let value = line[eq_pos + 1..].trim().to_string();

                    let pairs = sections.get_mut(section).unwrap();

                    // Empty value = reset all prior entries for this key
                    if value.is_empty() {
                        pairs.retain(|(k, _)| k != &key);
                    } else {
                        pairs.push((key, value));
                    }
                } else {
                    parse_warnings.push(format!(
                        "line {}: not a valid key=value pair: {}",
                        line_num + 1,
                        line
                    ));
                }
            } else {
                parse_warnings.push(format!(
                    "line {}: directive outside of section: {}",
                    line_num + 1,
                    line
                ));
            }
        }

        if sections.is_empty() {
            return Err(ParseError::NoSections {
                path: source_path,
            });
        }

        Ok(SystemdUnit {
            sections,
            source_path,
            drop_in_paths: Vec::new(),
            parse_warnings,
        })
    }

    fn join_continuations(input: &str) -> String {
        let mut result = String::new();
        let mut continuation = String::new();

        for line in input.lines() {
            if line.ends_with('\\') {
                // Strip the trailing backslash and append
                continuation.push_str(line[..line.len() - 1].trim_end());
                continuation.push(' ');
            } else if !continuation.is_empty() {
                // End of continuation: append this line trimmed
                continuation.push_str(line.trim());
                result.push_str(&continuation);
                result.push('\n');
                continuation.clear();
            } else {
                result.push_str(line);
                result.push('\n');
            }
        }

        if !continuation.is_empty() {
            result.push_str(&continuation);
            result.push('\n');
        }

        result
    }

    /// Get the last value for a key in a section (systemd semantics: last wins).
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections.get(section).and_then(|pairs| {
            pairs
                .iter()
                .rev()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        })
    }

    /// Get all values for a key in a section (for multi-value keys like ExecStartPre).
    pub fn get_all(&self, section: &str, key: &str) -> Vec<&str> {
        self.sections
            .get(section)
            .map(|pairs| {
                pairs
                    .iter()
                    .filter(|(k, _)| k == key)
                    .map(|(_, v)| v.as_str())
                    .collect()
            })
            .unwrap_or_default()
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test parser_tests`
Expected: all 7 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs tests/parser_tests.rs
git commit -m "feat: implement systemd unit file parser with INI parsing

Handles sections, key-value pairs, multi-value keys, continuation
lines, empty-value reset, comments, and malformed line warnings."
```

---

### Task 3: Parser — Drop-in Directory Merging

**Files:**
- Modify: `src/parser.rs`
- Modify: `tests/parser_tests.rs`

- [ ] **Step 1: Write the failing test for drop-in merging**

Add to `tests/parser_tests.rs`:
```rust
#[test]
fn test_parse_with_drop_in_merge() {
    let base = "\
[Service]
Type=simple
ExecStart=/usr/sbin/sshd -D
ExecStartPre=/usr/bin/ssh-keygen
";
    let drop_in = "\
[Service]
ExecStartPre=
ExecStartPre=/usr/bin/custom-keygen
ExecStart=
ExecStart=/usr/sbin/sshd -D -o UsePAM=yes
";
    let mut unit = SystemdUnit::parse(base, PathBuf::from("sshd.service")).unwrap();
    unit.merge_drop_in(drop_in, PathBuf::from("sshd.service.d/override.conf"));

    assert_eq!(unit.get("Service", "Type"), Some("simple"));
    // ExecStartPre was reset then replaced
    assert_eq!(unit.get_all("Service", "ExecStartPre"), vec!["/usr/bin/custom-keygen"]);
    // ExecStart was reset then replaced
    assert_eq!(unit.get("Service", "ExecStart"), Some("/usr/sbin/sshd -D -o UsePAM=yes"));
    assert_eq!(unit.drop_in_paths.len(), 1);
}
```

- [ ] **Step 2: Run tests to verify it fails**

Run: `cargo test --test parser_tests test_parse_with_drop_in_merge`
Expected: FAIL — `merge_drop_in` method does not exist

- [ ] **Step 3: Implement merge_drop_in**

Add to `SystemdUnit` impl block in `src/parser.rs`:

```rust
    /// Merge a drop-in override into this unit.
    /// Drop-in semantics: entries are added to existing sections.
    /// Empty-value assignments reset prior entries for that key.
    pub fn merge_drop_in(&mut self, input: &str, drop_in_path: PathBuf) {
        let joined = Self::join_continuations(input);
        let mut current_section: Option<String> = None;

        for line in joined.lines() {
            let line = line.trim();

            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            if line.starts_with('[') && line.ends_with(']') {
                let name = line[1..line.len() - 1].to_string();
                current_section = Some(name.clone());
                self.sections.entry(name).or_default();
                continue;
            }

            if let Some(ref section) = current_section {
                if let Some(eq_pos) = line.find('=') {
                    let key = line[..eq_pos].trim().to_string();
                    let value = line[eq_pos + 1..].trim().to_string();
                    let pairs = self.sections.get_mut(section).unwrap();

                    if value.is_empty() {
                        pairs.retain(|(k, _)| k != &key);
                    } else {
                        pairs.push((key, value));
                    }
                }
            }
        }

        self.drop_in_paths.push(drop_in_path);
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test parser_tests`
Expected: all 8 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/parser.rs tests/parser_tests.rs
git commit -m "feat: add drop-in directory merging to parser

Supports systemd .d/ override semantics: entries add to sections,
empty-value assignments reset prior entries for that key."
```

---

### Task 4: Parser — Real-World Unit Tests

**Files:**
- Modify: `tests/parser_tests.rs`

- [ ] **Step 1: Add real-world sshd test**

Add to `tests/parser_tests.rs`:
```rust
#[test]
fn test_parse_real_sshd() {
    let input = "\
[Unit]
Description=OpenSSH Daemon
Wants=sshdgenkeys.service
After=sshdgenkeys.service
After=network.target

[Service]
ExecStart=/usr/bin/sshd -D
ExecReload=/bin/kill -HUP $MAINPID
KillMode=process
Restart=always

[Install]
WantedBy=multi-user.target
";
    let unit = SystemdUnit::parse(input, PathBuf::from("sshd.service")).unwrap();

    assert_eq!(unit.get("Unit", "Description"), Some("OpenSSH Daemon"));
    assert_eq!(unit.get("Unit", "Wants"), Some("sshdgenkeys.service"));
    // After= has two values — get returns the last one
    assert_eq!(unit.get("Unit", "After"), Some("network.target"));
    // get_all returns both
    let afters = unit.get_all("Unit", "After");
    assert_eq!(afters, vec!["sshdgenkeys.service", "network.target"]);
    assert_eq!(unit.get("Service", "Restart"), Some("always"));
    assert_eq!(unit.get("Install", "WantedBy"), Some("multi-user.target"));
}

#[test]
fn test_parse_real_nginx() {
    let input = "\
[Unit]
Description=A high performance web server and a reverse proxy server
After=network-online.target remote-fs.target nss-lookup.target
Wants=network-online.target

[Service]
Type=forking
PIDFile=/run/nginx.pid
ExecStartPre=/usr/bin/nginx -t -q -g 'daemon on; master_process on;'
ExecStart=/usr/bin/nginx -g 'daemon on; master_process on;'
ExecReload=/usr/bin/nginx -s reload
ExecStop=/bin/kill -s QUIT $MAINPID
PrivateTmp=true

[Install]
WantedBy=multi-user.target
";
    let unit = SystemdUnit::parse(input, PathBuf::from("nginx.service")).unwrap();

    assert_eq!(unit.get("Service", "Type"), Some("forking"));
    assert_eq!(unit.get("Service", "PIDFile"), Some("/run/nginx.pid"));
    assert_eq!(
        unit.get("Service", "ExecStartPre"),
        Some("/usr/bin/nginx -t -q -g 'daemon on; master_process on;'")
    );
    assert_eq!(unit.get("Service", "PrivateTmp"), Some("true"));
}

#[test]
fn test_parse_real_docker() {
    let input = "\
[Unit]
Description=Docker Application Container Engine
Documentation=https://docs.docker.com
After=network-online.target docker.socket firewalld.service containerd.service time-set.target
Wants=network-online.target containerd.service
Requires=docker.socket

[Service]
Type=notify
ExecStart=/usr/bin/dockerd -H fd://
ExecReload=/bin/kill -s HUP $MAINPID
TimeoutSec=0
RestartSec=2
Restart=always
StartLimitBurst=3
StartLimitInterval=60s
LimitNOFILE=infinity
LimitNPROC=infinity
LimitCORE=infinity
TasksMax=infinity
Delegate=yes
KillMode=process
OOMScoreAdjust=-500

[Install]
WantedBy=multi-user.target
";
    let unit = SystemdUnit::parse(input, PathBuf::from("docker.service")).unwrap();

    assert_eq!(unit.get("Service", "Type"), Some("notify"));
    assert_eq!(unit.get("Service", "RestartSec"), Some("2"));
    assert_eq!(unit.get("Service", "Restart"), Some("always"));
    assert_eq!(unit.get("Unit", "Requires"), Some("docker.socket"));
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --test parser_tests`
Expected: all 11 tests PASS (these test the existing parser, no new code needed)

- [ ] **Step 3: Commit**

```bash
git add tests/parser_tests.rs
git commit -m "test: add real-world unit file parser tests (sshd, nginx, docker)"
```

---

### Task 5: Generator (TDD)

**Files:**
- Modify: `src/generator.rs`
- Create: `tests/generator_tests.rs`

- [ ] **Step 1: Write the failing tests**

Create `tests/generator_tests.rs`:
```rust
use sd2dinit::model::{DinitService, DinitType, RestartPolicy};
use sd2dinit::generator::generate;
use std::path::PathBuf;

fn minimal_service() -> DinitService {
    DinitService {
        name: "test".into(),
        source_path: PathBuf::from("/usr/lib/systemd/system/test.service"),
        service_type: DinitType::Process,
        command: Some("/usr/bin/test-daemon".into()),
        stop_command: None,
        user: None,
        group: None,
        working_dir: None,
        env_files: Vec::new(),
        pid_file: None,
        restart: RestartPolicy::Never,
        smooth_recovery: false,
        restart_delay: None,
        depends_on: Vec::new(),
        depends_ms: Vec::new(),
        waits_for: Vec::new(),
        logfile: None,
    }
}

#[test]
fn test_generate_minimal_service() {
    let svc = minimal_service();
    let output = generate(&svc);

    assert!(output.starts_with("# Generated by sd2dinit from /usr/lib/systemd/system/test.service\n"));
    assert!(output.contains("type = process\n"));
    assert!(output.contains("command = /usr/bin/test-daemon\n"));
    // Should NOT contain keys for None fields
    assert!(!output.contains("stop-command"));
    assert!(!output.contains("run-as"));
    assert!(!output.contains("working-dir"));
    assert!(!output.contains("env-file"));
    assert!(!output.contains("pid-file"));
    assert!(!output.contains("restart-delay"));
    assert!(!output.contains("logfile"));
}

#[test]
fn test_generate_restart_false_omitted() {
    let svc = minimal_service();
    let output = generate(&svc);
    // restart = false is the default in dinit, so we don't emit it
    assert!(!output.contains("restart"));
    assert!(!output.contains("smooth-recovery"));
}

#[test]
fn test_generate_restart_always() {
    let mut svc = minimal_service();
    svc.restart = RestartPolicy::Always;
    svc.smooth_recovery = true;
    svc.restart_delay = Some(5.0);
    let output = generate(&svc);

    assert!(output.contains("restart = true\n"));
    assert!(output.contains("smooth-recovery = true\n"));
    assert!(output.contains("restart-delay = 5\n"));
}

#[test]
fn test_generate_restart_on_failure() {
    let mut svc = minimal_service();
    svc.restart = RestartPolicy::OnFailure;
    svc.smooth_recovery = true;
    let output = generate(&svc);
    assert!(output.contains("restart = on-failure\n"));
    assert!(output.contains("smooth-recovery = true\n"));
}

#[test]
fn test_generate_run_as_user_and_group() {
    let mut svc = minimal_service();
    svc.user = Some("www-data".into());
    svc.group = Some("www-data".into());
    let output = generate(&svc);
    assert!(output.contains("run-as = www-data:www-data\n"));
}

#[test]
fn test_generate_run_as_user_only() {
    let mut svc = minimal_service();
    svc.user = Some("nobody".into());
    let output = generate(&svc);
    assert!(output.contains("run-as = nobody\n"));
}

#[test]
fn test_generate_run_as_group_only() {
    let mut svc = minimal_service();
    svc.group = Some("daemon".into());
    let output = generate(&svc);
    assert!(output.contains("run-as = :daemon\n"));
}

#[test]
fn test_generate_bgprocess_with_pid_file() {
    let mut svc = minimal_service();
    svc.service_type = DinitType::BgProcess;
    svc.pid_file = Some(PathBuf::from("/run/nginx.pid"));
    let output = generate(&svc);
    assert!(output.contains("type = bgprocess\n"));
    assert!(output.contains("pid-file = /run/nginx.pid\n"));
}

#[test]
fn test_generate_dependencies() {
    let mut svc = minimal_service();
    svc.depends_on = vec!["network".into(), "sshd-pre".into()];
    svc.depends_ms = vec!["dbus".into()];
    svc.waits_for = vec!["local-fs".into()];
    let output = generate(&svc);
    assert!(output.contains("depends-on = network\n"));
    assert!(output.contains("depends-on = sshd-pre\n"));
    assert!(output.contains("depends-ms = dbus\n"));
    assert!(output.contains("waits-for = local-fs\n"));
}

#[test]
fn test_generate_multiple_env_files() {
    let mut svc = minimal_service();
    svc.env_files = vec![
        PathBuf::from("/etc/dinit.d/sshd.env"),
        PathBuf::from("/etc/default/sshd"),
    ];
    let output = generate(&svc);
    assert!(output.contains("env-file = /etc/dinit.d/sshd.env\n"));
    assert!(output.contains("env-file = /etc/default/sshd\n"));
}

#[test]
fn test_generate_full_service() {
    let svc = DinitService {
        name: "sshd".into(),
        source_path: PathBuf::from("/usr/lib/systemd/system/sshd.service"),
        service_type: DinitType::Process,
        command: Some("/usr/bin/sshd -D".into()),
        stop_command: Some("/bin/kill -QUIT $PID".into()),
        user: Some("root".into()),
        group: None,
        working_dir: Some(PathBuf::from("/var/run/sshd")),
        env_files: vec![PathBuf::from("/etc/dinit.d/sshd.env")],
        pid_file: None,
        restart: RestartPolicy::OnFailure,
        smooth_recovery: true,
        restart_delay: Some(2.5),
        depends_on: vec!["network".into()],
        depends_ms: Vec::new(),
        waits_for: vec!["sshd-pre".into()],
        logfile: Some(PathBuf::from("/var/log/sshd.log")),
    };
    let output = generate(&svc);

    let expected = "\
# Generated by sd2dinit from /usr/lib/systemd/system/sshd.service
type = process
command = /usr/bin/sshd -D
stop-command = /bin/kill -QUIT $PID
run-as = root
working-dir = /var/run/sshd
logfile = /var/log/sshd.log
env-file = /etc/dinit.d/sshd.env
restart = on-failure
smooth-recovery = true
restart-delay = 2.5
depends-on = network
waits-for = sshd-pre
";
    assert_eq!(output, expected);
}

#[test]
fn test_generate_restart_delay_whole_number() {
    let mut svc = minimal_service();
    svc.restart = RestartPolicy::Always;
    svc.restart_delay = Some(5.0);
    let output = generate(&svc);
    // 5.0 should render as "5", not "5.0"
    assert!(output.contains("restart-delay = 5\n"));
}

#[test]
fn test_generate_restart_delay_fractional() {
    let mut svc = minimal_service();
    svc.restart = RestartPolicy::Always;
    svc.restart_delay = Some(2.5);
    let output = generate(&svc);
    assert!(output.contains("restart-delay = 2.5\n"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test generator_tests`
Expected: FAIL — `todo!()` panics

- [ ] **Step 3: Implement the generator**

Replace `src/generator.rs`:

```rust
use crate::model::{DinitService, DinitType, RestartPolicy};

pub fn generate(service: &DinitService) -> String {
    let mut out = String::new();

    // Provenance comment
    out.push_str(&format!(
        "# Generated by sd2dinit from {}\n",
        service.source_path.display()
    ));

    // Type
    let type_str = match service.service_type {
        DinitType::Process => "process",
        DinitType::BgProcess => "bgprocess",
        DinitType::Scripted => "scripted",
    };
    out.push_str(&format!("type = {}\n", type_str));

    // Command
    if let Some(ref cmd) = service.command {
        out.push_str(&format!("command = {}\n", cmd));
    }

    // Stop command
    if let Some(ref cmd) = service.stop_command {
        out.push_str(&format!("stop-command = {}\n", cmd));
    }

    // Run-as
    match (&service.user, &service.group) {
        (Some(u), Some(g)) => out.push_str(&format!("run-as = {}:{}\n", u, g)),
        (Some(u), None) => out.push_str(&format!("run-as = {}\n", u)),
        (None, Some(g)) => out.push_str(&format!("run-as = :{}\n", g)),
        (None, None) => {}
    }

    // Working directory
    if let Some(ref dir) = service.working_dir {
        out.push_str(&format!("working-dir = {}\n", dir.display()));
    }

    // Logfile
    if let Some(ref path) = service.logfile {
        out.push_str(&format!("logfile = {}\n", path.display()));
    }

    // Pid file
    if let Some(ref path) = service.pid_file {
        out.push_str(&format!("pid-file = {}\n", path.display()));
    }

    // Env files
    for path in &service.env_files {
        out.push_str(&format!("env-file = {}\n", path.display()));
    }

    // Restart
    match service.restart {
        RestartPolicy::Never => {} // default, omit
        RestartPolicy::Always => out.push_str("restart = true\n"),
        RestartPolicy::OnFailure => out.push_str("restart = on-failure\n"),
    }

    // Smooth recovery
    if service.smooth_recovery {
        out.push_str("smooth-recovery = true\n");
    }

    // Restart delay
    if let Some(delay) = service.restart_delay {
        if delay == delay.floor() {
            out.push_str(&format!("restart-delay = {}\n", delay as i64));
        } else {
            out.push_str(&format!("restart-delay = {}\n", delay));
        }
    }

    // Dependencies
    for dep in &service.depends_on {
        out.push_str(&format!("depends-on = {}\n", dep));
    }
    for dep in &service.depends_ms {
        out.push_str(&format!("depends-ms = {}\n", dep));
    }
    for dep in &service.waits_for {
        out.push_str(&format!("waits-for = {}\n", dep));
    }

    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test generator_tests`
Expected: all 13 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/generator.rs tests/generator_tests.rs
git commit -m "feat: implement dinit service file generator

Serializes DinitService structs to dinit-format text output with
provenance comments, run-as user:group joining, and smart formatting."
```

---

### Task 6: Converter — Type Mapping and Basic Fields (TDD)

**Files:**
- Modify: `src/converter.rs`
- Create: `tests/converter_tests.rs`

- [ ] **Step 1: Write the failing tests for type mapping**

Create `tests/converter_tests.rs`:
```rust
use sd2dinit::converter::convert;
use sd2dinit::config::Config;
use sd2dinit::model::{DinitType, RestartPolicy, Severity};
use sd2dinit::parser::SystemdUnit;
use std::path::PathBuf;

fn parse(input: &str) -> SystemdUnit {
    SystemdUnit::parse(input, PathBuf::from("/usr/lib/systemd/system/test.service")).unwrap()
}

fn default_config() -> Config {
    Config::default()
}

#[test]
fn test_convert_simple_type() {
    let unit = parse("\
[Service]
Type=simple
ExecStart=/usr/bin/daemon
");
    let result = convert(&unit, &default_config()).unwrap();
    assert_eq!(result.main_service.service_type, DinitType::Process);
    assert_eq!(result.main_service.command, Some("/usr/bin/daemon".into()));
}

#[test]
fn test_convert_default_type_is_simple() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon
");
    let result = convert(&unit, &default_config()).unwrap();
    assert_eq!(result.main_service.service_type, DinitType::Process);
}

#[test]
fn test_convert_forking_with_pidfile() {
    let unit = parse("\
[Service]
Type=forking
PIDFile=/run/nginx.pid
ExecStart=/usr/bin/nginx
");
    let result = convert(&unit, &default_config()).unwrap();
    assert_eq!(result.main_service.service_type, DinitType::BgProcess);
    assert_eq!(result.main_service.pid_file, Some(PathBuf::from("/run/nginx.pid")));
}

#[test]
fn test_convert_forking_without_pidfile_falls_back() {
    let unit = parse("\
[Service]
Type=forking
ExecStart=/usr/bin/daemon
");
    let result = convert(&unit, &default_config()).unwrap();
    assert_eq!(result.main_service.service_type, DinitType::Process);
    // Should have a warning about missing PIDFile
    assert!(result.warnings.iter().any(|w| w.directive == "Type" && w.message.contains("PIDFile")));
}

#[test]
fn test_convert_oneshot() {
    let unit = parse("\
[Service]
Type=oneshot
ExecStart=/usr/bin/setup
");
    let result = convert(&unit, &default_config()).unwrap();
    assert_eq!(result.main_service.service_type, DinitType::Scripted);
}

#[test]
fn test_convert_dbus_fallback() {
    let unit = parse("\
[Service]
Type=dbus
BusName=org.freedesktop.NetworkManager
ExecStart=/usr/bin/NetworkManager
");
    let result = convert(&unit, &default_config()).unwrap();
    assert_eq!(result.main_service.service_type, DinitType::Process);
    assert!(result.warnings.iter().any(|w| w.directive == "Type" && w.message.contains("dbus")));
}

#[test]
fn test_convert_notify_fallback() {
    let unit = parse("\
[Service]
Type=notify
ExecStart=/usr/bin/dockerd
");
    let result = convert(&unit, &default_config()).unwrap();
    assert_eq!(result.main_service.service_type, DinitType::Process);
    assert!(result.warnings.iter().any(|w| w.directive == "Type" && w.message.contains("notify")));
}

#[test]
fn test_convert_user_and_group() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon
User=www-data
Group=www-data
");
    let result = convert(&unit, &default_config()).unwrap();
    assert_eq!(result.main_service.user, Some("www-data".into()));
    assert_eq!(result.main_service.group, Some("www-data".into()));
}

#[test]
fn test_convert_working_directory() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon
WorkingDirectory=/var/lib/app
");
    let result = convert(&unit, &default_config()).unwrap();
    assert_eq!(result.main_service.working_dir, Some(PathBuf::from("/var/lib/app")));
}

#[test]
fn test_convert_no_execstart_errors() {
    let unit = parse("\
[Service]
Type=simple
");
    let result = convert(&unit, &default_config());
    assert!(result.is_err());
}

#[test]
fn test_convert_service_name_from_path() {
    let unit = SystemdUnit::parse(
        "[Service]\nExecStart=/bin/true\n",
        PathBuf::from("/usr/lib/systemd/system/my-app.service"),
    ).unwrap();
    let result = convert(&unit, &default_config()).unwrap();
    assert_eq!(result.main_service.name, "my-app");
}

#[test]
fn test_convert_pidfile() {
    let unit = parse("\
[Service]
Type=forking
PIDFile=/run/sshd.pid
ExecStart=/usr/sbin/sshd
");
    let result = convert(&unit, &default_config()).unwrap();
    assert_eq!(result.main_service.pid_file, Some(PathBuf::from("/run/sshd.pid")));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test converter_tests`
Expected: FAIL — `todo!()` panics

- [ ] **Step 3: Implement the converter — type mapping and basic fields**

Replace `src/converter.rs`:

```rust
use std::path::PathBuf;

use crate::config::Config;
use crate::error::ConvertError;
use crate::model::{ConversionResult, DinitService, DinitType, RestartPolicy, Severity, Warning};
use crate::parser::SystemdUnit;

pub fn convert(unit: &SystemdUnit, config: &Config) -> Result<ConversionResult, ConvertError> {
    let mut warnings: Vec<Warning> = Vec::new();

    // Derive service name from file path
    let name = unit
        .source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // ExecStart is required
    let exec_start = unit.get("Service", "ExecStart").ok_or_else(|| {
        ConvertError::NoExecStart {
            unit: name.clone(),
        }
    })?;

    // Type mapping
    let raw_type = unit.get("Service", "Type").unwrap_or("simple");
    let has_pid_file = unit.get("Service", "PIDFile").is_some();

    let service_type = match raw_type {
        "simple" => DinitType::Process,
        "forking" if has_pid_file => DinitType::BgProcess,
        "forking" => {
            warnings.push(Warning {
                directive: "Type".into(),
                message: "forking service has no PIDFile — falling back to process type".into(),
                severity: Severity::Warn,
            });
            DinitType::Process
        }
        "oneshot" => DinitType::Scripted,
        "dbus" => {
            warnings.push(Warning {
                directive: "Type".into(),
                message: "dbus activation not supported — falling back to process type".into(),
                severity: Severity::Warn,
            });
            DinitType::Process
        }
        "notify" => {
            warnings.push(Warning {
                directive: "Type".into(),
                message: "notify not supported — falling back to process type".into(),
                severity: Severity::Warn,
            });
            DinitType::Process
        }
        other => {
            warnings.push(Warning {
                directive: "Type".into(),
                message: format!("unknown type '{}' — falling back to process", other),
                severity: Severity::Warn,
            });
            DinitType::Process
        }
    };

    // Command — replace known specifiers
    let command = replace_specifiers(exec_start, &name, &mut warnings);

    // Stop command
    let stop_command = unit.get("Service", "ExecStop").map(|s| s.to_string());

    // User and Group
    let user = unit.get("Service", "User").map(|s| s.to_string());
    let group = unit.get("Service", "Group").map(|s| s.to_string());

    // Working directory
    let working_dir = unit
        .get("Service", "WorkingDirectory")
        .map(PathBuf::from);

    // PID file
    let pid_file = unit.get("Service", "PIDFile").map(PathBuf::from);

    // Restart mapping
    let (restart, smooth_recovery) = convert_restart(
        unit.get("Service", "Restart"),
        &mut warnings,
    );

    // Restart delay
    let restart_delay = unit.get("Service", "RestartSec").and_then(|s| {
        // Strip trailing 's' if present (e.g., "60s" -> "60")
        let s = s.trim_end_matches('s');
        s.parse::<f64>().ok()
    });

    // Environment
    let (env_files, env_file_content) = convert_environment(unit, config, &name);

    // Dependencies
    let (depends_on, depends_ms, waits_for) = convert_dependencies(unit, config, &mut warnings);

    // ExecStartPre / ExecStartPost
    let (pre_service, pre_script) =
        convert_exec_pre(unit, &name, &unit.source_path, &mut warnings);
    let (post_service, post_script) =
        convert_exec_post(unit, &name, &unit.source_path, &mut warnings);

    // ExecStopPost — fold into stop command wrapper if ExecStop exists
    let (final_stop_command, stop_script) =
        convert_stop_post(unit, stop_command, &name, &mut warnings);

    // WantedBy / RequiredBy in [Install]
    let should_enable = unit.get("Install", "WantedBy").is_some()
        || unit.get("Install", "RequiredBy").is_some();

    // Warn about out-of-scope directives
    warn_out_of_scope(unit, &mut warnings);

    // Add parser warnings
    for pw in &unit.parse_warnings {
        warnings.push(Warning {
            directive: "parse".into(),
            message: pw.clone(),
            severity: Severity::Warn,
        });
    }

    let mut main_depends_on = depends_on;

    // Wire up pre-service dependency
    if pre_service.is_some() {
        main_depends_on.push(format!("{}-pre", name));
    }

    let main_service = DinitService {
        name: name.clone(),
        source_path: unit.source_path.clone(),
        service_type,
        command: Some(command),
        stop_command: final_stop_command,
        user,
        group,
        working_dir,
        env_files,
        pid_file,
        restart,
        smooth_recovery,
        restart_delay,
        depends_on: main_depends_on,
        depends_ms,
        waits_for,
        logfile: None,
    };

    Ok(ConversionResult {
        main_service,
        pre_service,
        post_service,
        pre_script,
        post_script,
        stop_script,
        env_file_content,
        warnings,
        should_enable,
    })
}

fn replace_specifiers(input: &str, service_name: &str, warnings: &mut Vec<Warning>) -> String {
    let mut result = input.to_string();
    // Known specifiers
    if result.contains("%n") {
        result = result.replace("%n", &format!("{}.service", service_name));
    }
    if result.contains("%N") {
        result = result.replace("%N", service_name);
    }
    // Warn about unknown specifiers
    let unknown: Vec<String> = result
        .match_indices('%')
        .filter_map(|(i, _)| {
            result.chars().nth(i + 1).and_then(|c| {
                if c.is_alphabetic() && c != 'n' && c != 'N' {
                    Some(format!("%{}", c))
                } else {
                    None
                }
            })
        })
        .collect();
    for spec in &unknown {
        warnings.push(Warning {
            directive: "ExecStart".into(),
            message: format!("unknown specifier {} removed", spec),
            severity: Severity::Warn,
        });
    }
    // Remove unknown specifiers
    let mut cleaned = String::new();
    let mut chars = result.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            if let Some(&next) = chars.peek() {
                if next.is_alphabetic() && next != 'n' && next != 'N' {
                    chars.next(); // skip the specifier letter
                    continue;
                }
            }
        }
        cleaned.push(c);
    }
    cleaned
}

fn convert_restart(
    restart_value: Option<&str>,
    warnings: &mut Vec<Warning>,
) -> (RestartPolicy, bool) {
    match restart_value {
        None | Some("no") => (RestartPolicy::Never, false),
        Some("always") => (RestartPolicy::Always, true),
        Some("on-success") => {
            warnings.push(Warning {
                directive: "Restart".into(),
                message: "on-success maps to always-restart; dinit has no clean-exit-only restart mode".into(),
                severity: Severity::Warn,
            });
            (RestartPolicy::Always, true)
        }
        Some("on-failure") | Some("on-abnormal") | Some("on-abort") | Some("on-watchdog") => {
            (RestartPolicy::OnFailure, true)
        }
        Some(other) => {
            warnings.push(Warning {
                directive: "Restart".into(),
                message: format!("unknown restart value '{}' — defaulting to no restart", other),
                severity: Severity::Warn,
            });
            (RestartPolicy::Never, false)
        }
    }
}

fn convert_environment(
    unit: &SystemdUnit,
    config: &Config,
    service_name: &str,
) -> (Vec<PathBuf>, Option<String>) {
    let mut env_files = Vec::new();
    let mut env_content_lines: Vec<String> = Vec::new();

    // Collect Environment= directives
    for val in unit.get_all("Service", "Environment") {
        // systemd allows "KEY=VAL" or 'KEY=VAL' with quotes
        let val = val.trim_matches('"').trim_matches('\'');
        env_content_lines.push(val.to_string());
    }

    let env_file_content = if env_content_lines.is_empty() {
        None
    } else {
        let env_path = config.output_dir.join(format!("{}.env", service_name));
        env_files.push(env_path);
        Some(env_content_lines.join("\n") + "\n")
    };

    // Collect EnvironmentFile= directives (passthrough)
    for val in unit.get_all("Service", "EnvironmentFile") {
        // Strip leading '-' (systemd: ignore if missing)
        let path = val.strip_prefix('-').unwrap_or(val);
        env_files.push(PathBuf::from(path));
    }

    (env_files, env_file_content)
}

fn convert_dependencies(
    unit: &SystemdUnit,
    config: &Config,
    warnings: &mut Vec<Warning>,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut depends_on = Vec::new();
    let mut depends_ms = Vec::new();
    let mut waits_for = Vec::new();

    // Helper to resolve a systemd dependency name to a dinit service name
    let resolve = |dep: &str| -> String {
        let dep = dep.trim();
        if let Some(mapped) = config.dependency_map.get(dep) {
            return mapped.clone();
        }
        // Strip .service suffix
        dep.strip_suffix(".service").unwrap_or(dep).to_string()
    };

    // Requires= → depends-on
    for val in unit.get_all("Unit", "Requires") {
        for dep in val.split_whitespace() {
            depends_on.push(resolve(dep));
        }
    }

    // Wants= → depends-ms
    for val in unit.get_all("Unit", "Wants") {
        for dep in val.split_whitespace() {
            depends_ms.push(resolve(dep));
        }
    }

    // After= → waits-for
    for val in unit.get_all("Unit", "After") {
        for dep in val.split_whitespace() {
            waits_for.push(resolve(dep));
        }
    }

    // Before= → skip with note
    if !unit.get_all("Unit", "Before").is_empty() {
        warnings.push(Warning {
            directive: "Before".into(),
            message: "Before= skipped (no direct dinit equivalent)".into(),
            severity: Severity::Info,
        });
    }

    // Conflicts= → skip with warning
    if !unit.get_all("Unit", "Conflicts").is_empty() {
        warnings.push(Warning {
            directive: "Conflicts".into(),
            message: "Conflicts= has no direct dinit equivalent — skipped".into(),
            severity: Severity::Warn,
        });
    }

    (depends_on, depends_ms, waits_for)
}

fn convert_exec_pre(
    unit: &SystemdUnit,
    service_name: &str,
    source_path: &PathBuf,
    warnings: &mut Vec<Warning>,
) -> (Option<DinitService>, Option<String>) {
    let pre_cmds = unit.get_all("Service", "ExecStartPre");
    if pre_cmds.is_empty() {
        return (None, None);
    }

    let (command, script) = if pre_cmds.len() == 1 {
        let cmd = pre_cmds[0];
        let (is_dash, clean_cmd) = parse_dash_prefix(cmd);
        if is_dash {
            // Single command with - prefix: wrap with || true
            let script_content = format!("#!/bin/sh\nset -e\n{} || true\n", clean_cmd);
            let script_path = format!("{}-pre.sh", service_name);
            (script_path, Some(script_content))
        } else {
            // Single command without - prefix: direct command
            (clean_cmd.to_string(), None)
        }
    } else {
        // Multiple commands: generate wrapper script
        let mut script = String::from("#!/bin/sh\nset -e\n");
        for cmd in &pre_cmds {
            let (is_dash, clean_cmd) = parse_dash_prefix(cmd);
            if is_dash {
                script.push_str(&format!("{} || true\n", clean_cmd));
            } else {
                script.push_str(&format!("{}\n", clean_cmd));
            }
        }
        let script_path = format!("{}-pre.sh", service_name);
        (script_path, Some(script))
    };

    let pre_service = DinitService {
        name: format!("{}-pre", service_name),
        source_path: source_path.clone(),
        service_type: DinitType::Scripted,
        command: if script.is_some() {
            // Script mode: command points to the script file
            Some(format!("/bin/sh /etc/dinit.d/{}-pre.sh", service_name))
        } else {
            Some(command)
        },
        stop_command: None,
        user: unit.get("Service", "User").map(|s| s.to_string()),
        group: unit.get("Service", "Group").map(|s| s.to_string()),
        working_dir: unit.get("Service", "WorkingDirectory").map(PathBuf::from),
        env_files: Vec::new(),
        pid_file: None,
        restart: RestartPolicy::Never,
        smooth_recovery: false,
        restart_delay: None,
        depends_on: Vec::new(),
        depends_ms: Vec::new(),
        waits_for: Vec::new(),
        logfile: None,
    };

    (Some(pre_service), script)
}

fn convert_exec_post(
    unit: &SystemdUnit,
    service_name: &str,
    source_path: &PathBuf,
    _warnings: &mut Vec<Warning>,
) -> (Option<DinitService>, Option<String>) {
    let post_cmds = unit.get_all("Service", "ExecStartPost");
    if post_cmds.is_empty() {
        return (None, None);
    }

    let (command, script) = if post_cmds.len() == 1 {
        let cmd = post_cmds[0];
        let (is_dash, clean_cmd) = parse_dash_prefix(cmd);
        if is_dash {
            let script_content = format!("#!/bin/sh\nset -e\n{} || true\n", clean_cmd);
            let script_path = format!("{}-post.sh", service_name);
            (script_path, Some(script_content))
        } else {
            (clean_cmd.to_string(), None)
        }
    } else {
        let mut script = String::from("#!/bin/sh\nset -e\n");
        for cmd in &post_cmds {
            let (is_dash, clean_cmd) = parse_dash_prefix(cmd);
            if is_dash {
                script.push_str(&format!("{} || true\n", clean_cmd));
            } else {
                script.push_str(&format!("{}\n", clean_cmd));
            }
        }
        let script_path = format!("{}-post.sh", service_name);
        (script_path, Some(script))
    };

    let post_service = DinitService {
        name: format!("{}-post", service_name),
        source_path: source_path.clone(),
        service_type: DinitType::Scripted,
        command: if script.is_some() {
            Some(format!("/bin/sh /etc/dinit.d/{}-post.sh", service_name))
        } else {
            Some(command)
        },
        stop_command: None,
        user: unit.get("Service", "User").map(|s| s.to_string()),
        group: unit.get("Service", "Group").map(|s| s.to_string()),
        working_dir: unit.get("Service", "WorkingDirectory").map(PathBuf::from),
        env_files: Vec::new(),
        pid_file: None,
        restart: RestartPolicy::Never,
        smooth_recovery: false,
        restart_delay: None,
        depends_on: Vec::new(),
        depends_ms: Vec::new(),
        waits_for: vec![service_name.to_string()], // post waits-for main
        logfile: None,
    };

    (Some(post_service), script)
}

fn convert_stop_post(
    unit: &SystemdUnit,
    stop_command: Option<String>,
    service_name: &str,
    warnings: &mut Vec<Warning>,
) -> (Option<String>, Option<String>) {
    let stop_post_cmds = unit.get_all("Service", "ExecStopPost");
    if stop_post_cmds.is_empty() {
        return (stop_command, None);
    }

    match stop_command {
        Some(stop_cmd) => {
            // ExecStop + ExecStopPost: generate wrapper script
            let mut script = String::from("#!/bin/sh\nset -e\n");
            script.push_str(&format!("{}\n", stop_cmd));
            for cmd in &stop_post_cmds {
                let (is_dash, clean_cmd) = parse_dash_prefix(cmd);
                if is_dash {
                    script.push_str(&format!("{} || true\n", clean_cmd));
                } else {
                    script.push_str(&format!("{}\n", clean_cmd));
                }
            }
            let wrapper_cmd = format!("/bin/sh /etc/dinit.d/{}-stop.sh", service_name);
            (Some(wrapper_cmd), Some(script))
        }
        None => {
            // ExecStopPost alone: skip with warning
            warnings.push(Warning {
                directive: "ExecStopPost".into(),
                message: "ExecStopPost= without ExecStop= skipped — dinit handles stop signals natively".into(),
                severity: Severity::Warn,
            });
            (None, None)
        }
    }
}

fn warn_out_of_scope(unit: &SystemdUnit, warnings: &mut Vec<Warning>) {
    let sandboxing = [
        "ProtectSystem", "ProtectHome", "PrivateTmp", "PrivateDevices",
        "PrivateNetwork", "ProtectKernelTunables", "ProtectKernelModules",
        "ProtectControlGroups", "NoNewPrivileges", "ReadOnlyPaths",
        "ReadWritePaths", "InaccessiblePaths", "ProtectHostname",
        "LockPersonality", "MemoryDenyWriteExecute", "RestrictRealtime",
        "RestrictSUIDSGID", "RestrictNamespaces", "SystemCallFilter",
        "SystemCallArchitectures", "CapabilityBoundingSet", "AmbientCapabilities",
        "SecureBits",
    ];
    let cgroup = [
        "Slice", "CPUQuota", "MemoryMax", "MemoryHigh", "MemoryLow",
        "IOWeight", "IODeviceWeight", "TasksMax", "Delegate",
    ];
    let conditionals = [
        "ConditionPathExists", "ConditionPathIsDirectory",
        "ConditionFileNotEmpty", "ConditionDirectoryNotEmpty",
        "ConditionKernelCommandLine", "ConditionVirtualization",
        "ConditionArchitecture", "ConditionSecurity",
        "AssertPathExists",
    ];
    let socket = ["ListenStream", "ListenDatagram", "ListenSequentialPacket", "Accept"];

    if let Some(pairs) = unit.sections.get("Service") {
        for (key, _) in pairs {
            if sandboxing.contains(&key.as_str()) {
                warnings.push(Warning {
                    directive: key.clone(),
                    message: format!("{} (sandboxing) not supported — skipped", key),
                    severity: Severity::Info,
                });
            } else if cgroup.contains(&key.as_str()) {
                warnings.push(Warning {
                    directive: key.clone(),
                    message: format!("{} (cgroup) not supported — skipped", key),
                    severity: Severity::Info,
                });
            } else if conditionals.contains(&key.as_str()) {
                warnings.push(Warning {
                    directive: key.clone(),
                    message: format!("{} (conditional) not supported — skipped", key),
                    severity: Severity::Info,
                });
            } else if socket.contains(&key.as_str()) {
                warnings.push(Warning {
                    directive: key.clone(),
                    message: format!("{} (socket activation) not supported — skipped", key),
                    severity: Severity::Warn,
                });
            }
        }
    }
}

/// Parse the `-` prefix from a command. Returns (is_dash_prefixed, clean_command).
fn parse_dash_prefix(cmd: &str) -> (bool, &str) {
    let cmd = cmd.trim();
    if let Some(rest) = cmd.strip_prefix('-') {
        (true, rest.trim())
    } else {
        (false, cmd)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test converter_tests`
Expected: all 12 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/converter.rs tests/converter_tests.rs
git commit -m "feat: implement converter with type mapping, basic fields, and helpers

Maps systemd Type= to dinit types, handles User/Group, WorkingDirectory,
PIDFile, specifier replacement, and forking-without-pidfile fallback."
```

---

### Task 7: Converter — Restart, Dependencies, Environment, Pre/Post, Out-of-Scope Tests

**Files:**
- Modify: `tests/converter_tests.rs`

- [ ] **Step 1: Add restart mapping tests**

Add to `tests/converter_tests.rs`:
```rust
#[test]
fn test_convert_restart_always() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon
Restart=always
RestartSec=5
");
    let result = convert(&unit, &default_config()).unwrap();
    assert_eq!(result.main_service.restart, RestartPolicy::Always);
    assert!(result.main_service.smooth_recovery);
    assert_eq!(result.main_service.restart_delay, Some(5.0));
}

#[test]
fn test_convert_restart_on_failure() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon
Restart=on-failure
");
    let result = convert(&unit, &default_config()).unwrap();
    assert_eq!(result.main_service.restart, RestartPolicy::OnFailure);
    assert!(result.main_service.smooth_recovery);
}

#[test]
fn test_convert_restart_on_success_lossy_warning() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon
Restart=on-success
");
    let result = convert(&unit, &default_config()).unwrap();
    assert_eq!(result.main_service.restart, RestartPolicy::Always);
    assert!(result.warnings.iter().any(|w|
        w.directive == "Restart" && w.message.contains("on-success")
    ));
}

#[test]
fn test_convert_restart_no() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon
Restart=no
");
    let result = convert(&unit, &default_config()).unwrap();
    assert_eq!(result.main_service.restart, RestartPolicy::Never);
    assert!(!result.main_service.smooth_recovery);
}

#[test]
fn test_convert_restart_sec_with_suffix() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon
Restart=always
RestartSec=60s
");
    let result = convert(&unit, &default_config()).unwrap();
    assert_eq!(result.main_service.restart_delay, Some(60.0));
}
```

- [ ] **Step 2: Add dependency mapping tests**

```rust
#[test]
fn test_convert_requires_to_depends_on() {
    let unit = parse("\
[Unit]
Requires=docker.socket

[Service]
ExecStart=/usr/bin/dockerd
");
    let result = convert(&unit, &default_config()).unwrap();
    assert!(result.main_service.depends_on.contains(&"docker".to_string()));
}

#[test]
fn test_convert_wants_to_depends_ms() {
    let unit = parse("\
[Unit]
Wants=network-online.target

[Service]
ExecStart=/usr/bin/daemon
");
    let result = convert(&unit, &default_config()).unwrap();
    // network-online.target should be mapped to "network" by default config
    assert!(result.main_service.depends_ms.contains(&"network".to_string()));
}

#[test]
fn test_convert_after_to_waits_for() {
    let unit = parse("\
[Unit]
After=network.target syslog.service

[Service]
ExecStart=/usr/bin/daemon
");
    let result = convert(&unit, &default_config()).unwrap();
    assert!(result.main_service.waits_for.contains(&"network".to_string()));
    assert!(result.main_service.waits_for.contains(&"syslog".to_string()));
}

#[test]
fn test_convert_before_skipped_with_note() {
    let unit = parse("\
[Unit]
Before=multi-user.target

[Service]
ExecStart=/usr/bin/daemon
");
    let result = convert(&unit, &default_config()).unwrap();
    assert!(result.warnings.iter().any(|w| w.directive == "Before"));
}

#[test]
fn test_convert_conflicts_skipped_with_warning() {
    let unit = parse("\
[Unit]
Conflicts=iptables.service

[Service]
ExecStart=/usr/bin/daemon
");
    let result = convert(&unit, &default_config()).unwrap();
    assert!(result.warnings.iter().any(|w| w.directive == "Conflicts"));
}

#[test]
fn test_convert_dependency_map_custom() {
    let mut config = default_config();
    config.dependency_map.insert("custom.target".into(), "my-custom".into());
    let unit = parse("\
[Unit]
After=custom.target

[Service]
ExecStart=/usr/bin/daemon
");
    let result = convert(&unit, &config).unwrap();
    assert!(result.main_service.waits_for.contains(&"my-custom".to_string()));
}
```

- [ ] **Step 3: Add environment tests**

```rust
#[test]
fn test_convert_environment_generates_env_file() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon
Environment=FOO=bar
Environment=\"BAZ=qux\"
");
    let result = convert(&unit, &default_config()).unwrap();
    assert!(result.env_file_content.is_some());
    let content = result.env_file_content.unwrap();
    assert!(content.contains("FOO=bar"));
    assert!(content.contains("BAZ=qux"));
    assert!(result.main_service.env_files.iter().any(|p|
        p.to_str().unwrap().ends_with("test.env")
    ));
}

#[test]
fn test_convert_environment_file_passthrough() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon
EnvironmentFile=/etc/default/myapp
EnvironmentFile=-/etc/sysconfig/myapp
");
    let result = convert(&unit, &default_config()).unwrap();
    assert!(result.main_service.env_files.contains(&PathBuf::from("/etc/default/myapp")));
    assert!(result.main_service.env_files.contains(&PathBuf::from("/etc/sysconfig/myapp")));
}

#[test]
fn test_convert_both_environment_and_file() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon
Environment=KEY=val
EnvironmentFile=/etc/default/myapp
");
    let result = convert(&unit, &default_config()).unwrap();
    // Generated .env comes first, then passthrough
    assert_eq!(result.main_service.env_files.len(), 2);
    assert!(result.main_service.env_files[0].to_str().unwrap().ends_with("test.env"));
    assert_eq!(result.main_service.env_files[1], PathBuf::from("/etc/default/myapp"));
}
```

- [ ] **Step 4: Add exec pre/post tests**

```rust
#[test]
fn test_convert_single_exec_start_pre() {
    let unit = parse("\
[Service]
ExecStartPre=/usr/bin/setup
ExecStart=/usr/bin/daemon
");
    let result = convert(&unit, &default_config()).unwrap();
    assert!(result.pre_service.is_some());
    let pre = result.pre_service.unwrap();
    assert_eq!(pre.name, "test-pre");
    assert_eq!(pre.service_type, DinitType::Scripted);
    assert_eq!(pre.command, Some("/usr/bin/setup".into()));
    assert!(result.pre_script.is_none()); // no wrapper needed
    // Main service should depend on pre
    assert!(result.main_service.depends_on.contains(&"test-pre".to_string()));
}

#[test]
fn test_convert_single_exec_start_pre_with_dash() {
    let unit = parse("\
[Service]
ExecStartPre=-/usr/bin/optional-setup
ExecStart=/usr/bin/daemon
");
    let result = convert(&unit, &default_config()).unwrap();
    assert!(result.pre_service.is_some());
    assert!(result.pre_script.is_some());
    let script = result.pre_script.unwrap();
    assert!(script.contains("#!/bin/sh"));
    assert!(script.contains("set -e"));
    assert!(script.contains("/usr/bin/optional-setup || true"));
}

#[test]
fn test_convert_multiple_exec_start_pre_mixed() {
    let unit = parse("\
[Service]
ExecStartPre=/usr/bin/must-succeed
ExecStartPre=-/usr/bin/optional
ExecStartPre=/usr/bin/also-must-succeed
ExecStart=/usr/bin/daemon
");
    let result = convert(&unit, &default_config()).unwrap();
    assert!(result.pre_script.is_some());
    let script = result.pre_script.unwrap();
    assert!(script.contains("/usr/bin/must-succeed\n"));
    assert!(script.contains("/usr/bin/optional || true\n"));
    assert!(script.contains("/usr/bin/also-must-succeed\n"));
    // Main depends on pre
    assert!(result.main_service.depends_on.contains(&"test-pre".to_string()));
}

#[test]
fn test_convert_exec_start_post() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon
ExecStartPost=/usr/bin/notify-ready
");
    let result = convert(&unit, &default_config()).unwrap();
    assert!(result.post_service.is_some());
    let post = result.post_service.unwrap();
    assert_eq!(post.name, "test-post");
    // Post waits-for main
    assert!(post.waits_for.contains(&"test".to_string()));
}

#[test]
fn test_convert_exec_stop_plus_stop_post() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon
ExecStop=/usr/bin/graceful-stop
ExecStopPost=/usr/bin/cleanup
");
    let result = convert(&unit, &default_config()).unwrap();
    assert!(result.stop_script.is_some());
    let script = result.stop_script.unwrap();
    assert!(script.contains("/usr/bin/graceful-stop"));
    assert!(script.contains("/usr/bin/cleanup"));
    // stop_command should point to the wrapper
    assert!(result.main_service.stop_command.unwrap().contains("-stop.sh"));
}

#[test]
fn test_convert_exec_stop_post_alone_skipped() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon
ExecStopPost=/usr/bin/cleanup
");
    let result = convert(&unit, &default_config()).unwrap();
    assert!(result.stop_script.is_none());
    assert!(result.main_service.stop_command.is_none());
    assert!(result.warnings.iter().any(|w| w.directive == "ExecStopPost"));
}
```

- [ ] **Step 5: Add out-of-scope warning tests**

```rust
#[test]
fn test_convert_sandboxing_warnings() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon
ProtectSystem=strict
PrivateTmp=true
NoNewPrivileges=true
");
    let result = convert(&unit, &default_config()).unwrap();
    assert!(result.warnings.iter().any(|w| w.directive == "ProtectSystem"));
    assert!(result.warnings.iter().any(|w| w.directive == "PrivateTmp"));
    assert!(result.warnings.iter().any(|w| w.directive == "NoNewPrivileges"));
}

#[test]
fn test_convert_should_enable_from_wanted_by() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon

[Install]
WantedBy=multi-user.target
");
    let result = convert(&unit, &default_config()).unwrap();
    assert!(result.should_enable);
}

#[test]
fn test_convert_should_not_enable_without_install() {
    let unit = parse("\
[Service]
ExecStart=/usr/bin/daemon
");
    let result = convert(&unit, &default_config()).unwrap();
    assert!(!result.should_enable);
}
```

- [ ] **Step 6: Run all converter tests**

Run: `cargo test --test converter_tests`
Expected: all tests PASS

- [ ] **Step 7: Commit**

```bash
git add tests/converter_tests.rs
git commit -m "test: comprehensive converter tests for restart, deps, env, pre/post, warnings"
```

---

### Task 8: Config Loading

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Implement config loading**

Replace the `load` method in `src/config.rs`:

```rust
impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let config_path = dirs_config_path();

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

        // Merge built-in defaults with user config (user overrides take precedence)
        let defaults = Self::default();
        for (k, v) in defaults.dependency_map {
            config.dependency_map.entry(k).or_insert(v);
        }

        Ok(config)
    }
}

fn dirs_config_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("sd2dinit/config.toml")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config/sd2dinit/config.toml")
    } else {
        PathBuf::from("/etc/sd2dinit/config.toml")
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: compiles successfully

- [ ] **Step 3: Commit**

```bash
git add src/config.rs
git commit -m "feat: implement config loading from XDG config path

Loads ~/.config/sd2dinit/config.toml with built-in dependency_map
defaults that user config can override."
```

---

### Task 9: CLI — Clap Subcommands and Convert

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Implement CLI with convert subcommand**

Replace `src/main.rs`:

```rust
use std::fs;
use std::path::PathBuf;
use std::process;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::*;

use sd2dinit::config::Config;
use sd2dinit::converter;
use sd2dinit::generator;
use sd2dinit::model::Severity;
use sd2dinit::parser::SystemdUnit;

#[derive(Parser)]
#[command(name = "sd2dinit", about = "Convert systemd unit files to dinit service files")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Convert a systemd unit file to dinit format
    Convert {
        /// Path to the systemd .service unit file
        unit_file: PathBuf,
        /// Output directory for generated dinit files
        #[arg(long, default_value = None)]
        output_dir: Option<PathBuf>,
        /// Print generated files without writing
        #[arg(long)]
        dry_run: bool,
        /// Overwrite existing dinit files
        #[arg(long)]
        force: bool,
    },
    /// Convert and optionally enable/start the service
    Install {
        /// Path to the systemd .service unit file
        unit_file: PathBuf,
        /// Output directory for generated dinit files
        #[arg(long, default_value = None)]
        output_dir: Option<PathBuf>,
        /// Enable the service via dinitctl
        #[arg(long)]
        enable: bool,
        /// Start the service via dinitctl
        #[arg(long)]
        start: bool,
        /// Print generated files without writing
        #[arg(long)]
        dry_run: bool,
        /// Overwrite existing dinit files
        #[arg(long)]
        force: bool,
    },
    /// Pacman hook mode — reads targets from stdin and batch converts
    Hook,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Convert {
            unit_file,
            output_dir,
            dry_run,
            force,
        } => run_convert(&unit_file, output_dir.as_deref(), dry_run, force),
        Commands::Install {
            unit_file,
            output_dir,
            enable,
            start,
            dry_run,
            force,
        } => run_install(&unit_file, output_dir.as_deref(), enable, start, dry_run, force),
        Commands::Hook => run_hook(),
    };

    match result {
        Ok(exit_code) => process::exit(exit_code),
        Err(e) => {
            eprintln!("{} {:#}", "error:".red().bold(), e);
            process::exit(2);
        }
    }
}

fn run_convert(
    unit_file: &PathBuf,
    output_dir: Option<&std::path::Path>,
    dry_run: bool,
    force: bool,
) -> Result<i32> {
    let mut config = Config::load().context("failed to load config")?;
    // Override config output_dir if CLI flag provided (so converter uses correct paths for scripts)
    if let Some(ref dir) = output_dir {
        config.output_dir = dir.clone();
    }
    let out_dir = config.output_dir.clone();

    // Check for non-.service files
    if let Some(ext) = unit_file.extension() {
        let ext = ext.to_str().unwrap_or("");
        if ext != "service" {
            eprintln!(
                "{} only .service units are supported, got .{} — skipping",
                "warning:".yellow().bold(),
                ext
            );
            return Ok(1);
        }
    }

    // Check for template units
    if let Some(stem) = unit_file.file_stem().and_then(|s| s.to_str()) {
        if stem.contains('@') {
            eprintln!(
                "{} template/instance units not supported — skipping {}",
                "warning:".yellow().bold(),
                unit_file.display()
            );
            return Ok(1);
        }
    }

    let content = fs::read_to_string(unit_file)
        .with_context(|| format!("failed to read {}", unit_file.display()))?;

    let mut unit = SystemdUnit::parse(&content, unit_file.clone())
        .with_context(|| format!("failed to parse {}", unit_file.display()))?;

    // Apply drop-in overrides
    let drop_in_dir = unit_file.with_extension("service.d");
    if drop_in_dir.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(&drop_in_dir)
            .with_context(|| format!("failed to read drop-in dir {}", drop_in_dir.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "conf").unwrap_or(false))
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let drop_content = fs::read_to_string(entry.path())?;
            unit.merge_drop_in(&drop_content, entry.path());
        }
    }

    let result = converter::convert(&unit, &config)
        .with_context(|| format!("failed to convert {}", unit_file.display()))?;

    let mut had_warnings = false;

    // Print warnings
    for w in &result.warnings {
        had_warnings = true;
        let prefix = match w.severity {
            Severity::Info => "info:".blue().bold(),
            Severity::Warn => "warning:".yellow().bold(),
            Severity::Error => "error:".red().bold(),
        };
        eprintln!("{} [{}] {}", prefix, w.directive, w.message);
    }

    // Generate main service
    let main_output = generator::generate(&result.main_service);
    write_or_print(
        &out_dir.join(&result.main_service.name),
        &main_output,
        dry_run,
        force,
        &result.main_service.name,
    )?;

    // Generate pre-service
    if let Some(ref pre) = result.pre_service {
        let pre_output = generator::generate(pre);
        write_or_print(&out_dir.join(&pre.name), &pre_output, dry_run, force, &pre.name)?;
    }

    // Generate post-service
    if let Some(ref post) = result.post_service {
        let post_output = generator::generate(post);
        write_or_print(&out_dir.join(&post.name), &post_output, dry_run, force, &post.name)?;
    }

    // Write wrapper scripts
    if let Some(ref script) = result.pre_script {
        let path = out_dir.join(format!("{}-pre.sh", result.main_service.name));
        write_or_print(&path, script, dry_run, force, &format!("{}-pre.sh", result.main_service.name))?;
    }
    if let Some(ref script) = result.post_script {
        let path = out_dir.join(format!("{}-post.sh", result.main_service.name));
        write_or_print(&path, script, dry_run, force, &format!("{}-post.sh", result.main_service.name))?;
    }
    if let Some(ref script) = result.stop_script {
        let path = out_dir.join(format!("{}-stop.sh", result.main_service.name));
        write_or_print(&path, script, dry_run, force, &format!("{}-stop.sh", result.main_service.name))?;
    }

    // Write env file
    if let Some(ref env_content) = result.env_file_content {
        let path = out_dir.join(format!("{}.env", result.main_service.name));
        write_or_print(&path, env_content, dry_run, force, &format!("{}.env", result.main_service.name))?;
    }

    if !dry_run {
        eprintln!(
            "{} converted {} → {}",
            "ok:".green().bold(),
            unit_file.display(),
            out_dir.join(&result.main_service.name).display()
        );
    }

    Ok(if had_warnings { 1 } else { 0 })
}

fn run_install(
    unit_file: &PathBuf,
    output_dir: Option<&std::path::Path>,
    enable: bool,
    start: bool,
    dry_run: bool,
    force: bool,
) -> Result<i32> {
    let exit_code = run_convert(unit_file, output_dir, dry_run, force)?;

    if dry_run {
        if enable {
            eprintln!("{} would run: dinitctl enable <service>", "dry-run:".cyan().bold());
        }
        if start {
            eprintln!("{} would run: dinitctl start <service>", "dry-run:".cyan().bold());
        }
        return Ok(exit_code);
    }

    let service_name = unit_file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    if enable {
        let status = process::Command::new("dinitctl")
            .args(["enable", service_name])
            .status()
            .context("failed to run dinitctl enable")?;
        if status.success() {
            eprintln!("{} enabled {}", "ok:".green().bold(), service_name);
        } else {
            eprintln!("{} dinitctl enable {} failed", "error:".red().bold(), service_name);
            return Ok(2);
        }
    }

    if start {
        let status = process::Command::new("dinitctl")
            .args(["start", service_name])
            .status()
            .context("failed to run dinitctl start")?;
        if status.success() {
            eprintln!("{} started {}", "ok:".green().bold(), service_name);
        } else {
            eprintln!("{} dinitctl start {} failed", "error:".red().bold(), service_name);
            return Ok(2);
        }
    }

    Ok(exit_code)
}

fn run_hook() -> Result<i32> {
    sd2dinit::hook::run_hook(&Config::load()?)?;
    Ok(0)
}

fn write_or_print(
    path: &PathBuf,
    content: &str,
    dry_run: bool,
    force: bool,
    label: &str,
) -> Result<()> {
    if dry_run {
        println!("{}--- {} ---{}", "\n".normal(), label.bold(), "\n".normal());
        println!("{}", content);
        return Ok(());
    }

    if path.exists() && !force {
        eprintln!(
            "{} {} already exists — use --force to overwrite",
            "skip:".yellow().bold(),
            path.display()
        );
        return Ok(());
    }

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    fs::write(path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;

    // Make wrapper scripts executable
    #[cfg(unix)]
    if path.extension().map(|e| e == "sh").unwrap_or(false) {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }

    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: compiles successfully

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: implement CLI with convert, install, and hook subcommands

Includes --dry-run, --force, colored output, drop-in directory scanning,
template/non-service unit detection, and dinitctl enable/start for install."
```

---

### Task 10: Hook Mode

**Files:**
- Modify: `src/hook.rs`
- Create: `hooks/sd2dinit.hook`

- [ ] **Step 1: Implement hook mode**

Replace `src/hook.rs`:

```rust
use std::io::{self, BufRead};
use std::path::PathBuf;

use crate::config::Config;
use crate::converter;
use crate::generator;
use crate::model::Severity;
use crate::parser::SystemdUnit;

pub fn run_hook(config: &Config) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut converted = 0;
    let mut skipped = 0;

    for line in stdin.lock().lines() {
        let line = line?;
        let path = PathBuf::from(line.trim());

        // Only process .service files
        if path.extension().map(|e| e != "service").unwrap_or(true) {
            continue;
        }

        // Skip template units
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if stem.contains('@') {
                eprintln!("  skip: template unit {}", path.display());
                skipped += 1;
                continue;
            }
        }

        // Check ignored list
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            if config.ignored_units.contains(&name.to_string()) {
                skipped += 1;
                continue;
            }
        }

        // Construct full path (hook targets are relative to filesystem root)
        let full_path = PathBuf::from("/").join(&path);
        if !full_path.exists() {
            continue;
        }

        match convert_unit(&full_path, config) {
            Ok(name) => {
                eprintln!("  converted: {}", name);
                converted += 1;
            }
            Err(e) => {
                eprintln!("  error converting {}: {:#}", path.display(), e);
                skipped += 1;
            }
        }
    }

    eprintln!("sd2dinit hook: {} converted, {} skipped", converted, skipped);
    Ok(())
}

fn convert_unit(path: &PathBuf, config: &Config) -> anyhow::Result<String> {
    let content = std::fs::read_to_string(path)?;
    let unit = SystemdUnit::parse(&content, path.clone())?;
    let result = converter::convert(&unit, config)?;

    // Write main service
    let main_output = generator::generate(&result.main_service);
    let out_path = config.output_dir.join(&result.main_service.name);

    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out_path, &main_output)?;

    // Write pre/post services
    if let Some(ref pre) = result.pre_service {
        let output = generator::generate(pre);
        std::fs::write(config.output_dir.join(&pre.name), &output)?;
    }
    if let Some(ref post) = result.post_service {
        let output = generator::generate(post);
        std::fs::write(config.output_dir.join(&post.name), &output)?;
    }

    // Write scripts
    if let Some(ref script) = result.pre_script {
        let path = config.output_dir.join(format!("{}-pre.sh", result.main_service.name));
        std::fs::write(&path, script)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    if let Some(ref script) = result.post_script {
        let path = config.output_dir.join(format!("{}-post.sh", result.main_service.name));
        std::fs::write(&path, script)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    if let Some(ref script) = result.stop_script {
        let path = config.output_dir.join(format!("{}-stop.sh", result.main_service.name));
        std::fs::write(&path, script)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
        }
    }

    // Write env file
    if let Some(ref env_content) = result.env_file_content {
        let path = config.output_dir.join(format!("{}.env", result.main_service.name));
        std::fs::write(&path, env_content)?;
    }

    // Print warnings
    for w in &result.warnings {
        let prefix = match w.severity {
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Error => "error",
        };
        eprintln!("    {}: [{}] {}", prefix, w.directive, w.message);
    }

    Ok(result.main_service.name.clone())
}
```

- [ ] **Step 2: Create the alpm hook file**

Create `hooks/sd2dinit.hook`:
```ini
[Trigger]
Type = Path
Operation = Install
Operation = Upgrade
Target = usr/lib/systemd/system/*.service

[Action]
Description = Converting systemd units to dinit...
When = PostTransaction
Exec = /usr/bin/sd2dinit hook
NeedsTargets
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: compiles successfully

- [ ] **Step 4: Commit**

```bash
git add src/hook.rs hooks/sd2dinit.hook
git commit -m "feat: implement pacman hook mode and alpm hook file

Reads target file paths from stdin, filters for .service files,
respects ignored_units config, and batch converts all matching units."
```

---

### Task 11: Integration Tests

**Files:**
- Create: `tests/integration_tests.rs`

- [ ] **Step 1: Write integration tests**

Create `tests/integration_tests.rs`:
```rust
use sd2dinit::config::Config;
use sd2dinit::converter;
use sd2dinit::generator;
use sd2dinit::parser::SystemdUnit;
use std::path::PathBuf;

fn full_convert(input: &str, filename: &str) -> String {
    let unit = SystemdUnit::parse(input, PathBuf::from(filename)).unwrap();
    let config = Config::default();
    let result = converter::convert(&unit, &config).unwrap();
    generator::generate(&result.main_service)
}

#[test]
fn test_integration_sshd() {
    let input = "\
[Unit]
Description=OpenSSH Daemon
Wants=sshdgenkeys.service
After=sshdgenkeys.service
After=network.target

[Service]
ExecStart=/usr/bin/sshd -D
ExecReload=/bin/kill -HUP $MAINPID
Restart=always

[Install]
WantedBy=multi-user.target
";
    let output = full_convert(input, "/usr/lib/systemd/system/sshd.service");

    assert!(output.contains("# Generated by sd2dinit from /usr/lib/systemd/system/sshd.service"));
    assert!(output.contains("type = process"));
    assert!(output.contains("command = /usr/bin/sshd -D"));
    assert!(output.contains("restart = true"));
    assert!(output.contains("smooth-recovery = true"));
    assert!(output.contains("depends-ms = sshdgenkeys"));
    assert!(output.contains("waits-for = sshdgenkeys"));
    assert!(output.contains("waits-for = network"));
}

#[test]
fn test_integration_nginx_forking() {
    let input = "\
[Unit]
Description=A high performance web server
After=network-online.target
Wants=network-online.target

[Service]
Type=forking
PIDFile=/run/nginx.pid
ExecStartPre=/usr/bin/nginx -t -q
ExecStart=/usr/bin/nginx
ExecStop=/bin/kill -s QUIT $MAINPID
User=root

[Install]
WantedBy=multi-user.target
";
    let unit = SystemdUnit::parse(input, PathBuf::from("/usr/lib/systemd/system/nginx.service")).unwrap();
    let config = Config::default();
    let result = converter::convert(&unit, &config).unwrap();
    let output = generator::generate(&result.main_service);

    assert!(output.contains("type = bgprocess"));
    assert!(output.contains("pid-file = /run/nginx.pid"));
    assert!(output.contains("command = /usr/bin/nginx"));
    assert!(output.contains("stop-command = /bin/kill -s QUIT $MAINPID"));
    assert!(output.contains("run-as = root"));
    // Should have a pre-service
    assert!(result.pre_service.is_some());
    assert!(output.contains("depends-on = nginx-pre"));
    // network-online.target mapped to network
    assert!(output.contains("depends-ms = network"));
    assert!(output.contains("waits-for = network"));
}

#[test]
fn test_integration_docker_notify() {
    let input = "\
[Unit]
Description=Docker Application Container Engine
After=network-online.target containerd.service
Wants=network-online.target containerd.service
Requires=docker.socket

[Service]
Type=notify
ExecStart=/usr/bin/dockerd -H fd://
ExecReload=/bin/kill -s HUP $MAINPID
Restart=always
RestartSec=2
LimitNOFILE=infinity
TasksMax=infinity
Delegate=yes
OOMScoreAdjust=-500

[Install]
WantedBy=multi-user.target
";
    let unit = SystemdUnit::parse(input, PathBuf::from("/usr/lib/systemd/system/docker.service")).unwrap();
    let config = Config::default();
    let result = converter::convert(&unit, &config).unwrap();
    let output = generator::generate(&result.main_service);

    // notify falls back to process
    assert!(output.contains("type = process"));
    assert!(output.contains("restart = true"));
    assert!(output.contains("restart-delay = 2"));
    assert!(output.contains("depends-on = docker"));  // docker.socket → docker
    assert!(output.contains("depends-ms = network"));  // network-online.target → network
    assert!(output.contains("depends-ms = containerd"));
    // Should have notify warning
    assert!(result.warnings.iter().any(|w| w.message.contains("notify")));
    // Should have cgroup warnings
    assert!(result.warnings.iter().any(|w| w.directive == "TasksMax"));
    assert!(result.warnings.iter().any(|w| w.directive == "Delegate"));
}

#[test]
fn test_integration_oneshot_with_environment() {
    let input = "\
[Unit]
Description=Setup tmpfiles

[Service]
Type=oneshot
ExecStart=/usr/bin/systemd-tmpfiles --create
Environment=TMPDIR=/tmp
Environment=LANG=C
EnvironmentFile=/etc/locale.conf
WorkingDirectory=/
";
    let unit = SystemdUnit::parse(input, PathBuf::from("/usr/lib/systemd/system/tmpfiles.service")).unwrap();
    let config = Config::default();
    let result = converter::convert(&unit, &config).unwrap();
    let output = generator::generate(&result.main_service);

    assert!(output.contains("type = scripted"));
    assert!(output.contains("working-dir = /"));
    // Should have generated env file + passthrough
    assert!(result.env_file_content.is_some());
    let env = result.env_file_content.unwrap();
    assert!(env.contains("TMPDIR=/tmp"));
    assert!(env.contains("LANG=C"));
    assert_eq!(result.main_service.env_files.len(), 2);
}

#[test]
fn test_integration_drop_in_override() {
    let base = "\
[Service]
Type=simple
ExecStart=/usr/bin/original-daemon
ExecStartPre=/usr/bin/original-setup
Restart=no
";
    let drop_in = "\
[Service]
ExecStartPre=
ExecStartPre=/usr/bin/custom-setup
Restart=always
";
    let mut unit = SystemdUnit::parse(base, PathBuf::from("/usr/lib/systemd/system/myapp.service")).unwrap();
    unit.merge_drop_in(drop_in, PathBuf::from("/etc/systemd/system/myapp.service.d/override.conf"));

    let config = Config::default();
    let result = converter::convert(&unit, &config).unwrap();
    let output = generator::generate(&result.main_service);

    // ExecStart should be unchanged (not reset in drop-in)
    assert!(output.contains("command = /usr/bin/original-daemon"));
    // Restart was overridden
    assert!(output.contains("restart = true"));
    // Pre should use custom, not original
    assert!(result.pre_service.is_some());
    let pre = result.pre_service.unwrap();
    assert!(pre.command.unwrap().contains("custom-setup"));
}

#[test]
fn test_integration_mixed_pre_with_dash() {
    let input = "\
[Service]
ExecStartPre=/usr/bin/must-work
ExecStartPre=-/usr/bin/optional
ExecStart=/usr/bin/daemon
";
    let unit = SystemdUnit::parse(input, PathBuf::from("/usr/lib/systemd/system/mixed.service")).unwrap();
    let config = Config::default();
    let result = converter::convert(&unit, &config).unwrap();

    assert!(result.pre_script.is_some());
    let script = result.pre_script.unwrap();
    assert!(script.starts_with("#!/bin/sh\nset -e\n"));
    assert!(script.contains("/usr/bin/must-work\n"));
    assert!(script.contains("/usr/bin/optional || true\n"));
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo test`
Expected: all unit tests + integration tests PASS

- [ ] **Step 3: Commit**

```bash
git add tests/integration_tests.rs
git commit -m "test: add integration tests for full conversion pipeline

Covers sshd, nginx (forking), docker (notify), oneshot with env,
drop-in overrides, and mixed pre-command dash prefixes."
```

---

### Task 12: Final Verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 2: Build release binary**

Run: `cargo build --release`
Expected: binary at `target/release/sd2dinit`

- [ ] **Step 3: Test --dry-run with a sample service**

Create a test file and run:
```bash
mkdir -p /tmp/sd2dinit-test
cat > /tmp/sd2dinit-test/sshd.service << 'EOF'
[Unit]
Description=OpenSSH Daemon
After=network.target

[Service]
ExecStart=/usr/bin/sshd -D
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

cargo run -- convert /tmp/sd2dinit-test/sshd.service --dry-run
```

Expected: prints the generated dinit service file to stdout without writing.

- [ ] **Step 4: Test actual file output**

```bash
cargo run -- convert /tmp/sd2dinit-test/sshd.service --output-dir /tmp/sd2dinit-test/output/
cat /tmp/sd2dinit-test/output/sshd
```

Expected: dinit service file written to `/tmp/sd2dinit-test/output/sshd`.

- [ ] **Step 5: Commit any fixes from verification**

If tests revealed issues, fix and commit. Otherwise, no action needed.

- [ ] **Step 6: Final commit — clean up and tag**

```bash
git add -A
git status  # verify no unexpected files
```

If clean, the project is complete.
