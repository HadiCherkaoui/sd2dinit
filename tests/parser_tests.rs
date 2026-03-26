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
