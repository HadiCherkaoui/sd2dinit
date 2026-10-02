// SPDX-FileCopyrightText: Hadi Cherkaoui <contact@hide.cherkaoui.ch>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashSet;
use std::path::PathBuf;

use crate::command::{ExecLine, dinit_literal};
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

    let mut starts: Vec<ExecLine> = unit
        .get_all("Service", "ExecStart")
        .into_iter()
        .filter_map(|raw| clean_exec("ExecStart", raw, &name, &mut warnings))
        .collect();
    if starts.is_empty() {
        return Err(ConvertError::NoExecStart { unit: name });
    }
    if starts.len() > 1 && service_type != DinitType::Scripted {
        warnings.push(Warning {
            directive: "ExecStart".into(),
            message: "several ExecStart= lines are only valid for Type=oneshot — using the last"
                .into(),
            severity: Severity::Warn,
        });
        starts.drain(..starts.len() - 1);
    }
    // A scripted service can honour '-' through its script; a process cannot.
    let (command, start_script) = match starts.as_slice() {
        [start] if !(start.prefix.ignore_failure && service_type == DinitType::Scripted) => {
            if start.prefix.ignore_failure {
                warnings.push(Warning {
                    directive: "ExecStart".into(),
                    message: "'-' prefix dropped — dinit treats a failing exit as a failure".into(),
                    severity: Severity::Info,
                });
            }
            (start.to_dinit("ExecStart", &mut warnings), None)
        }
        _ => (
            build_script_command(config, &name, "start"),
            Some(sequential_script(&starts)),
        ),
    };

    // User and Group
    let user = unit.get("Service", "User").map(str::to_owned);
    if let Some(user) = user.as_deref()
        && user.bytes().all(|b| b.is_ascii_digit())
    {
        warnings.push(Warning {
            directive: "User".into(),
            message: format!(
                "User={user} is numeric — dinit then keeps its own group and drops supplementary groups; use a user name"
            ),
            severity: Severity::Warn,
        });
    }
    match (user.as_deref(), unit.get("Service", "Group")) {
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

    let (pre_service, pre_script) =
        match exec_hook(unit, "ExecStartPre", &name, "pre", config, &mut warnings) {
            Some((command, script)) => {
                // systemd orders ExecStartPre= after the unit's dependencies too
                let mut pre = hook_service(unit, format!("{name}-pre"), command, &env_files);
                pre.depends_on = deps.depends_on.clone();
                pre.waits_for = deps.waits_for.clone();
                pre.after = deps.after.clone();
                (Some(pre), script)
            }
            None => (None, None),
        };
    let (post_service, post_script) = match exec_hook(
        unit,
        "ExecStartPost",
        &name,
        "post",
        config,
        &mut warnings,
    ) {
        Some((command, script)) => {
            warnings.push(Warning {
                directive: "ExecStartPost".into(),
                message: format!(
                    "runs as {name}-post, which dinit has no hook to start — enable {name}-post as well"
                ),
                severity: Severity::Warn,
            });
            let mut post = hook_service(unit, format!("{name}-post"), command, &env_files);
            post.waits_for = vec![name.clone()];
            (Some(post), script)
        }
        None => (None, None),
    };

    let stop = convert_stop(unit, &service_type, &name, config, &mut warnings);

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
        stop_command: stop.command,
        term_signal: stop.term_signal.map(str::to_owned),
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
        start_script,
        stop_script: stop.script,
        env_file_content,
        warnings,
        should_enable,
    })
}

/// Expands the unit specifiers dinit has an equivalent for and drops the rest.
///
/// Templates are never converted, so `%n`, `%N` and `%p` are fully known.
fn replace_specifiers(
    directive: &str,
    input: &str,
    service_name: &str,
    warnings: &mut Vec<Warning>,
) -> String {
    let mut result = String::with_capacity(input.len());
    let mut warned = HashSet::new();
    let mut chars = input.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            result.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => result.push('%'),
            Some('n') => {
                result.push_str(service_name);
                result.push_str(".service");
            }
            Some('N' | 'p') => result.push_str(service_name),
            Some(other) => {
                if warned.insert(other) {
                    warnings.push(Warning {
                        directive: directive.into(),
                        message: format!("unknown specifier %{other} removed"),
                        severity: Severity::Warn,
                    });
                }
            }
            None => result.push('%'),
        }
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
    struct Resolved<'a> {
        directive: &'static str,
        relation: Relation,
        dep: &'a str,
        name: String,
    }
    let mut resolved = Vec::new();
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
            resolved.push(Resolved {
                directive,
                relation,
                dep,
                name,
            });
        }
    }

    // dinit rejects `before` plus any other relation to the same service as a
    // cycle; the ordering is kept because it is what Before= promised.
    let before: HashSet<&str> = resolved
        .iter()
        .filter(|r| r.relation == Relation::Before)
        .map(|r| r.name.as_str())
        .collect();

    let mut deps = Dependencies::default();
    let mut placed: HashSet<&str> = HashSet::new();
    for r in &resolved {
        if r.relation != Relation::Before && before.contains(r.name.as_str()) {
            let lost = match r.relation {
                Relation::After => "the ordering after it",
                _ => "starting it along with this service",
            };
            warnings.push(Warning {
                directive: r.directive.into(),
                message: format!(
                    "{}={} resolves to {}, which Before= also names — kept as before, dropping {lost}",
                    r.directive, r.dep, r.name
                ),
                severity: Severity::Warn,
            });
            continue;
        }
        if !placed.insert(&r.name) {
            continue;
        }
        let list = match r.relation {
            Relation::DependsOn => &mut deps.depends_on,
            Relation::WaitsFor => &mut deps.waits_for,
            Relation::After => &mut deps.after,
            Relation::Before => &mut deps.before,
        };
        list.push(r.name.clone());
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

/// Parses one `Exec*=` value after expanding its specifiers.
///
/// `$MAINPID` is reported here for every directive but `ExecStop=`, which
/// [`convert_stop`] handles itself.
fn clean_exec(
    directive: &str,
    raw: &str,
    service_name: &str,
    warnings: &mut Vec<Warning>,
) -> Option<ExecLine> {
    let raw = replace_specifiers(directive, raw, service_name, warnings);
    let line = ExecLine::parse(directive, &raw, warnings)?;
    if directive != "ExecStop" && line.uses_mainpid() {
        warn_mainpid(directive, warnings);
    }
    Some(line)
}

fn warn_mainpid(directive: &str, warnings: &mut Vec<Warning>) {
    warnings.push(Warning {
        directive: directive.into(),
        message: "dinit does not set $MAINPID — it expands to nothing".into(),
        severity: Severity::Warn,
    });
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
    let script = config
        .output_dir
        .join(format!("{service_name}-{suffix}.sh"));
    format!("/bin/sh {}", dinit_literal(&script.display().to_string()))
}

/// `-f` because systemd never glob-expands a variable's value.
const SCRIPT_HEADER: &str = "#!/bin/sh\nset -e\nset -f\n";

/// A script running `lines` in order and stopping at the first failure, as systemd does.
fn sequential_script(lines: &[ExecLine]) -> String {
    let mut script = String::from(SCRIPT_HEADER);
    for line in lines {
        script.push_str(&line.script_line());
    }
    script
}

/// Builds the command for a service's `ExecStartPre=` or `ExecStartPost=` lines.
///
/// A single line that must succeed runs directly as the dinit command; anything
/// else becomes a script in which `-` lines may fail. Returns `None` when the
/// directive has no usable lines.
fn exec_hook(
    unit: &SystemdUnit,
    directive: &str,
    service_name: &str,
    suffix: &str,
    config: &Config,
    warnings: &mut Vec<Warning>,
) -> Option<(String, Option<String>)> {
    let lines: Vec<ExecLine> = unit
        .get_all("Service", directive)
        .into_iter()
        .filter_map(|raw| clean_exec(directive, raw, service_name, warnings))
        .collect();
    match lines.as_slice() {
        [] => None,
        [line] if !line.prefix.ignore_failure => Some((line.to_dinit(directive, warnings), None)),
        _ => Some((
            build_script_command(config, service_name, suffix),
            Some(sequential_script(&lines)),
        )),
    }
}

/// A scripted service that runs one of the main service's `Exec*=` hooks.
fn hook_service(
    unit: &SystemdUnit,
    name: String,
    command: String,
    env_files: &[PathBuf],
) -> DinitService {
    DinitService {
        name,
        source_path: unit.source_path.clone(),
        service_type: DinitType::Scripted,
        command: Some(command),
        stop_command: None,
        term_signal: None,
        user: unit.get("Service", "User").map(str::to_owned),
        working_dir: unit.get("Service", "WorkingDirectory").map(PathBuf::from),
        env_files: env_files.to_vec(),
        pid_file: None,
        restart: RestartPolicy::Never,
        smooth_recovery: false,
        restart_delay: None,
        depends_on: Vec::new(),
        waits_for: Vec::new(),
        after: Vec::new(),
        before: Vec::new(),
        logfile: None,
    }
}

#[derive(Debug, Default)]
struct Stop {
    command: Option<String>,
    script: Option<String>,
    term_signal: Option<&'static str>,
}

/// Converts `ExecStop=` and `ExecStopPost=` into a stop command or script.
///
/// While a stop command is set dinit signals nothing itself, so a lone
/// `kill $MAINPID` is replaced by dinit's own `term-signal`.
fn convert_stop(
    unit: &SystemdUnit,
    service_type: &DinitType,
    service_name: &str,
    config: &Config,
    warnings: &mut Vec<Warning>,
) -> Stop {
    let mut stops: Vec<ExecLine> = unit
        .get_all("Service", "ExecStop")
        .into_iter()
        .filter_map(|raw| clean_exec("ExecStop", raw, service_name, warnings))
        .collect();
    let mut stop = Stop::default();
    if let [line] = stops.as_slice()
        && let Some(signal) = line.mainpid_kill_signal()
    {
        warnings.push(Warning {
            directive: "ExecStop".into(),
            message: format!(
                "kill $MAINPID dropped — dinit sends SIG{signal} to the process itself"
            ),
            severity: Severity::Info,
        });
        if signal != "TERM" && *service_type != DinitType::Scripted {
            stop.term_signal = Some(signal);
        }
        stops.clear();
    } else if stops.iter().any(ExecLine::uses_mainpid) {
        warn_mainpid("ExecStop", warnings);
    }

    let posts: Vec<ExecLine> = unit
        .get_all("Service", "ExecStopPost")
        .into_iter()
        .filter_map(|raw| clean_exec("ExecStopPost", raw, service_name, warnings))
        .collect();

    match (stops.as_slice(), posts.is_empty()) {
        ([], true) => {}
        ([], false) => warnings.push(Warning {
            directive: "ExecStopPost".into(),
            message:
                "ExecStopPost= without ExecStop= skipped — dinit handles stop signals natively"
                    .into(),
            severity: Severity::Warn,
        }),
        ([line], true) => {
            if line.prefix.ignore_failure {
                warnings.push(Warning {
                    directive: "ExecStop".into(),
                    message: "'-' prefix dropped — dinit reports a failing stop command".into(),
                    severity: Severity::Info,
                });
            }
            stop.command = Some(line.to_dinit("ExecStop", warnings));
        }
        _ => {
            // ExecStopPost= runs even when ExecStop= fails, so the status waits in rc.
            let mut script = String::from("#!/bin/sh\nset -f\nrc=0\n");
            for (i, line) in stops.iter().enumerate() {
                let guard = if i == 0 { "" } else { "[ \"$rc\" -ne 0 ] || " };
                let on_failure = if line.prefix.ignore_failure {
                    "true"
                } else {
                    "rc=$?"
                };
                script.push_str(&format!("{guard}{} || {on_failure}\n", line.to_shell()));
            }
            if !posts.is_empty() {
                script.push_str("set -e\n");
                for line in &posts {
                    script.push_str(&line.script_line());
                }
            }
            script.push_str("exit \"$rc\"\n");
            stop.command = Some(build_script_command(config, service_name, "stop"));
            stop.script = Some(script);
        }
    }
    stop
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
