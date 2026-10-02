// SPDX-FileCopyrightText: Hadi Cherkaoui <contact@hide.cherkaoui.ch>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::error::ConvertError;
use crate::model::{ConversionResult, DinitService, DinitType, RestartPolicy, Severity, Warning};
use crate::parser::SystemdUnit;
use crate::services::KnownServices;

/// Converts a parsed systemd unit into dinit service descriptions.
///
/// Dependencies are only emitted on services in `known` (or mapped through
/// `config.dependency_map`), because dinit refuses to load a service whose
/// `depends-on` or `waits-for` names a service it cannot find. Everything that
/// cannot be expressed in dinit is reported in [`ConversionResult::warnings`].
///
/// # Errors
///
/// Returns [`ConvertError::NoExecStart`] when the unit has no `ExecStart=`.
pub fn convert(
    unit: &SystemdUnit,
    config: &Config,
    known: &KnownServices,
) -> Result<ConversionResult, ConvertError> {
    let mut warnings: Vec<Warning> = Vec::new();

    // Derive service name from file path
    let name = unit
        .source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // ExecStart is required
    let exec_start = unit
        .get("Service", "ExecStart")
        .ok_or_else(|| ConvertError::NoExecStart { unit: name.clone() })?;

    // Type mapping
    let raw_type = unit.get("Service", "Type").unwrap_or("simple");
    let has_pid_file = unit.get("Service", "PIDFile").is_some();

    let service_type = match raw_type {
        "simple" | "exec" | "idle" => DinitType::Process,
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
    let (exec_prefix, exec_cmd) = clean_exec("ExecStart", exec_start, &mut warnings);
    if exec_cmd.is_empty() {
        return Err(ConvertError::NoExecStart { unit: name });
    }
    if exec_prefix.ignore_failure {
        warnings.push(Warning {
            directive: "ExecStart".into(),
            message: "'-' prefix dropped — dinit treats a failing exit as a failure".into(),
            severity: Severity::Info,
        });
    }
    let command = {
        let cmd = replace_specifiers(&exec_cmd, &name, &mut warnings);
        if exec_prefix.no_env_expansion {
            exec_prefix.quote(&cmd, Quoting::Dinit)
        } else {
            convert_env_refs(cmd)
        }
    };

    // Stop command
    let stop_command = unit
        .get("Service", "ExecStop")
        .map(|raw| clean_exec("ExecStop", raw, &mut warnings));

    // User and Group
    let user = unit.get("Service", "User").map(|s| s.to_string());
    match (user.as_deref(), unit.get("Service", "Group")) {
        (Some(user), _) if user.bytes().all(|b| b.is_ascii_digit()) => warnings.push(Warning {
            directive: "User".into(),
            message: format!(
                "User={user} is numeric — dinit then keeps its own group (root) and drops supplementary groups; use a user name"
            ),
            severity: Severity::Warn,
        }),
        (Some(user), Some(group)) if group != user => warnings.push(Warning {
            directive: "Group".into(),
            message: format!(
                "Group={group} ignored — dinit runs the service with {user}'s primary group"
            ),
            severity: Severity::Warn,
        }),
        (None, Some(group)) => warnings.push(Warning {
            directive: "Group".into(),
            message: format!(
                "Group={group} without User= ignored — dinit's run-as takes only a user"
            ),
            severity: Severity::Warn,
        }),
        _ => {}
    }

    // Working directory
    let working_dir = unit.get("Service", "WorkingDirectory").map(PathBuf::from);

    // PID file
    let pid_file = unit.get("Service", "PIDFile").map(PathBuf::from);

    // Restart mapping
    let (restart, smooth_recovery) = convert_restart(unit.get("Service", "Restart"), &mut warnings);

    // Restart delay
    let restart_delay = unit.get("Service", "RestartSec").and_then(|span| {
        let secs = parse_timespan_secs(span);
        if secs.is_none() {
            warnings.push(Warning {
                directive: "RestartSec".into(),
                message: format!(
                    "RestartSec={span} is not a time span sd2dinit understands — dropped"
                ),
                severity: Severity::Warn,
            });
        }
        secs
    });

    // Environment — parse shell-format EnvironmentFile entries and rewrite them
    // as a dinit-compatible combined env-file.
    let (env_files, env_file_content) = convert_environment(unit, config, &name, &mut warnings);

    // Dependencies
    let deps = convert_dependencies(unit, &name, config, known, &mut warnings);

    // ExecStartPre / ExecStartPost
    let (pre_service, pre_script) =
        convert_exec_pre(unit, &name, &unit.source_path, config, &mut warnings);
    let (post_service, post_script) =
        convert_exec_post(unit, &name, &unit.source_path, config, &mut warnings);

    // ExecStopPost
    let (final_stop_command, stop_script) =
        convert_stop_post(unit, stop_command, &name, config, &mut warnings);

    // WantedBy / RequiredBy in [Install]
    let should_enable =
        unit.get("Install", "WantedBy").is_some() || unit.get("Install", "RequiredBy").is_some();

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

    let mut main_depends_on = deps.depends_on;
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
        working_dir,
        env_files,
        pid_file,
        restart,
        smooth_recovery,
        restart_delay,
        depends_on: main_depends_on,
        waits_for: deps.waits_for,
        after: deps.after,
        before: deps.before,
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
        if c == '%'
            && let Some(&next) = chars.peek()
            && next.is_alphabetic()
        {
            // Unknown specifier — consume and warn once
            chars.next();
            if warned.insert(next) {
                warnings.push(Warning {
                    directive: "ExecStart".into(),
                    message: format!("unknown specifier %{next} removed"),
                    severity: Severity::Warn,
                });
            }
            continue;
        }
        result.push(c);
    }
    result
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
                message:
                    "on-success maps to always-restart; dinit has no clean-exit-only restart mode"
                        .into(),
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
                message: format!(
                    "unknown restart value '{}' — defaulting to no restart",
                    other
                ),
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
        vars.extend(split_environment(val));
    }

    // EnvironmentFile= entries — read and parse shell-format files, rewriting
    // them into dinit-compatible KEY=VALUE format in one combined env-file.
    // This avoids dinit having to parse shell quoting syntax (single-quoted
    // values, embedded $ characters, etc.) that it does not support.
    for val in unit.get_all("Service", "EnvironmentFile") {
        let (optional, path_str) = match val.strip_prefix('-') {
            Some(p) => (true, p),
            None => (false, val),
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
                severity: if optional {
                    Severity::Info
                } else {
                    Severity::Warn
                },
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

/// Splits an `Environment=` value into its assignments.
///
/// systemd allows several space-separated assignments on one line, each
/// optionally wrapped in double or single quotes, e.g.
/// `"A=1 2" B=3 'C=x y'`. Words without `=` are ignored, as systemd does.
fn split_environment(value: &str) -> Vec<(String, String)> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' | '\'' => {
                in_word = true;
                while let Some(q) = chars.next() {
                    if q == c {
                        break;
                    }
                    if q == '\\' && c == '"' {
                        word.extend(chars.next());
                    } else {
                        word.push(q);
                    }
                }
            }
            '\\' => {
                in_word = true;
                word.extend(chars.next());
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    words
        .into_iter()
        .filter_map(|w| {
            let (key, val) = w.split_once('=')?;
            (!key.is_empty()).then(|| (key.to_string(), val.to_string()))
        })
        .collect()
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
    if let Some(inner) = s.strip_prefix('\'') {
        // Single-quoted: everything literal up to the closing `'`.
        let end = inner.find('\'').unwrap_or(inner.len());
        inner[..end].to_string()
    } else if let Some(inner) = s.strip_prefix('"') {
        parse_double_quoted_value(inner)
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
                Some(c) => {
                    result.push('\\');
                    result.push(c);
                }
                None => result.push('\\'),
            },
            '$' => {
                let is_var = chars
                    .peek()
                    .is_some_and(|&c| c.is_alphabetic() || c == '_' || c == '{');
                if is_var {
                    if chars.peek() == Some(&'{') {
                        chars.next();
                        for c in chars.by_ref() {
                            if c == '}' {
                                break;
                            }
                        }
                    } else {
                        while chars
                            .peek()
                            .is_some_and(|c| c.is_alphanumeric() || *c == '_')
                        {
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

/// Formats a value for use in a dinit env-file.
///
/// Dinit's env-file parser is purely literal — no quoting syntax, no escape
/// sequences, no variable substitution. Everything from `=` to end of line
/// becomes the value verbatim. Writing the shell-stripped value as-is is
/// both correct and sufficient.
fn dinit_quote_value(value: &str) -> String {
    value.to_string()
}

/// How a dinit service relates to one of its dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Relation {
    DependsOn,
    WaitsFor,
    After,
    Before,
}

/// The systemd directives sd2dinit maps, strongest relation first.
///
/// Processing them in this order lets the first placement of a name win, so a
/// service listed under several directives keeps only its strongest relation.
const DEPENDENCY_DIRECTIVES: [(&str, Relation); 4] = [
    ("Requires", Relation::DependsOn),
    ("Wants", Relation::WaitsFor),
    ("After", Relation::After),
    ("Before", Relation::Before),
];

#[derive(Debug, Default)]
struct Dependencies {
    depends_on: Vec<String>,
    waits_for: Vec<String>,
    after: Vec<String>,
    before: Vec<String>,
}

fn convert_dependencies(
    unit: &SystemdUnit,
    service_name: &str,
    config: &Config,
    known: &KnownServices,
    warnings: &mut Vec<Warning>,
) -> Dependencies {
    let mut resolved: Vec<(&str, Relation, &str, String)> = Vec::new();
    for (directive, relation) in DEPENDENCY_DIRECTIVES {
        for dep in unit
            .get_all("Unit", directive)
            .into_iter()
            .flat_map(str::split_whitespace)
        {
            let Some(name) = resolve_dependency(dep, config, known) else {
                warnings.push(Warning {
                    directive: directive.into(),
                    message: format!("{dep}: no dinit service of that name — dependency dropped"),
                    severity: match relation {
                        Relation::DependsOn | Relation::WaitsFor => Severity::Warn,
                        Relation::After | Relation::Before => Severity::Info,
                    },
                });
                continue;
            };
            if name == service_name {
                warnings.push(Warning {
                    directive: directive.into(),
                    message: format!("{dep} resolves to this service itself — dropped"),
                    severity: Severity::Info,
                });
                continue;
            }
            resolved.push((directive, relation, dep, name));
        }
    }

    // dinit rejects `before` plus a pull-in of the same service as a cycle;
    // the ordering is kept because it is what Before= promised.
    let before: HashSet<&str> = resolved
        .iter()
        .filter(|(_, relation, _, _)| *relation == Relation::Before)
        .map(|(_, _, _, name)| name.as_str())
        .collect();

    let mut deps = Dependencies::default();
    let mut placed: HashSet<&str> = HashSet::new();
    let mut warned: HashSet<&str> = HashSet::new();
    for (_, relation, dep, name) in &resolved {
        if *relation != Relation::Before && before.contains(name.as_str()) {
            if *relation != Relation::After && warned.insert(name) {
                warnings.push(Warning {
                    directive: "Before".into(),
                    message: format!(
                        "{dep} is also in Before= — kept as before, so it is no longer started along with this service"
                    ),
                    severity: Severity::Warn,
                });
            }
            continue;
        }
        // The first placement is the strongest, as DEPENDENCY_DIRECTIVES is ordered.
        if !placed.insert(name) {
            continue;
        }
        let list = match relation {
            Relation::DependsOn => &mut deps.depends_on,
            Relation::WaitsFor => &mut deps.waits_for,
            Relation::After => &mut deps.after,
            Relation::Before => &mut deps.before,
        };
        list.push(name.clone());
    }

    if !unit.get_all("Unit", "Conflicts").is_empty() {
        warnings.push(Warning {
            directive: "Conflicts".into(),
            message: "Conflicts= has no direct dinit equivalent — skipped".into(),
            severity: Severity::Warn,
        });
    }

    deps
}

/// Maps a systemd unit name to the dinit service it should depend on.
///
/// `dependency_map` wins outright. Otherwise the exact name (`network.target`)
/// is preferred over the suffix-stripped one (`network`), and only names that
/// exist in `known` are returned.
fn resolve_dependency(dep: &str, config: &Config, known: &KnownServices) -> Option<String> {
    if let Some(mapped) = config.dependency_map.get(dep) {
        return Some(mapped.clone());
    }
    [dep, strip_unit_suffix(dep)]
        .into_iter()
        .find(|name| known.contains(name))
        .map(str::to_owned)
}

fn strip_unit_suffix(dep: &str) -> &str {
    const SUFFIXES: &[&str] = &[
        ".service", ".target", ".socket", ".mount", ".path", ".timer",
    ];
    SUFFIXES
        .iter()
        .find_map(|suffix| dep.strip_suffix(suffix))
        .unwrap_or(dep)
}

/// systemd's special prefixes on `Exec*=` values.
#[derive(Debug, Default, Clone, Copy)]
struct ExecPrefix {
    /// `-`: a non-zero exit is not a failure.
    ignore_failure: bool,
    /// `:`: no `$VAR` expansion.
    no_env_expansion: bool,
}

/// Where a cleaned command ends up, which decides how a literal `$` is written.
#[derive(Debug, Clone, Copy)]
enum Quoting {
    /// A dinit `command`, where `$$` is a literal `$`.
    Dinit,
    /// A line in a generated `/bin/sh` script, where `\$` is.
    Shell,
}

impl ExecPrefix {
    /// Writes `cmd` for `quoting`, escaping `$` when the `:` prefix was given.
    fn quote(self, cmd: &str, quoting: Quoting) -> String {
        match (self.no_env_expansion, quoting) {
            (false, _) => cmd.to_owned(),
            (true, Quoting::Dinit) => cmd.replace('$', "$$"),
            (true, Quoting::Shell) => cmd.replace('$', "\\$"),
        }
    }

    /// Formats `cmd` as a line of a generated `/bin/sh` script running under `set -e`.
    fn script_line(self, cmd: &str) -> String {
        let cmd = self.quote(cmd, Quoting::Shell);
        if self.ignore_failure {
            format!("{cmd} || true\n")
        } else {
            format!("{cmd}\n")
        }
    }
}

/// Strips systemd's prefixes (`-`, `@`, `:`, `+`, `!`, `!!`) from an `Exec*=` value.
///
/// `@` makes the second word argv[0], which dinit cannot set, so that word is
/// dropped. `+`, `!` and `!!` (run with full privileges) cannot be expressed
/// either. Both are reported in `warnings`; `$` escaping for `:` is left to the
/// caller because it depends on where the command is written.
fn clean_exec(directive: &str, raw: &str, warnings: &mut Vec<Warning>) -> (ExecPrefix, String) {
    let mut prefix = ExecPrefix::default();
    let mut argv0 = false;
    let mut privileged = false;
    let mut rest = raw.trim();
    loop {
        match rest.as_bytes().first() {
            Some(b'-') => prefix.ignore_failure = true,
            Some(b':') => prefix.no_env_expansion = true,
            Some(b'@') => argv0 = true,
            Some(b'+' | b'!') => privileged = true,
            _ => break,
        }
        rest = &rest[1..];
    }
    let rest = rest.trim_start();

    let command = if argv0 {
        let mut words = rest.split_whitespace();
        let program = words.next().unwrap_or_default();
        let dropped = words.next().unwrap_or_default();
        warnings.push(Warning {
            directive: directive.into(),
            message: format!("'@' prefix: dinit cannot set argv[0] — '{dropped}' dropped"),
            severity: Severity::Warn,
        });
        std::iter::once(program)
            .chain(words)
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        rest.to_string()
    };

    if privileged {
        warnings.push(Warning {
            directive: directive.into(),
            message: "'+'/'!' prefix (full privileges) not supported — the command runs as the run-as user".into(),
            severity: Severity::Warn,
        });
    }

    (prefix, command)
}

/// Parses a systemd time span such as `3`, `500ms` or `1min 30s` into seconds.
///
/// A bare number is seconds, as in systemd. Returns `None` for anything else,
/// including `infinity`, which dinit's `restart-delay` cannot express.
fn parse_timespan_secs(span: &str) -> Option<f64> {
    let mut rest = span.trim();
    if rest.is_empty() {
        return None;
    }
    let mut total = 0.0;
    while !rest.is_empty() {
        let num_len = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        let value: f64 = rest[..num_len].parse().ok()?;
        rest = rest[num_len..].trim_start();
        let unit_len = rest
            .find(|c: char| !c.is_alphabetic())
            .unwrap_or(rest.len());
        // Sub-second units divide rather than multiply so 500ms is exactly 0.5.
        let (mul, div) = match &rest[..unit_len] {
            "" | "s" | "sec" | "second" | "seconds" => (1.0, 1.0),
            "ms" | "msec" => (1.0, 1_000.0),
            "us" | "usec" | "\u{b5}s" | "\u{3bc}s" => (1.0, 1_000_000.0),
            "ns" | "nsec" => (1.0, 1_000_000_000.0),
            "m" | "min" | "minute" | "minutes" => (60.0, 1.0),
            "h" | "hr" | "hour" | "hours" => (3_600.0, 1.0),
            "d" | "day" | "days" => (86_400.0, 1.0),
            "w" | "week" | "weeks" => (604_800.0, 1.0),
            // systemd's month and year are 30.44 and 365.25 days
            "M" | "month" | "months" => (2_629_800.0, 1.0),
            "y" | "year" | "years" => (31_557_600.0, 1.0),
            _ => return None,
        };
        total += value * mul / div;
        rest = rest[unit_len..].trim_start();
    }
    Some(total)
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
    source_path: &Path,
    config: &Config,
    warnings: &mut Vec<Warning>,
) -> (Option<DinitService>, Option<String>) {
    let pre_cmds = unit.get_all("Service", "ExecStartPre");
    if pre_cmds.is_empty() {
        return (None, None);
    }

    let (command, script) = exec_hook_command(
        "ExecStartPre",
        &pre_cmds,
        config,
        service_name,
        "pre",
        warnings,
    );

    let pre_service = DinitService {
        name: format!("{}-pre", service_name),
        source_path: source_path.to_path_buf(),
        service_type: DinitType::Scripted,
        command: Some(command),
        stop_command: None,
        user: unit.get("Service", "User").map(|s| s.to_string()),
        working_dir: unit.get("Service", "WorkingDirectory").map(PathBuf::from),
        env_files: Vec::new(),
        pid_file: None,
        restart: RestartPolicy::Never,
        smooth_recovery: false,
        restart_delay: None,
        depends_on: Vec::new(),
        waits_for: Vec::new(),
        after: Vec::new(),
        before: Vec::new(),
        logfile: None,
    };

    (Some(pre_service), script)
}

fn convert_exec_post(
    unit: &SystemdUnit,
    service_name: &str,
    source_path: &Path,
    config: &Config,
    warnings: &mut Vec<Warning>,
) -> (Option<DinitService>, Option<String>) {
    let post_cmds = unit.get_all("Service", "ExecStartPost");
    if post_cmds.is_empty() {
        return (None, None);
    }

    let (command, script) = exec_hook_command(
        "ExecStartPost",
        &post_cmds,
        config,
        service_name,
        "post",
        warnings,
    );

    let post_service = DinitService {
        name: format!("{}-post", service_name),
        source_path: source_path.to_path_buf(),
        service_type: DinitType::Scripted,
        command: Some(command),
        stop_command: None,
        user: unit.get("Service", "User").map(|s| s.to_string()),
        working_dir: unit.get("Service", "WorkingDirectory").map(PathBuf::from),
        env_files: Vec::new(),
        pid_file: None,
        restart: RestartPolicy::Never,
        smooth_recovery: false,
        restart_delay: None,
        depends_on: Vec::new(),
        waits_for: vec![service_name.to_string()],
        after: Vec::new(),
        before: Vec::new(),
        logfile: None,
    };

    (Some(post_service), script)
}

fn convert_stop_post(
    unit: &SystemdUnit,
    stop_command: Option<(ExecPrefix, String)>,
    service_name: &str,
    config: &Config,
    warnings: &mut Vec<Warning>,
) -> (Option<String>, Option<String>) {
    let stop_post_cmds = unit.get_all("Service", "ExecStopPost");
    if stop_post_cmds.is_empty() {
        let command = stop_command.map(|(prefix, cmd)| {
            if prefix.ignore_failure {
                warnings.push(Warning {
                    directive: "ExecStop".into(),
                    message: "'-' prefix dropped — dinit reports a failing stop command".into(),
                    severity: Severity::Info,
                });
            }
            prefix.quote(&cmd, Quoting::Dinit)
        });
        return (command, None);
    }

    match stop_command {
        Some((stop_prefix, stop_cmd)) => {
            let mut script = String::from("#!/bin/sh\nset -e\n");
            script.push_str(&stop_prefix.script_line(&stop_cmd));
            for raw in &stop_post_cmds {
                let (prefix, cmd) = clean_exec("ExecStopPost", raw, warnings);
                script.push_str(&prefix.script_line(&cmd));
            }
            let wrapper_cmd = build_script_command(config, service_name, "stop");
            (Some(wrapper_cmd), Some(script))
        }
        None => {
            warnings.push(Warning {
                directive: "ExecStopPost".into(),
                message:
                    "ExecStopPost= without ExecStop= skipped — dinit handles stop signals natively"
                        .into(),
                severity: Severity::Warn,
            });
            (None, None)
        }
    }
}

fn warn_out_of_scope(unit: &SystemdUnit, warnings: &mut Vec<Warning>) {
    // [Service] directives that are out of scope
    const SANDBOXING: &[&str] = &[
        "ProtectSystem",
        "ProtectHome",
        "PrivateTmp",
        "PrivateDevices",
        "PrivateNetwork",
        "ProtectKernelTunables",
        "ProtectKernelModules",
        "ProtectControlGroups",
        "NoNewPrivileges",
        "ReadOnlyPaths",
        "ReadWritePaths",
        "InaccessiblePaths",
        "ProtectHostname",
        "LockPersonality",
        "MemoryDenyWriteExecute",
        "RestrictRealtime",
        "RestrictSUIDSGID",
        "RestrictNamespaces",
        "SystemCallFilter",
        "SystemCallArchitectures",
        "CapabilityBoundingSet",
        "AmbientCapabilities",
        "SecureBits",
        "ProtectClock",
        "ProtectKernelLogs",
        "IPAddressDeny",
        "RestrictAddressFamilies",
        "PrivateUsers",
        // DynamicUser creates an ephemeral unprivileged UID at runtime; dinit
        // has no equivalent so the service runs as root instead.
        "DynamicUser",
        "SupplementaryGroups",
    ];
    const CGROUP: &[&str] = &[
        "Slice",
        "CPUQuota",
        "MemoryMax",
        "MemoryHigh",
        "MemoryLow",
        "IOWeight",
        "IODeviceWeight",
        "TasksMax",
        "Delegate",
    ];

    // [Unit] directives that are out of scope
    const CONDITIONALS: &[&str] = &[
        "ConditionPathExists",
        "ConditionPathIsDirectory",
        "ConditionFileNotEmpty",
        "ConditionDirectoryNotEmpty",
        "ConditionKernelCommandLine",
        "ConditionVirtualization",
        "ConditionArchitecture",
        "ConditionSecurity",
        "AssertPathExists",
    ];

    // [Socket] directives that are out of scope
    const SOCKET: &[&str] = &[
        "ListenStream",
        "ListenDatagram",
        "ListenSequentialPacket",
        "Accept",
    ];

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

/// Builds the command for a service's `ExecStartPre=` or `ExecStartPost=` lines.
///
/// A single line that must succeed runs directly as the dinit command; anything
/// else becomes a `/bin/sh` script in which `-` lines may fail.
fn exec_hook_command(
    directive: &str,
    cmds: &[&str],
    config: &Config,
    service_name: &str,
    suffix: &str,
    warnings: &mut Vec<Warning>,
) -> (String, Option<String>) {
    let cleaned: Vec<(ExecPrefix, String)> = cmds
        .iter()
        .map(|raw| clean_exec(directive, raw, warnings))
        .collect();

    if let [(prefix, cmd)] = cleaned.as_slice()
        && !prefix.ignore_failure
    {
        return (prefix.quote(cmd, Quoting::Dinit), None);
    }

    let mut script = String::from("#!/bin/sh\nset -e\n");
    for (prefix, cmd) in &cleaned {
        script.push_str(&prefix.script_line(cmd));
    }
    (
        build_script_command(config, service_name, suffix),
        Some(script),
    )
}
