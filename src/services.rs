// SPDX-FileCopyrightText: Hadi Cherkaoui <contact@hide.cherkaoui.ch>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The set of dinit service names a generated dependency may point at.

use std::collections::HashSet;
use std::path::Path;

/// dinit service names that generated dependencies are allowed to reference.
///
/// dinit refuses to load a service whose `depends-on` or `waits-for` names a
/// service it cannot find, so the converter only emits dependencies on names in
/// this set. Build it with [`KnownServices::scan`] on a real system, or collect
/// it from names in tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownServices {
    names: HashSet<String>,
}

impl KnownServices {
    /// Creates an empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Collects the service description files found in `dirs`.
    ///
    /// Every regular file (or symlink to one) counts as a service named after
    /// the file. Directories that do not exist or cannot be read are skipped:
    /// dinit's default search path includes `/run/dinit.d`, which is usually
    /// absent.
    #[must_use]
    pub fn scan<P: AsRef<Path>>(dirs: &[P]) -> Self {
        let mut known = Self::new();
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                if entry.path().is_file()
                    && let Some(name) = entry.file_name().to_str()
                {
                    known.insert(name);
                }
            }
        }
        known
    }

    /// Adds `name`, e.g. a service that is about to be generated.
    pub fn insert(&mut self, name: impl Into<String>) {
        self.names.insert(name.into());
    }

    /// Returns whether a service called `name` exists.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }
}

impl<S: Into<String>> FromIterator<S> for KnownServices {
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
        let mut known = Self::new();
        known.extend(iter);
        known
    }
}

impl<S: Into<String>> Extend<S> for KnownServices {
    fn extend<I: IntoIterator<Item = S>>(&mut self, iter: I) {
        self.names.extend(iter.into_iter().map(Into::into));
    }
}
