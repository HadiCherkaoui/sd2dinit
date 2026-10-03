// SPDX-FileCopyrightText: Hadi Cherkaoui <contact@hide.cherkaoui.ch>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! systemd `Exec*=` command lines, split into words once and rendered for
//! dinit or for a generated `/bin/sh` script.
//!
//! systemd unquotes a command line itself and runs the resulting argv without
//! a shell, so every word is re-quoted for its destination: dinit only knows
//! double quotes, and sh would otherwise expand `*`, `~`, `;` and friends.

use std::path::Path;

use crate::model::{Severity, Warning};

/// systemd's special prefixes on `Exec*=` values.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ExecPrefix {
    /// `-`: a non-zero exit is not a failure.
    pub(crate) ignore_failure: bool,
    /// `:`: no `$VAR` expansion.
    pub(crate) no_env_expansion: bool,
}

/// One command of an `Exec*=` line, split into words as systemd splits it.
#[derive(Debug, Clone)]
pub(crate) struct ExecLine {
    pub(crate) prefix: ExecPrefix,
    /// Unquoted, unescaped and with specifiers expanded, but before `$VAR` expansion.
    words: Vec<String>,
}

/// A lone `kill … $MAINPID`, which only signals the main process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MainpidKill {
    /// A signal that stops the process, which dinit can send as `term-signal`.
    Stop(&'static str),
    /// A signal that does not stop the process, or options sd2dinit cannot read.
    Other,
}

/// dinit's `term-signal` names, from `dinitctl signal --list`.
const DINIT_SIGNALS: &[&str] = &[
    "HUP", "INT", "QUIT", "KILL", "USR1", "USR2", "TERM", "CONT", "STOP",
];

/// systemd sends SIGTERM after `ExecStop=` whatever it signalled, so only
/// these can stand in for a `kill $MAINPID` without changing the outcome.
const STOP_SIGNALS: &[&str] = &["INT", "QUIT", "TERM", "KILL"];

/// The bytes systemd separates words with.
const SEPARATORS: &[u8] = b" \t\n\r";

impl ExecLine {
    /// Parses an `Exec*=` value into its commands, which a lone `;` separates.
    ///
    /// `specifiers` expands unit specifiers in each word. What dinit cannot
    /// express is reported in `warnings`: `@` makes the second word argv[0],
    /// which dinit cannot set, so that word is dropped, and `+`, `!` and `|`
    /// are ignored. A line with an unterminated quote or invalid UTF-8, and a
    /// command with no executable, are dropped with a warning.
    pub(crate) fn parse(
        directive: &str,
        raw: &str,
        specifiers: &mut dyn FnMut(&str) -> String,
        warnings: &mut Vec<Warning>,
    ) -> Vec<Self> {
        let mut warn = |message: String| {
            warnings.push(Warning {
                directive: directive.into(),
                message,
                severity: Severity::Warn,
            });
        };
        let commands = match tokenize(raw) {
            Ok(commands) => commands,
            Err(problem) => {
                warn(format!("{raw}: {problem} — line dropped"));
                return Vec::new();
            }
        };

        let mut lines = Vec::new();
        for mut words in commands {
            let mut prefix = ExecPrefix::default();
            let (mut argv0, mut privileged, mut shell) = (false, false, false);
            let first = std::mem::take(&mut words[0]);
            words[0] = first
                .trim_start_matches(|c| match c {
                    '-' if !prefix.ignore_failure => {
                        prefix.ignore_failure = true;
                        true
                    }
                    ':' if !prefix.no_env_expansion => {
                        prefix.no_env_expansion = true;
                        true
                    }
                    '@' if !argv0 => {
                        argv0 = true;
                        true
                    }
                    '|' if !shell => {
                        shell = true;
                        true
                    }
                    '+' | '!' => {
                        privileged = true;
                        true
                    }
                    _ => false,
                })
                .to_owned();

            if privileged {
                warn("'+'/'!' prefix (full privileges) not supported — the command runs as the run-as user".into());
            }
            if shell {
                warn("'|' prefix (run through the user's shell) not supported — the command runs directly".into());
            }
            if words[0].is_empty() {
                warn(format!("{raw}: no command after the prefixes — dropped"));
                continue;
            }
            for word in &mut words {
                *word = specifiers(word);
            }
            if argv0 && words.len() > 1 {
                let dropped = words.remove(1);
                warn(format!(
                    "'@' prefix: dinit cannot set argv[0] — '{dropped}' dropped"
                ));
            }
            if !prefix.no_env_expansion {
                // systemd drops an argument that is `$` plus something that cannot be a name
                let args = words.split_off(1);
                words.extend(args.into_iter().filter(|word| {
                    let bad = word.starts_with('$')
                        && !word[1..].starts_with(['{', '$'])
                        && !is_env_name(&word[1..]);
                    if bad {
                        warn(format!(
                            "'{word}' is not a variable name — dropped, as systemd does"
                        ));
                    }
                    !bad
                }));
            }
            lines.push(Self { prefix, words });
        }
        lines
    }

    /// Returns `true` if the line reads `$MAINPID`, which dinit never sets.
    pub(crate) fn uses_mainpid(&self) -> bool {
        self.words
            .iter()
            .any(|w| w.contains("$MAINPID") || w.contains("${MAINPID}"))
    }

    /// The arguments that are a lone `$NAME`, which systemd splits into words.
    pub(crate) fn split_vars(&self) -> impl Iterator<Item = &str> {
        let expand = !self.prefix.no_env_expansion;
        self.words
            .iter()
            .filter(move |_| expand)
            .filter_map(|word| match classify(word, true) {
                Word::Split(name) => Some(name),
                Word::Pieces(_) => None,
            })
    }

    /// Recognises a `kill [-s SIG | -SIG] $MAINPID` line.
    pub(crate) fn mainpid_kill(&self) -> Option<MainpidKill> {
        if self.prefix.no_env_expansion {
            return None;
        }
        let (program, args) = self.words.split_first()?;
        let (target, options) = args.split_last()?;
        if Path::new(program).file_name()? != "kill"
            || !matches!(target.as_str(), "$MAINPID" | "${MAINPID}")
        {
            return None;
        }
        let signal = match options {
            [] => Some("TERM"),
            [flag, signal] if flag == "-s" || flag == "--signal" => signal_name(signal),
            [flag] => flag.strip_prefix('-').and_then(signal_name),
            _ => None,
        };
        Some(match signal {
            Some(signal) if STOP_SIGNALS.contains(&signal) => MainpidKill::Stop(signal),
            _ => MainpidKill::Other,
        })
    }

    /// Renders the line as a dinit command value.
    ///
    /// A newline inside a word cannot be written in a dinit service file; it
    /// becomes a space and is reported in `warnings`.
    pub(crate) fn to_dinit(&self, directive: &str, warnings: &mut Vec<Warning>) -> String {
        let mut newline = false;
        let line = self
            .words
            .iter()
            .map(|word| dinit_word(word, !self.prefix.no_env_expansion, &mut newline))
            .collect::<Vec<_>>()
            .join(" ");
        if newline {
            warnings.push(Warning {
                directive: directive.into(),
                message:
                    "a newline inside an argument cannot be written for dinit — replaced by a space"
                        .into(),
                severity: Severity::Warn,
            });
        }
        line
    }

    /// Renders the line as a `/bin/sh` command with the same argv.
    pub(crate) fn to_shell(&self) -> String {
        self.words
            .iter()
            .enumerate()
            .map(|(i, word)| shell_word(word, !self.prefix.no_env_expansion, i == 0))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Formats the line for a generated script running under `set -e`.
    pub(crate) fn script_line(&self) -> String {
        if self.prefix.ignore_failure {
            format!("{} || true\n", self.to_shell())
        } else {
            format!("{}\n", self.to_shell())
        }
    }
}

/// Normalizes `QUIT`, `SIGQUIT`, `sigquit` or `3` to a name dinit's `term-signal` accepts.
///
/// Numbers are only read where POSIX fixes them; the rest differ between architectures.
pub(crate) fn signal_name(raw: &str) -> Option<&'static str> {
    let upper = raw.to_ascii_uppercase();
    let name = upper.strip_prefix("SIG").unwrap_or(&upper);
    let name = match name {
        "1" => "HUP",
        "2" => "INT",
        "3" => "QUIT",
        "9" => "KILL",
        "15" => "TERM",
        name => name,
    };
    DINIT_SIGNALS.iter().copied().find(|&s| s == name)
}

/// Writes `value` as one literal dinit word, with no variable expansion.
pub(crate) fn dinit_literal(value: &str) -> String {
    dinit_word(value, false, &mut false)
}

/// Splits `line` into commands of words by systemd's rules.
///
/// Quotes and C escapes follow systemd.syntax(7). As in systemd's
/// `config_parse_exec`, a lone unquoted `;` separates commands and a lone `\;`
/// is a literal `;`.
fn tokenize(line: &str) -> Result<Vec<Vec<String>>, &'static str> {
    let bytes = line.as_bytes();
    let lone = |at: usize| bytes.get(at).is_none_or(|b| SEPARATORS.contains(b));
    let mut commands = Vec::new();
    let mut current = Vec::new();
    let mut i = 0;
    loop {
        while bytes.get(i).is_some_and(|b| SEPARATORS.contains(b)) {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b';' && lone(i + 1) {
            commands.push(std::mem::take(&mut current));
            i += 1;
            continue;
        }
        if bytes[i..].starts_with(b"\\;") && lone(i + 2) {
            current.push(";".to_owned());
            i += 2;
            continue;
        }
        let mut word = Vec::new();
        let mut quote = None;
        while let Some(&b) = bytes.get(i) {
            i += 1;
            match (quote, b) {
                (_, b'\\') => i = unescape(bytes, i, &mut word),
                (None, b'"' | b'\'') => quote = Some(b),
                (Some(q), b) if b == q => quote = None,
                (None, b) if SEPARATORS.contains(&b) => break,
                (_, b) => word.push(b),
            }
        }
        if quote.is_some() {
            return Err("unterminated quote");
        }
        current.push(String::from_utf8(word).map_err(|_| "escape that is not UTF-8")?);
    }
    commands.push(current);
    commands.retain(|command| !command.is_empty());
    Ok(commands)
}

/// Decodes the C escape whose backslash precedes `bytes[i]`, as systemd's
/// `cunescape_one` does, and returns the index after it.
///
/// Unknown escapes, and ones that would decode to NUL, are kept as written.
fn unescape(bytes: &[u8], i: usize, word: &mut Vec<u8>) -> usize {
    let Some(&c) = bytes.get(i) else {
        word.push(b'\\');
        return i;
    };
    let simple = match c {
        b'a' => Some(0x07),
        b'b' => Some(0x08),
        b'f' => Some(0x0c),
        b'n' => Some(b'\n'),
        b'r' => Some(b'\r'),
        b't' => Some(b'\t'),
        b'v' => Some(0x0b),
        b's' => Some(b' '),
        b'\\' | b'"' | b'\'' => Some(c),
        _ => None,
    };
    if let Some(decoded) = simple {
        word.push(decoded);
        return i + 1;
    }
    let (radix, len, start) = match c {
        b'x' => (16, 2, i + 1),
        b'u' => (16, 4, i + 1),
        b'U' => (16, 8, i + 1),
        b'0'..=b'7' => (8, 3, i),
        _ => {
            word.extend([b'\\', c]);
            return i + 1;
        }
    };
    let value = bytes
        .get(start..start + len)
        .filter(|digits| digits.iter().all(|&d| char::from(d).is_digit(radix)))
        .and_then(|digits| std::str::from_utf8(digits).ok())
        .and_then(|digits| u32::from_str_radix(digits, radix).ok())
        .filter(|&value| value != 0);
    // \x and octal escapes are raw bytes; \u and \U are code points
    let decoded = match (c, value) {
        (b'u' | b'U', Some(value)) => char::from_u32(value).map(|ch| ch.to_string().into_bytes()),
        (_, Some(value)) => u8::try_from(value).ok().map(|byte| vec![byte]),
        (_, None) => None,
    };
    match decoded {
        Some(decoded) => {
            word.extend(decoded);
            start + len
        }
        None => {
            word.extend([b'\\', c]);
            i + 1
        }
    }
}

fn is_env_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A word as systemd's `$VAR` expansion sees it.
enum Word<'a> {
    /// `$NAME` alone: the value split at whitespace into zero or more words.
    Split(&'a str),
    /// Everything else, which always stays exactly one word.
    Pieces(Vec<Piece<'a>>),
}

enum Piece<'a> {
    Char(char),
    /// A `${NAME}` reference.
    Var(&'a str),
}

/// Splits `word` into literal characters and `${NAME}` references.
///
/// Inside a word systemd expands only `${NAME}`: a bare `$NAME` and the
/// `${NAME:-…}` forms stay literal, and `${…}` around anything that is not a
/// name expands to nothing.
fn classify(word: &str, expand: bool) -> Word<'_> {
    if !expand {
        return Word::Pieces(word.chars().map(Piece::Char).collect());
    }
    if let Some(name) = word.strip_prefix('$')
        && is_env_name(name)
    {
        return Word::Split(name);
    }
    let mut pieces = Vec::new();
    let mut rest = word;
    while let Some(c) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("$$") {
            pieces.push(Piece::Char('$'));
            rest = after;
        } else if let Some(after) = rest.strip_prefix("${")
            && let Some(end) = after.find(['}', ':'])
            && after.as_bytes()[end] == b'}'
        {
            let name = &after[..end];
            if is_env_name(name) {
                pieces.push(Piece::Var(name));
            }
            rest = &after[end + 1..];
        } else {
            pieces.push(Piece::Char(c));
            rest = &rest[c.len_utf8()..];
        }
    }
    Word::Pieces(pieces)
}

/// dinit splits on unquoted C `isspace` characters, rejects an unquoted `#`
/// that does not start a comment, and expands `$` even inside double quotes.
fn dinit_word(word: &str, expand: bool, newline: &mut bool) -> String {
    let pieces = match classify(word, expand) {
        Word::Split(name) => return format!("$/{name}"),
        Word::Pieces(pieces) => pieces,
    };
    let mut quote = word.is_empty() || word.contains('#');
    let mut out = String::with_capacity(word.len());
    for piece in pieces {
        match piece {
            Piece::Var(name) => {
                out.push_str("${");
                out.push_str(name);
                out.push('}');
            }
            Piece::Char('$') => out.push_str("$$"),
            Piece::Char(c @ ('\\' | '"')) => {
                out.push('\\');
                out.push(c);
            }
            Piece::Char('\n' | '\r') => {
                *newline = true;
                quote = true;
                out.push(' ');
            }
            Piece::Char(c) => {
                quote |= matches!(c, ' ' | '\t' | '\x0b' | '\x0c');
                out.push(c);
            }
        }
    }
    if quote { format!("\"{out}\"") } else { out }
}

/// Characters sh gives no meaning to; `=` only matters in the first word.
fn is_shell_safe(c: char, first: bool) -> bool {
    c.is_ascii_alphanumeric() || "_-./:@%+,".contains(c) || (c == '=' && !first)
}

fn shell_word(word: &str, expand: bool, first: bool) -> String {
    let pieces = match classify(word, expand) {
        // Unquoted, so sh splits it like systemd; generated scripts run with `set -f`.
        Word::Split(name) => return format!("${{{name}}}"),
        Word::Pieces(pieces) => pieces,
    };
    let literal: Option<String> = pieces
        .iter()
        .map(|p| match p {
            Piece::Char(c) => Some(*c),
            Piece::Var(_) => None,
        })
        .collect();
    if let Some(text) = literal {
        if !text.is_empty() && text.chars().all(|c| is_shell_safe(c, first)) {
            return text;
        }
        return format!("'{}'", text.replace('\'', "'\\''"));
    }
    let mut out = String::from("\"");
    for piece in pieces {
        match piece {
            Piece::Var(name) => {
                out.push_str("${");
                out.push_str(name);
                out.push('}');
            }
            Piece::Char(c @ ('$' | '`' | '"' | '\\')) => {
                out.push('\\');
                out.push(c);
            }
            Piece::Char(c) => out.push(c),
        }
    }
    out.push('"');
    out
}
