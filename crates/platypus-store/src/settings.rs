//! User preferences in a human-editable `settings.toml`.
//!
//! Keys are dotted paths (`editor.font_size`) stored as nested tables, so every feature
//! owns a section. The store does not know the keys: their types and defaults are declared
//! by the features that read them (`apps/desktop/src/settings.ts` for the UI, [`get`] for
//! Rust), and a value that no longer matches its declaration falls back to the default.
//!
//! [`get`]: SettingsStore::get

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use crate::{Result, StoreError};

const FILE_VERSION: i64 = 1;
const VERSION_KEY: &str = "version";

#[derive(Debug)]
pub struct SettingsStore {
    path: PathBuf,
    /// Everything but the `version` key.
    values: toml::Table,
}

impl SettingsStore {
    /// Loads `path`, or starts empty if it does not exist yet.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let mut values = match std::fs::read_to_string(&path) {
            Ok(text) => text
                .trim_start_matches('\u{feff}')
                .parse::<toml::Table>()
                .map_err(|e| StoreError::Format {
                    path: path.clone(),
                    message: e.to_string(),
                })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
            Err(source) => return Err(StoreError::Io { path, source }),
        };
        if let Some(version) = values.remove(VERSION_KEY)
            && version.as_integer().is_none_or(|v| v > FILE_VERSION)
        {
            return Err(StoreError::Format {
                path,
                message: format!("written by a newer PlatypusDB (version {version})"),
            });
        }
        Ok(Self { path, values })
    }

    /// All values, as nested tables.
    #[must_use]
    pub fn all(&self) -> &toml::Table {
        &self.values
    }

    /// The value at `key` as `T`; `None` when missing or of another type.
    #[must_use]
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let (sections, name) = split_key(key);
        let mut table = &self.values;
        for section in sections {
            table = table.get(section)?.as_table()?;
        }
        table.get(name)?.clone().try_into().ok()
    }

    /// Sets `key`, or removes it with `None` so the default applies again.
    pub fn set(&mut self, key: &str, value: Option<toml::Value>) -> Result<()> {
        let (sections, name) = split_key(key);
        if name.is_empty() || sections.iter().any(|s| s.is_empty()) || key == VERSION_KEY {
            return Err(StoreError::Invalid(format!("invalid setting key `{key}`")));
        }
        let mut table = &mut self.values;
        for section in sections {
            let entry = table
                .entry(section)
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            // A plain value where a section is needed is replaced by the section.
            if !entry.is_table() {
                *entry = toml::Value::Table(toml::Table::new());
            }
            let toml::Value::Table(next) = entry else {
                unreachable!("replaced by a table above")
            };
            table = next;
        }
        match value {
            Some(value) => {
                table.insert(name.to_owned(), value);
            }
            None => {
                table.remove(name);
            }
        }
        self.write()
    }

    /// Writes atomically: a crash mid-write never leaves a truncated file behind.
    fn write(&self) -> Result<()> {
        let io_error = |source| StoreError::Io {
            path: self.path.clone(),
            source,
        };
        let dir = self.path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir).map_err(io_error)?;
        let mut file = toml::Table::new();
        file.insert(VERSION_KEY.to_owned(), toml::Value::Integer(FILE_VERSION));
        file.extend(self.values.clone());
        let text = toml::to_string_pretty(&file).map_err(|e| StoreError::Format {
            path: self.path.clone(),
            message: e.to_string(),
        })?;
        let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(io_error)?;
        tmp.write_all(text.as_bytes()).map_err(io_error)?;
        crate::atomic::replace(tmp, &self.path).map_err(io_error)?;
        Ok(())
    }
}

/// `"a.b.c"` → `(["a", "b"], "c")`.
fn split_key(key: &str) -> (Vec<&str>, &str) {
    let mut parts: Vec<&str> = key.split('.').collect();
    let name = parts.pop().unwrap_or_default();
    (parts, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        let mut store = SettingsStore::open(&path).unwrap();
        assert_eq!(store.get::<i64>("editor.font_size"), None);

        store
            .set("editor.font_size", Some(toml::Value::Integer(15)))
            .unwrap();
        store
            .set("grid.dates.relative", Some(toml::Value::Boolean(true)))
            .unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[editor]"), "{text}");
        assert!(text.contains("version = 1"), "{text}");

        let mut store = SettingsStore::open(&path).unwrap();
        assert_eq!(store.get::<i64>("editor.font_size"), Some(15));
        assert_eq!(store.get::<bool>("grid.dates.relative"), Some(true));
        assert_eq!(store.get::<String>("editor.font_size"), None, "wrong type");
        assert!(!store.all().contains_key("version"));

        store.set("editor.font_size", None).unwrap();
        assert_eq!(store.get::<i64>("editor.font_size"), None);
    }

    #[test]
    fn rejects_bad_keys_and_newer_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        let mut store = SettingsStore::open(&path).unwrap();
        for key in ["", "a.", ".a", "version"] {
            assert!(
                store.set(key, Some(toml::Value::Boolean(true))).is_err(),
                "{key}"
            );
        }
        std::fs::write(&path, "version = 99\n").unwrap();
        assert!(SettingsStore::open(&path).is_err());
    }
}
