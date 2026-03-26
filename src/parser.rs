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
