// SPDX-FileCopyrightText: Hadi Cherkaoui <contact@hide.cherkaoui.ch>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! How `Exec*=` lines are unquoted and expanded as systemd does, then re-quoted
//! for dinit or a generated `/bin/sh` script.

use sd2dinit::config::Config;
use sd2dinit::converter::convert;
use sd2dinit::model::{ConversionResult, Severity};
use sd2dinit::parser::SystemdUnit;
use sd2dinit::services::KnownServices;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn convert_unit(input: &str) -> ConversionResult {
    let path = PathBuf::from("/usr/lib/systemd/system/test.service");
    let unit = SystemdUnit::parse(input, path).unwrap();
    convert(&unit, &Config::default(), &KnownServices::new()).unwrap()
}

fn command(exec_start: &str) -> String {
    convert_unit(&format!("[Service]\nExecStart={exec_start}\n"))
        .main_service
        .command
        .unwrap()
}

#[test]
fn quoted_words_are_requoted_for_dinit() {
    for (systemd, dinit) in [
        // dinit has no single quotes and would split this into three words
        (
            "/bin/sh -c 'echo hi; exit 1'",
            "/bin/sh -c \"echo hi; exit 1\"",
        ),
        ("/bin/echo \"a  b\"", "/bin/echo \"a  b\""),
        (
            "/bin/echo \"say \\\"hi\\\"\"",
            "/bin/echo \"say \\\"hi\\\"\"",
        ),
        ("/bin/echo ''", "/bin/echo \"\""),
        ("/bin/echo '#not-a-comment'", "/bin/echo \"#not-a-comment\""),
        ("/bin/echo a\\sb", "/bin/echo \"a b\""),
        // dinit rejects a # that does not start a comment
        ("/usr/bin/d --color=#fff", "/usr/bin/d \"--color=#fff\""),
        // only space, tab and line breaks separate words
        ("/bin/echo a\u{a0}b", "/bin/echo a\u{a0}b"),
        // \x escapes are bytes; ones that decode to NUL or past 255 stay as written
        (
            "/bin/echo \\xc3\\xa9 \\x00 \\400",
            "/bin/echo é \\\\x00 \\\\400",
        ),
        (
            "/usr/bin/amixer set Master 50% unmute",
            "/usr/bin/amixer set Master 50% unmute",
        ),
        ("/usr/bin/d --limit \"90%\"", "/usr/bin/d --limit 90%"),
    ] {
        assert_eq!(command(systemd), dinit, "ExecStart={systemd}");
    }
}

#[test]
fn variables_expand_with_systemds_word_rules() {
    for (systemd, dinit) in [
        // alone, $VAR splits into words; ${VAR} is always exactly one word
        ("/usr/bin/d $OPTS", "/usr/bin/d $/OPTS"),
        ("/usr/bin/d ${OPTS}", "/usr/bin/d ${OPTS}"),
        ("/usr/bin/d --dir=${DIR}/x", "/usr/bin/d --dir=${DIR}/x"),
        // Exec lines get no ${VAR:-default}, and ${…} around a non-name is empty
        (
            "/usr/bin/d ${DIR:-/var/lib/d}",
            "/usr/bin/d $${DIR:-/var/lib/d}",
        ),
        ("/usr/bin/d \"${A B}x\"", "/usr/bin/d x"),
        // inside a word systemd leaves a bare $VAR alone
        ("/usr/bin/d --dir=$DIR", "/usr/bin/d --dir=$$DIR"),
        ("/usr/bin/d $$HOME", "/usr/bin/d $$HOME"),
        (":/usr/bin/d ${OPTS}", "/usr/bin/d $${OPTS}"),
    ] {
        assert_eq!(command(systemd), dinit, "ExecStart={systemd}");
    }
}

#[test]
fn specifiers_expand_in_every_exec_line() {
    let result = convert_unit(
        "[Service]\nExecStart=/usr/bin/d --name=%N --unit=%n --pct=100%%\nExecStop=/usr/bin/d stop %p\n",
    );
    let svc = &result.main_service;
    assert_eq!(
        svc.command.as_deref(),
        Some("/usr/bin/d --name=test --unit=test.service --pct=100%")
    );
    assert_eq!(svc.stop_command.as_deref(), Some("/usr/bin/d stop test"));
}

#[test]
fn script_lines_keep_systemds_argv() {
    let result = convert_unit(
        "[Service]\nExecStartPre=/bin/sh -c 'echo $$HOME > /tmp/x'\nExecStartPre=-/usr/bin/p --dir=${DIR}/x $ARGS *.conf\nExecStart=/usr/bin/d\n",
    );
    let script = result.pre_script.unwrap();
    assert!(
        script.contains("/bin/sh -c 'echo $HOME > /tmp/x'\n"),
        "{script}"
    );
    assert!(
        script.contains("/usr/bin/p \"--dir=${DIR}/x\" ${ARGS} '*.conf' || true\n"),
        "{script}"
    );
    assert!(script.contains("set -f\n"), "{script}");
}

#[test]
fn colon_lines_in_scripts_pass_dollars_to_the_inner_shell() {
    // the inner sh expands $HOME, as it would under systemd
    let result = convert_unit(
        "[Service]\nExecStart=/usr/bin/d\nExecStop=/usr/bin/d stop\nExecStopPost=:/bin/sh -c 'echo $HOME'\n",
    );
    let script = result.stop_script.unwrap();
    assert!(script.contains("/bin/sh -c 'echo $HOME'\n"), "{script}");
}

#[test]
fn lines_with_only_a_prefix_are_dropped() {
    let result = convert_unit(
        "[Service]\nExecStartPre=-\nExecStart=/usr/bin/d\nExecStop=-\nExecStopPost=@\n",
    );
    assert!(result.pre_service.is_none());
    assert!(result.main_service.stop_command.is_none());
    assert!(result.stop_script.is_none());
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.directive == "ExecStartPre" && w.severity == Severity::Warn)
    );
}

#[test]
fn unterminated_quote_drops_the_line() {
    let unit = SystemdUnit::parse(
        "[Service]\nExecStart=/bin/echo 'oops\n",
        PathBuf::from("/usr/lib/systemd/system/test.service"),
    )
    .unwrap();
    assert!(convert(&unit, &Config::default(), &KnownServices::new()).is_err());
}

#[test]
fn at_prefix_drops_the_argv0_word_after_unquoting() {
    assert_eq!(
        command("@/usr/bin/d 'my daemon' --flag"),
        "/usr/bin/d --flag"
    );
}

#[test]
fn kill_of_mainpid_becomes_dinits_own_signal() {
    for (exec_stop, signal) in [
        ("/bin/kill -s QUIT $MAINPID", Some("QUIT")),
        ("/usr/bin/kill -INT ${MAINPID}", Some("INT")),
        ("kill -9 $MAINPID", Some("KILL")),
        ("/bin/kill $MAINPID", None),
        ("/bin/kill -SIGTERM $MAINPID", None),
    ] {
        let result = convert_unit(&format!(
            "[Service]\nExecStart=/usr/bin/d\nExecStop={exec_stop}\n"
        ));
        let svc = &result.main_service;
        assert_eq!(svc.term_signal.as_deref(), signal, "ExecStop={exec_stop}");
        assert!(svc.stop_command.is_none(), "ExecStop={exec_stop}");
    }
}

#[test]
fn other_uses_of_mainpid_are_reported() {
    let result = convert_unit(
        "[Service]\nExecStart=/usr/bin/d\nExecStartPost=/bin/sh -c 'echo $MAINPID > /run/d.pid'\nExecStop=/usr/bin/d stop --pid=${MAINPID}\n",
    );
    for directive in ["ExecStartPost", "ExecStop"] {
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.directive == directive && w.message.contains("MAINPID")),
            "{directive}"
        );
    }
}

/// Runs a generated stop script whose ExecStop= is `stop` and ExecStopPost= touches a marker.
fn run_stop_script(name: &str, stop: &str) -> (i32, bool) {
    let dir = std::env::temp_dir().join(format!("sd2dinit-stop-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("cleaned");
    let result = convert_unit(&format!(
        "[Service]\nExecStart=/usr/bin/d\n{stop}ExecStopPost=/usr/bin/touch {}\n",
        marker.display()
    ));
    let script = dir.join("stop.sh");
    fs::write(&script, result.stop_script.unwrap()).unwrap();
    let status = Command::new("/bin/sh").arg(&script).status().unwrap();
    let cleaned = marker.exists();
    fs::remove_dir_all(&dir).unwrap();
    (status.code().unwrap(), cleaned)
}

#[test]
fn stop_post_runs_even_when_stop_fails_and_the_failure_is_kept() {
    assert_eq!(run_stop_script("fails", "ExecStop=/bin/false\n"), (1, true));
    assert_eq!(
        run_stop_script("may-fail", "ExecStop=-/bin/false\n"),
        (0, true)
    );
    assert_eq!(
        run_stop_script(
            "second",
            "ExecStop=/bin/false\nExecStop=/usr/bin/touch /nonexistent/dir/x\n"
        ),
        (1, true)
    );
}

#[test]
fn every_exec_stop_line_is_kept() {
    let result =
        convert_unit("[Service]\nExecStart=/usr/bin/d\nExecStop=/usr/bin/a\nExecStop=/usr/bin/b\n");
    let script = result.stop_script.unwrap();
    assert!(script.contains("/usr/bin/a || rc=$?\n"), "{script}");
    assert!(
        script.contains("[ \"$rc\" -ne 0 ] || /usr/bin/b || rc=$?\n"),
        "{script}"
    );
}

#[test]
fn kill_with_a_signal_that_does_not_stop_is_dropped() {
    // systemd follows ExecStop= with SIGTERM; dinit would only send the HUP
    for exec_stop in [
        "/bin/kill -HUP $MAINPID",
        "/bin/kill -n 15 $MAINPID",
        "kill -WINCH $MAINPID",
    ] {
        let result = convert_unit(&format!(
            "[Service]\nExecStart=/usr/bin/d\nExecStop={exec_stop}\n"
        ));
        let svc = &result.main_service;
        assert!(
            svc.term_signal.is_none() && svc.stop_command.is_none(),
            "ExecStop={exec_stop}"
        );
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.directive == "ExecStop" && w.severity == Severity::Warn),
            "ExecStop={exec_stop}"
        );
    }
}

#[test]
fn stop_post_after_a_dropped_kill_says_why_it_is_skipped() {
    let result = convert_unit(
        "[Service]\nExecStart=/usr/bin/d\nExecStop=/bin/kill -s QUIT $MAINPID\nExecStopPost=/bin/rm -f /run/d.sock\n",
    );
    assert_eq!(result.main_service.term_signal.as_deref(), Some("QUIT"));
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.directive == "ExecStopPost" && w.message.contains("stop signal"))
    );
}

#[test]
fn kill_signal_becomes_term_signal() {
    for (kill_signal, term_signal) in [
        ("SIGQUIT", Some("QUIT")),
        ("INT", Some("INT")),
        ("SIGTERM", None),
        ("SIGWINCH", None),
    ] {
        let result = convert_unit(&format!(
            "[Service]\nExecStart=/usr/bin/d\nKillSignal={kill_signal}\n"
        ));
        assert_eq!(
            result.main_service.term_signal.as_deref(),
            term_signal,
            "KillSignal={kill_signal}"
        );
    }
}

#[test]
fn a_lone_semicolon_separates_commands() {
    // a lone \; is a literal argument, and one inside a word is kept as written
    let result = convert_unit(
        r"[Service]
ExecStart=/usr/bin/d
ExecStop=/usr/bin/d stop
ExecStopPost=/bin/rm -f /run/d.sock ; -/bin/rm -f /run/d.pid \; a\;b
",
    );
    let script = result.stop_script.unwrap();
    assert!(script.contains("\n/bin/rm -f /run/d.sock\n"), "{script}");
    assert!(
        script.contains(r"/bin/rm -f /run/d.pid ';' 'a\;b' || true"),
        "{script}"
    );
}

#[test]
fn split_variables_with_quoted_values_are_reported() {
    let result = convert_unit(
        "[Service]\nEnvironment=\"OPTS=--name 'a b' c\" PLAIN=x\nExecStart=/usr/bin/d $OPTS $PLAIN\n",
    );
    let quoted: Vec<_> = result
        .warnings
        .iter()
        .filter(|w| w.message.contains("split at whitespace"))
        .collect();
    assert_eq!(quoted.len(), 1, "{quoted:?}");
    assert!(quoted[0].message.contains("$OPTS"));
}
