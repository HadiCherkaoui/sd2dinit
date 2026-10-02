// SPDX-FileCopyrightText: Hadi Cherkaoui <contact@hide.cherkaoui.ch>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! systemd `Exec*=` command lines, split into words once and rendered for
//! dinit or for a generated `/bin/sh` script.
//!
//! systemd unquotes a command line itself and runs the resulting argv without
//! a shell, so every word is re-quoted for its destination: dinit only knows
//! double quotes, and sh would otherwise expand `*`, `~`, `;` and friends.

use std::iter::Peekable;
use std::path::Path;
use std::str::Chars;

use crate::model::{Severity, Warning};

/// systemd's special prefixes on `Exec*=` values.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ExecPrefix {
    /// `-`: a non-zero exit is not a failure.
    pub(crate) ignore_failure: bool,
    /// `:`: no `$VAR` expansion.
    pub(crate) no_env_expansion: bool,
}

/// One `Exec*=` line, split into words as systemd splits it.
#[derive(Debug, Clone)]
pub(crate) struct ExecLine {
    pub(crate) prefix: ExecPrefix,
    /// Unquoted and unescaped, but before `$VAR` expansion.
    words: Vec<String>,
}

/// dinit's `term-signal` names, from `dinitctl signal --list`.
const DINIT_SIGNALS: &[&str] = &[
    "HUP", "INT", "QUIT", "KILL", "USR1", "USR2", "TERM", "CONT", "STOP",
];

impl ExecLine {
    /// Parses an `Exec*=` value, reporting what dinit cannot express in `warnings`.
    ///
    /// `@` makes the second word argv[0], which dinit cannot set, so that word
    /// is dropped. `+`, `!` and `|` cannot be expressed either.
    ///
    /// Returns `None`, with a warning, when the line has an unterminated quote
    /// or no command after its prefixes.
    pub(crate) fn parse(directive: &str, raw: &str, warnings: &mut Vec<Warning>) -> Option<Self> {
        let mut prefix = ExecPrefix::default();
        let (mut argv0, mut privileged, mut shell) = (false, false, false);
        let mut rest = raw.trim_start();
        loop {
            match rest.chars().next() {
                Some('-') => prefix.ignore_failure = true,
                Some(':') => prefix.no_env_expansion = true,
                Some('@') => argv0 = true,
                Some('+' | '!') => privileged = true,
                Some('|') => shell = true,
                _ => break,
            }
            rest = &rest[1..];
        }

        let mut warn = |message: String| {
            warnings.push(Warning {
                directive: directive.into(),
                message,
                severity: Severity::Warn,
            });
        };
        let Some(mut words) = split_words(rest) else {
            warn(format!("{raw}: unterminated quote — line dropped"));
            return None;
        };
        if argv0 && words.len() > 1 {
            let dropped = words.remove(1);
            warn(format!(
                "'@' prefix: dinit cannot set argv[0] — '{dropped}' dropped"
            ));
        }
        if privileged {
            warn("'+'/'!' prefix (full privileges) not supported — the command runs as the run-as user".into());
        }
        if shell {
            warn("'|' prefix (run through the user's shell) not supported — the command runs directly".into());
        }
        if !prefix.no_env_expansion {
            // systemd drops a word that is `$` plus something that cannot be a name
            words.retain(|word| {
                let bad = word.starts_with('$')
                    && !word[1..].starts_with(['{', '$'])
                    && !is_env_name(&word[1..]);
                if bad {
                    warn(format!(
                        "'{word}' is not a variable name — dropped, as systemd does"
                    ));
                }
                !bad
            });
        }
        if words.is_empty() {
            warn(format!(
                "{raw}: no command after the prefixes — line dropped"
            ));
            return None;
        }
        Some(Self { prefix, words })
    }

    /// Returns `true` if the line reads `$MAINPID`, which dinit never sets.
    pub(crate) fn uses_mainpid(&self) -> bool {
        self.words
            .iter()
            .any(|w| w.contains("$MAINPID") || w.contains("${MAINPID}"))
    }

    /// Returns the signal of a `kill [-s SIG | -SIG] $MAINPID` line.
    ///
    /// Such a line only signals the main process, which is what dinit does by
    /// itself with `term-signal` when no stop command is set.
    pub(crate) fn mainpid_kill_signal(&self) -> Option<&'static str> {
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
            [] => "TERM",
            [flag, signal] if flag == "-s" || flag == "--signal" => signal.as_str(),
            [flag] => flag.strip_prefix('-')?,
            _ => return None,
        };
        let signal = signal.to_ascii_uppercase();
        let signal = signal.strip_prefix("SIG").unwrap_or(&signal);
        // Only the numbers POSIX fixes; the rest differ between architectures.
        let signal = match signal {
            "1" => "HUP",
            "2" => "INT",
            "3" => "QUIT",
            "9" => "KILL",
            "15" => "TERM",
            name => name,
        };
        DINIT_SIGNALS.iter().copied().find(|&s| s == signal)
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

/// Writes `value` as one literal dinit word, with no variable expansion.
pub(crate) fn dinit_literal(value: &str) -> String {
    dinit_word(value, false, &mut false)
}

/// Splits `line` into words by systemd's quoting rules (systemd.syntax(7)).
///
/// Returns `None` when a quote is left open.
fn split_words(line: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        if chars.peek().is_none() {
            return Some(words);
        }
        let mut word = String::new();
        let mut quote = None;
        while let Some(c) = chars.next() {
            match (quote, c) {
                (_, '\\') => unescape(&mut chars, &mut word),
                (None, '"' | '\'') => quote = Some(c),
                (Some(q), c) if c == q => quote = None,
                (None, c) if c.is_whitespace() => break,
                (_, c) => word.push(c),
            }
        }
        if quote.is_some() {
            return None;
        }
        words.push(word);
    }
}

/// Decodes the C-style escape after a backslash; unknown ones are kept as written.
fn unescape(chars: &mut Peekable<Chars<'_>>, word: &mut String) {
    let Some(c) = chars.next() else {
        word.push('\\');
        return;
    };
    let simple = match c {
        'a' => Some('\x07'),
        'b' => Some('\x08'),
        'f' => Some('\x0c'),
        'n' => Some('\n'),
        'r' => Some('\r'),
        't' => Some('\t'),
        'v' => Some('\x0b'),
        's' => Some(' '),
        '\\' | '"' | '\'' | ';' => Some(c),
        _ => None,
    };
    if let Some(decoded) = simple {
        word.push(decoded);
        return;
    }
    let (radix, len) = match c {
        'x' => (16, 2),
        'u' => (16, 4),
        'U' => (16, 8),
        '0'..='7' => (8, 3),
        _ => {
            word.push('\\');
            word.push(c);
            return;
        }
    };
    let mut digits = String::new();
    if radix == 8 {
        digits.push(c);
    }
    while digits.len() < len
        && let Some(d) = chars.next_if(|d| d.is_digit(radix))
    {
        digits.push(d);
    }
    let decoded = u32::from_str_radix(&digits, radix)
        .ok()
        .and_then(char::from_u32)
        .filter(|_| digits.len() == len);
    match decoded {
        Some(decoded) => word.push(decoded),
        None => {
            word.push('\\');
            if radix != 8 {
                word.push(c);
            }
            word.push_str(&digits);
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
    /// The inside of `${…}`, including systemd's `:-` and `:+` forms.
    Var(&'a str),
}

/// Splits `word` into literal characters and `${…}` references.
///
/// Inside a word systemd expands only `${NAME}`; a bare `$NAME` there is literal.
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
            && let Some(end) = after.find('}')
        {
            pieces.push(Piece::Var(&after[..end]));
            rest = &after[end + 1..];
        } else {
            pieces.push(Piece::Char(c));
            rest = &rest[c.len_utf8()..];
        }
    }
    Word::Pieces(pieces)
}

/// dinit splits on unquoted whitespace, treats a word starting with `#` as a
/// comment, and expands `$` even inside double quotes.
fn dinit_word(word: &str, expand: bool, newline: &mut bool) -> String {
    let pieces = match classify(word, expand) {
        Word::Split(name) => return format!("$/{name}"),
        Word::Pieces(pieces) => pieces,
    };
    let mut quote = word.is_empty() || word.starts_with('#');
    let mut out = String::with_capacity(word.len());
    for piece in pieces {
        match piece {
            Piece::Var(inner) => {
                out.push_str("${");
                out.push_str(inner);
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
                quote |= c.is_whitespace();
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
            Piece::Var(inner) => {
                out.push_str("${");
                out.push_str(inner);
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
