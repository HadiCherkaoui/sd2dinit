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
        ConvertError::NoExecStart { unit: name.clone() }
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

    // Command — replace known specifiers, then convert $VAR to dinit's native
    // $/VAR word-splitting form. EnvironmentFile= entries are parsed at
    // conversion time and rewritten as a dinit-compatible env-file, so dinit
    // can read them directly without a shell wrapper.
    let command = {
        let cmd = replace_specifiers(exec_start, &name, &mut warnings);
        convert_env_refs(cmd)
    };

    // Stop command
    let stop_command = unit.get("Service", "ExecStop").map(|s| s.to_string());

    // User and Group
    let user = unit.get("Service", "User").map(|s| s.to_string());
    let group = unit.get("Service", "Group").map(|s| s.to_string());

    // Working directory
    let working_dir = unit.get("Service", "WorkingDirectory").map(PathBuf::from);

    // PID file
    let pid_file = unit.get("Service", "PIDFile").map(PathBuf::from);

    // Restart mapping
    let (restart, smooth_recovery) = convert_restart(unit.get("Service", "Restart"), &mut warnings);

    // Restart delay
    let restart_delay = unit.get("Service", "RestartSec").and_then(|s| {
        let s = s.trim_end_matches('s');
        s.parse::<f64>().ok()
    });

    // Environment — parse shell-format EnvironmentFile entries and rewrite them
    // as a dinit-compatible combined env-file.
    let (env_files, env_file_content) = convert_environment(unit, config, &name, &mut warnings);

    // Dependencies
    let (depends_on, depends_ms, waits_for) = convert_dependencies(unit, config, &mut warnings);

    // ExecStartPre / ExecStartPost
    let (pre_service, pre_script) = convert_exec_pre(unit, &name, &unit.source_path.clone(), config, &mut warnings);
    let (post_service, post_script) = convert_exec_post(unit, &name, &unit.source_path.clone(), config, &mut warnings);

    // ExecStopPost
    let (final_stop_command, stop_script) = convert_stop_post(unit, stop_command, &name, config, &mut warnings);

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

/// Converts systemd `$VAR` references in a command string to dinit syntax.
///
/// Standalone tokens that are exactly `$VAR` or `${VAR}` (the entire argument
/// is the variable) become `$/VAR` — dinit's word-splitting form. `$/VAR`
/// expands the value and splits it on whitespace into zero-or-more arguments,
/// collapsing entirely when the variable is empty or unset. This matches
/// systemd's behaviour for argument-list variables like `$EARLYOOM_ARGS`.
///
/// Embedded references (e.g. `--path=$VAR/sub`) are kept as `$VAR` since
/// the surrounding context constrains them to a single token.
fn convert_env_refs(cmd: String) -> String {
    if !cmd.contains('$') {
        return cmd;
    }
    cmd.split_whitespace()
        .map(|token| {
            if let Some(rest) = token.strip_prefix('$') {
                // Braced form: ${VAR}
                let inner = if rest.starts_with('{') && rest.ends_with('}') {
                    &rest[1..rest.len() - 1]
                } else {
                    rest
                };
                // Only convert if the token is EXACTLY a variable name
                // (alphanumeric + underscores — no surrounding text).
                if !inner.is_empty() && inner.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    return format!("$/{}", inner);
                }
            }
            token.to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn replace_specifiers(input: &str, service_name: &str, warnings: &mut Vec<Warning>) -> String {
    // First replace known specifiers
    let input = input.replace("%n", &format!("{}.service", service_name));
    let input = input.replace("%N", service_name);

    // Single pass: remove unknown specifiers with warning (deduplicated)
    let mut result = String::with_capacity(input.len());
    let mut warned: std::collections::HashSet<char> = std::collections::HashSet::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            if let Some(&next) = chars.peek() {
                if next.is_alphabetic() {
                    // Unknown specifier — consume and warn once
                    chars.next();
                    if warned.insert(next) {
                        warnings.push(Warning {
                            directive: "ExecStart".into(),
                            message: format!("unknown specifier %{} removed", next),
                            severity: Severity::Warn,
                        });
                    }
                    continue;
                }
            }
        }
        result.push(c);
    }
    result
}

fn convert_restart(restart_value: Option<&str>, warnings: &mut Vec<Warning>) -> (RestartPolicy, bool) {
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
    warnings: &mut Vec<Warning>,
) -> (Vec<PathBuf>, Option<String>) {
    let mut vars: Vec<(String, String)> = Vec::new();

    // Inline Environment= directives
    for val in unit.get_all("Service", "Environment") {
        let val = val.trim_matches('"').trim_matches('\'');
        if let Some(eq) = val.find('=') {
            vars.push((val[..eq].to_string(), val[eq + 1..].to_string()));
        }
    }

    // EnvironmentFile= entries — read and parse shell-format files, rewriting
    // them into dinit-compatible KEY=VALUE format in one combined env-file.
    // This avoids dinit having to parse shell quoting syntax (single-quoted
    // values, embedded $ characters, etc.) that it does not support.
    for val in unit.get_all("Service", "EnvironmentFile") {
        let (optional, path_str) = match val.strip_prefix('-') {
            Some(p) => (true, p),
            None    => (false, val),
        };
        let path = std::path::Path::new(path_str);
        if !path.exists() {
            warnings.push(Warning {
                directive: "EnvironmentFile".into(),
                message: if optional {
                    format!("optional env-file {path_str} not found — skipped")
                } else {
                    format!("env-file {path_str} not found")
                },
                severity: if optional { Severity::Info } else { Severity::Warn },
            });
            continue;
        }
        match std::fs::read_to_string(path) {
            Ok(content) => vars.extend(parse_shell_env_file(&content)),
            Err(e) => warnings.push(Warning {
                directive: "EnvironmentFile".into(),
                message: format!("could not read {path_str}: {e}"),
                severity: Severity::Warn,
            }),
        }
    }

    if vars.is_empty() {
        return (Vec::new(), None);
    }

    let env_path = config.output_dir.join(format!("{}.env", service_name));
    let content = vars
        .iter()
        .map(|(k, v)| format!("{}={}\n", k, dinit_quote_value(v)))
        .collect::<String>();
    (vec![env_path], Some(content))
}

/// Parses a shell-format env-file (e.g. `/etc/default/*`) into key-value pairs
/// with shell quoting removed.
fn parse_shell_env_file(content: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line
            .strip_prefix("export")
            .map(str::trim_start)
            .unwrap_or(line);
        let Some(eq) = line.find('=') else { continue };
        let key = line[..eq].trim().to_string();
        if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        result.push((key, parse_shell_value(&line[eq + 1..])));
    }
    result
}

/// Strips shell quoting from a raw env-file value string.
fn parse_shell_value(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('\'') {
        // Single-quoted: everything literal up to the closing `'`.
        let end = s[1..].find('\'').map(|i| i + 1).unwrap_or(s.len());
        s[1..end].to_string()
    } else if s.starts_with('"') {
        parse_double_quoted_value(&s[1..])
    } else {
        s.to_string()
    }
}

/// Parses a double-quoted shell value, processing `\` escape sequences.
/// `$VAR` and `${VAR}` references are consumed but not expanded — we cannot
/// resolve them without a running shell, and the most common use case is
/// standalone argument-list variables (`$DAEMON_ARGS`) rather than embedded
/// substitutions.
fn parse_double_quoted_value(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => break,
            '\\' => match chars.next() {
                // Only these have special meaning inside double quotes (POSIX)
                Some(c @ ('"' | '\\' | '$' | '`')) => result.push(c),
                Some('\n') => {} // line continuation
                Some(c) => { result.push('\\'); result.push(c); }
                None => result.push('\\'),
            },
            '$' => {
                let is_var = chars.peek().map_or(false, |&c| {
                    c.is_alphabetic() || c == '_' || c == '{'
                });
                if is_var {
                    if chars.peek() == Some(&'{') {
                        chars.next();
                        for c in chars.by_ref() { if c == '}' { break; } }
                    } else {
                        while chars.peek().map_or(false, |c| c.is_alphanumeric() || *c == '_') {
                            chars.next();
                        }
                    }
                    // Variable not expanded — emit nothing
                } else {
                    result.push('$'); // literal $
                }
            }
            c => result.push(c),
        }
    }
    result
}

/// Quotes a value string for use in a dinit-format env-file.
///
/// - No `'` in value → single-quote it (no `$` expansion possible).
/// - Has `'` but no `$` → double-quote it.
/// - Has both → double-quote with `$` escaped as `\$`.
fn dinit_quote_value(value: &str) -> String {
    let has_sq = value.contains('\'');
    let has_dollar = value.contains('$');
    if !has_sq {
        format!("'{value}'")
    } else if !has_dollar {
        let esc = value.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{esc}\"")
    } else {
        let esc = value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$");
        format!("\"{esc}\"")
    }
}

fn convert_dependencies(
    unit: &SystemdUnit,
    config: &Config,
    warnings: &mut Vec<Warning>,
) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut depends_on = Vec::new();
    let mut depends_ms = Vec::new();
    let mut waits_for = Vec::new();

    let resolve = |dep: &str| -> String {
        let dep = dep.trim();
        if let Some(mapped) = config.dependency_map.get(dep) {
            return mapped.clone();
        }
        // Strip .service suffix; leave .target, .socket, etc. stripped too
        let stripped = dep
            .strip_suffix(".service")
            .or_else(|| dep.strip_suffix(".target"))
            .or_else(|| dep.strip_suffix(".socket"))
            .or_else(|| dep.strip_suffix(".mount"))
            .or_else(|| dep.strip_suffix(".path"))
            .or_else(|| dep.strip_suffix(".timer"))
            .unwrap_or(dep);
        stripped.to_string()
    };

    for val in unit.get_all("Unit", "Requires") {
        for dep in val.split_whitespace() {
            depends_on.push(resolve(dep));
        }
    }

    for val in unit.get_all("Unit", "Wants") {
        for dep in val.split_whitespace() {
            depends_ms.push(resolve(dep));
        }
    }

    for val in unit.get_all("Unit", "After") {
        for dep in val.split_whitespace() {
            waits_for.push(resolve(dep));
        }
    }

    if !unit.get_all("Unit", "Before").is_empty() {
        warnings.push(Warning {
            directive: "Before".into(),
            message: "Before= skipped (no direct dinit equivalent)".into(),
            severity: Severity::Info,
        });
    }

    if !unit.get_all("Unit", "Conflicts").is_empty() {
        warnings.push(Warning {
            directive: "Conflicts".into(),
            message: "Conflicts= has no direct dinit equivalent — skipped".into(),
            severity: Severity::Warn,
        });
    }

    (depends_on, depends_ms, waits_for)
}

fn build_script_command(config: &Config, service_name: &str, suffix: &str) -> String {
    format!(
        "/bin/sh {}/{}-{}.sh",
        config.output_dir.display(),
        service_name,
        suffix
    )
}

fn convert_exec_pre(
    unit: &SystemdUnit,
    service_name: &str,
    source_path: &PathBuf,
    config: &Config,
    _warnings: &mut Vec<Warning>,
) -> (Option<DinitService>, Option<String>) {
    let pre_cmds = unit.get_all("Service", "ExecStartPre");
    if pre_cmds.is_empty() {
        return (None, None);
    }

    let (command, script) = if pre_cmds.len() == 1 {
        let cmd = pre_cmds[0];
        let (is_dash, clean_cmd) = parse_dash_prefix(cmd);
        if is_dash {
            let script_content = format!("#!/bin/sh\nset -e\n{} || true\n", clean_cmd);
            (build_script_command(config, service_name, "pre"), Some(script_content))
        } else {
            (clean_cmd.to_string(), None)
        }
    } else {
        let mut script = String::from("#!/bin/sh\nset -e\n");
        for cmd in &pre_cmds {
            let (is_dash, clean_cmd) = parse_dash_prefix(cmd);
            if is_dash {
                script.push_str(&format!("{} || true\n", clean_cmd));
            } else {
                script.push_str(&format!("{}\n", clean_cmd));
            }
        }
        (build_script_command(config, service_name, "pre"), Some(script))
    };

    let pre_service = DinitService {
        name: format!("{}-pre", service_name),
        source_path: source_path.clone(),
        service_type: DinitType::Scripted,
        command: Some(command),
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
    config: &Config,
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
            (build_script_command(config, service_name, "post"), Some(script_content))
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
        (build_script_command(config, service_name, "post"), Some(script))
    };

    let post_service = DinitService {
        name: format!("{}-post", service_name),
        source_path: source_path.clone(),
        service_type: DinitType::Scripted,
        command: Some(command),
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
        waits_for: vec![service_name.to_string()],
        logfile: None,
    };

    (Some(post_service), script)
}

fn convert_stop_post(
    unit: &SystemdUnit,
    stop_command: Option<String>,
    service_name: &str,
    config: &Config,
    warnings: &mut Vec<Warning>,
) -> (Option<String>, Option<String>) {
    let stop_post_cmds = unit.get_all("Service", "ExecStopPost");
    if stop_post_cmds.is_empty() {
        return (stop_command, None);
    }

    match stop_command {
        Some(stop_cmd) => {
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
            let wrapper_cmd = build_script_command(config, service_name, "stop");
            (Some(wrapper_cmd), Some(script))
        }
        None => {
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
    // [Service] directives that are out of scope
    const SANDBOXING: &[&str] = &[
        "ProtectSystem", "ProtectHome", "PrivateTmp", "PrivateDevices",
        "PrivateNetwork", "ProtectKernelTunables", "ProtectKernelModules",
        "ProtectControlGroups", "NoNewPrivileges", "ReadOnlyPaths",
        "ReadWritePaths", "InaccessiblePaths", "ProtectHostname",
        "LockPersonality", "MemoryDenyWriteExecute", "RestrictRealtime",
        "RestrictSUIDSGID", "RestrictNamespaces", "SystemCallFilter",
        "SystemCallArchitectures", "CapabilityBoundingSet", "AmbientCapabilities",
        "SecureBits", "ProtectClock", "ProtectKernelLogs", "IPAddressDeny",
        "RestrictAddressFamilies", "PrivateUsers",
        // DynamicUser creates an ephemeral unprivileged UID at runtime; dinit
        // has no equivalent so the service runs as root instead.
        "DynamicUser", "SupplementaryGroups",
    ];
    const CGROUP: &[&str] = &[
        "Slice", "CPUQuota", "MemoryMax", "MemoryHigh", "MemoryLow",
        "IOWeight", "IODeviceWeight", "TasksMax", "Delegate",
    ];

    // [Unit] directives that are out of scope
    const CONDITIONALS: &[&str] = &[
        "ConditionPathExists", "ConditionPathIsDirectory",
        "ConditionFileNotEmpty", "ConditionDirectoryNotEmpty",
        "ConditionKernelCommandLine", "ConditionVirtualization",
        "ConditionArchitecture", "ConditionSecurity",
        "AssertPathExists",
    ];

    // [Socket] directives that are out of scope
    const SOCKET: &[&str] = &["ListenStream", "ListenDatagram", "ListenSequentialPacket", "Accept"];

    // Scan [Service] for sandboxing and cgroup directives
    if let Some(pairs) = unit.sections.get("Service") {
        let mut seen = std::collections::HashSet::new();
        for (key, _) in pairs {
            if !seen.insert(key.clone()) {
                continue;
            }
            if SANDBOXING.contains(&key.as_str()) {
                warnings.push(Warning {
                    directive: key.clone(),
                    message: format!("{} (sandboxing) not supported — skipped", key),
                    severity: Severity::Info,
                });
            } else if CGROUP.contains(&key.as_str()) {
                warnings.push(Warning {
                    directive: key.clone(),
                    message: format!("{} (cgroup) not supported — skipped", key),
                    severity: Severity::Info,
                });
            }
        }
    }

    // Scan [Unit] for conditional directives
    if let Some(pairs) = unit.sections.get("Unit") {
        let mut seen = std::collections::HashSet::new();
        for (key, _) in pairs {
            if !seen.insert(key.clone()) {
                continue;
            }
            if CONDITIONALS.contains(&key.as_str()) {
                warnings.push(Warning {
                    directive: key.clone(),
                    message: format!("{} (conditional) not supported — skipped", key),
                    severity: Severity::Warn,
                });
            }
        }
    }

    // Scan [Socket] for socket activation directives
    if let Some(pairs) = unit.sections.get("Socket") {
        let mut seen = std::collections::HashSet::new();
        for (key, _) in pairs {
            if !seen.insert(key.clone()) {
                continue;
            }
            if SOCKET.contains(&key.as_str()) {
                warnings.push(Warning {
                    directive: key.clone(),
                    message: format!("{} (socket activation) not supported — skipped", key),
                    severity: Severity::Warn,
                });
            }
        }
    }
}

fn parse_dash_prefix(cmd: &str) -> (bool, &str) {
    let cmd = cmd.trim();
    if let Some(rest) = cmd.strip_prefix('-') {
        (true, rest.trim())
    } else {
        (false, cmd)
    }
}
