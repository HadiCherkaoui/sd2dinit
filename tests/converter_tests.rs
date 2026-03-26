use sd2dinit::converter::convert;
use sd2dinit::config::Config;
use sd2dinit::model::{DinitType, RestartPolicy};
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

// --- Task 7: Restart tests ---

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

// --- Task 7: Dependency tests ---

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

// --- Task 7: Environment tests ---

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
    assert_eq!(result.main_service.env_files.len(), 2);
    assert!(result.main_service.env_files[0].to_str().unwrap().ends_with("test.env"));
    assert_eq!(result.main_service.env_files[1], PathBuf::from("/etc/default/myapp"));
}

// --- Task 7: Pre/Post tests ---

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
    assert!(result.pre_script.is_none());
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
    assert!(result.main_service.stop_command.as_deref().unwrap_or("").contains("-stop.sh"));
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

// --- Task 7: Out-of-scope tests ---

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
