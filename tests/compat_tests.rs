// SPDX-FileCopyrightText: Hadi Cherkaoui <contact@hide.cherkaoui.ch>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Regressions found running converted units under dinit 0.22 on Artix.

use sd2dinit::config::Config;
use sd2dinit::converter::convert;
use sd2dinit::generator::generate;
use sd2dinit::model::{ConversionResult, Severity};
use sd2dinit::parser::SystemdUnit;
use sd2dinit::services::KnownServices;
use std::path::PathBuf;

/// The services an Artix install ships that these units refer to.
fn artix() -> KnownServices {
    [
        "network",
        "network.target",
        "network-online.target",
        "boot",
        "dbus",
        "docker",
    ]
    .into_iter()
    .collect()
}

fn convert_named(input: &str, name: &str, known: &KnownServices) -> ConversionResult {
    let path = PathBuf::from(format!("/usr/lib/systemd/system/{name}.service"));
    let unit = SystemdUnit::parse(input, path).unwrap();
    convert(&unit, &Config::default(), known).unwrap()
}

fn convert_unit(input: &str) -> ConversionResult {
    convert_named(input, "test", &artix())
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

const OLLAMA: &str = "\
[Unit]
Description=Ollama Service
Wants=network-online.target
After=network.target network-online.target

[Service]
ExecStart=/usr/bin/ollama serve
WorkingDirectory=/var/lib/ollama
Environment=\"HOME=/var/lib/ollama\"
Environment=\"OLLAMA_MODELS=/var/lib/ollama\"
User=ollama
Group=ollama
Restart=on-failure
RestartSec=3
RestartPreventExitStatus=1
Type=simple
PrivateTmp=yes
ProtectSystem=full
ProtectHome=yes

[Install]
WantedBy=multi-user.target
";

const COOLERCONTROLD: &str = "\
[Unit]
Description=CoolerControl Daemon
After=network.target lm-sensors.service lm_sensors.service

[Service]
Type=notify
Environment=\"CC_LOG=INFO\"
ExecStart=/usr/bin/coolercontrold
Restart=always
RestartSec=1

[Install]
WantedBy=multi-user.target
";

// --- run-as ---

#[test]
fn run_as_names_only_the_user() {
    let output = generate(&convert_named(OLLAMA, "ollama", &artix()).main_service);
    assert!(output.contains("run-as = ollama\n"), "{output}");
    assert!(!output.contains("ollama:ollama"), "{output}");
}

#[test]
fn group_equal_to_user_needs_no_warning() {
    let result = convert_named(OLLAMA, "ollama", &artix());
    assert!(!result.warnings.iter().any(|w| w.directive == "Group"));
}

#[test]
fn group_different_from_user_is_warned_about() {
    let result = convert_unit("[Service]\nExecStart=/usr/bin/d\nUser=www\nGroup=web\n");
    assert_eq!(result.main_service.user.as_deref(), Some("www"));
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.directive == "Group" && w.severity == Severity::Warn)
    );
}

#[test]
fn group_without_user_writes_no_run_as() {
    let result = convert_unit("[Service]\nExecStart=/usr/bin/d\nGroup=web\n");
    assert!(!generate(&result.main_service).contains("run-as"));
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.directive == "Group" && w.severity == Severity::Warn)
    );
}

// --- dependency resolution ---

#[test]
fn network_target_resolves_to_the_dinit_target_not_the_deprecated_service() {
    let result = convert_unit("[Unit]\nAfter=network.target\n[Service]\nExecStart=/usr/bin/d\n");
    assert_eq!(
        result.main_service.after,
        vec!["network.target".to_string()]
    );
}

#[test]
fn dependencies_on_missing_services_are_dropped() {
    let result = convert_named(COOLERCONTROLD, "coolercontrold", &artix());
    assert_eq!(
        result.main_service.after,
        vec!["network.target".to_string()]
    );
    let output = generate(&result.main_service);
    assert!(
        !output.contains("lm-sensors") && !output.contains("lm_sensors"),
        "{output}"
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.message.contains("lm-sensors.service"))
    );
}

#[test]
fn missing_wanted_service_is_a_warning() {
    let result = convert_unit("[Unit]\nWants=nope.service\n[Service]\nExecStart=/usr/bin/d\n");
    assert!(result.main_service.waits_for.is_empty());
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.directive == "Wants" && w.severity == Severity::Warn)
    );
}

#[test]
fn suffix_is_stripped_when_only_the_bare_name_exists() {
    let result = convert_unit("[Unit]\nRequires=dbus.service\n[Service]\nExecStart=/usr/bin/d\n");
    assert_eq!(result.main_service.depends_on, vec!["dbus".to_string()]);
}

#[test]
fn user_dependency_map_is_honoured_even_for_unknown_names() {
    let mut config = Config::default();
    config
        .dependency_map
        .insert("custom.target".into(), "my-custom".into());
    let unit = SystemdUnit::parse(
        "[Unit]\nAfter=custom.target\n[Service]\nExecStart=/usr/bin/d\n",
        PathBuf::from("/usr/lib/systemd/system/test.service"),
    )
    .unwrap();
    let result = convert(&unit, &config, &KnownServices::new()).unwrap();
    assert_eq!(result.main_service.after, vec!["my-custom".to_string()]);
}

#[test]
fn a_unit_never_depends_on_itself() {
    let unit = "[Unit]\nRequires=docker.socket\nAfter=docker.socket\n[Service]\nExecStart=/usr/bin/dockerd\n";
    let result = convert_named(unit, "docker", &artix());
    assert!(result.main_service.depends_on.is_empty());
    assert!(result.main_service.after.is_empty());
}

// --- dependency kinds ---

#[test]
fn ollama_matches_systemd_semantics() {
    let output = generate(&convert_named(OLLAMA, "ollama", &artix()).main_service);
    assert!(
        output.contains("waits-for = network-online.target\n"),
        "{output}"
    );
    assert!(output.contains("after = network.target\n"), "{output}");
    assert!(!output.contains("depends-ms"), "{output}");
    assert_eq!(count(&output, "network-online.target"), 1, "{output}");
    assert!(output.contains("restart = on-failure\n"), "{output}");
}

#[test]
fn before_maps_to_dinit_before() {
    let result = convert_unit("[Unit]\nBefore=network.target\n[Service]\nExecStart=/usr/bin/d\n");
    assert_eq!(
        result.main_service.before,
        vec!["network.target".to_string()]
    );
    assert!(generate(&result.main_service).contains("before = network.target\n"));
}

#[test]
fn strongest_relation_wins() {
    let result = convert_unit(
        "[Unit]\nRequires=dbus.service\nWants=dbus.service\nAfter=dbus.service\n[Service]\nExecStart=/usr/bin/d\n",
    );
    assert_eq!(result.main_service.depends_on, vec!["dbus".to_string()]);
    assert!(result.main_service.waits_for.is_empty());
    assert!(result.main_service.after.is_empty());
}

#[test]
fn repeated_entries_collapse() {
    let result = convert_unit(
        "[Unit]\nAfter=network.target\nAfter=network.target\n[Service]\nExecStart=/usr/bin/d\n",
    );
    assert_eq!(
        result.main_service.after,
        vec!["network.target".to_string()]
    );
}

// --- restart ---

#[test]
fn restart_no_is_written_because_dinit_defaults_to_restarting() {
    let output = generate(&convert_unit("[Service]\nExecStart=/usr/bin/d\n").main_service);
    assert!(output.contains("restart = false\n"), "{output}");
}

#[test]
fn restart_sec_understands_systemd_time_spans() {
    for (span, secs) in [
        ("3", 3.0),
        ("500ms", 0.5),
        ("1min", 60.0),
        ("1min 30s", 90.0),
        ("2.5s", 2.5),
    ] {
        let result = convert_unit(&format!(
            "[Service]\nExecStart=/usr/bin/d\nRestart=always\nRestartSec={span}\n"
        ));
        assert_eq!(
            result.main_service.restart_delay,
            Some(secs),
            "RestartSec={span}"
        );
    }
}

#[test]
fn unparseable_restart_sec_is_warned_about() {
    let result = convert_unit("[Service]\nExecStart=/usr/bin/d\nRestart=always\nRestartSec=soon\n");
    assert_eq!(result.main_service.restart_delay, None);
    assert!(result.warnings.iter().any(|w| w.directive == "RestartSec"));
}

// --- ExecStart= prefixes ---

#[test]
fn exec_prefixes_are_not_part_of_the_command() {
    let cases = [
        ("-/usr/bin/foo --x", "/usr/bin/foo --x"),
        ("+/usr/bin/foo", "/usr/bin/foo"),
        ("!!/usr/bin/foo", "/usr/bin/foo"),
        ("@/usr/bin/foo foo-name -a", "/usr/bin/foo -a"),
        (":/usr/bin/echo $HOME", "/usr/bin/echo $$HOME"),
    ];
    for (exec, command) in cases {
        let result = convert_unit(&format!("[Service]\nExecStart={exec}\n"));
        assert_eq!(
            result.main_service.command.as_deref(),
            Some(command),
            "ExecStart={exec}"
        );
    }
}

#[test]
fn privileged_prefix_is_warned_about() {
    let result = convert_unit("[Service]\nExecStart=+/usr/bin/foo\n");
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.directive == "ExecStart" && w.severity == Severity::Warn)
    );
}

#[test]
fn exec_stop_prefixes_are_stripped_too() {
    let result = convert_unit("[Service]\nExecStart=/usr/bin/d\nExecStop=-/usr/bin/d stop\n");
    assert_eq!(
        result.main_service.stop_command.as_deref(),
        Some("/usr/bin/d stop")
    );
}

#[test]
fn exec_type_is_a_plain_process() {
    let result = convert_unit("[Service]\nType=exec\nExecStart=/usr/bin/d\n");
    assert!(!result.warnings.iter().any(|w| w.directive == "Type"));
}

// --- Environment= ---

#[test]
fn one_environment_line_can_hold_several_assignments() {
    let result =
        convert_unit("[Service]\nExecStart=/usr/bin/d\nEnvironment=\"A=1 2\" B=3 'C=x y'\n");
    let env = result.env_file_content.unwrap();
    assert_eq!(env, "A=1 2\nB=3\nC=x y\n");
}
