//! What is synchronised: a flat set of items, each with a key, a value, the time of its last
//! change and the machine that made it. A deleted item stays as a tombstone, so the deletion
//! reaches the other machines instead of the item coming back.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use ring::digest::{SHA256, digest};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The newest document format this build reads.
pub const DOCUMENT_FORMAT: u32 = 1;

/// Key prefixes, one per kind of item.
pub const SETTING: &str = "setting/";
pub const CONNECTION: &str = "connection/";
pub const PROFILE: &str = "profile/";
pub const PROJECT: &str = "project/";
pub const RAIL_FOLDER: &str = "railfolder/";
pub const SCRIPT: &str = "script/";

/// One synchronised thing: a setting, a connection, a project, a script…
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// Milliseconds since the Unix epoch of the last change.
    pub modified: i64,
    /// The machine that made the last change.
    #[serde(default)]
    pub by: String,
    /// A deletion: the value is gone but the item is remembered.
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub value: Value,
}

impl Item {
    #[must_use]
    pub fn live(value: Value, modified: i64, by: &str) -> Self {
        Self {
            modified,
            by: by.to_owned(),
            deleted: false,
            value,
        }
    }

    #[must_use]
    pub fn tombstone(modified: i64, by: &str) -> Self {
        Self {
            modified,
            by: by.to_owned(),
            deleted: true,
            value: Value::Null,
        }
    }

    /// A hash of what the item says, not of when or by whom: two machines holding the same
    /// content agree, whatever their clocks.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        fingerprint(self.deleted, &self.value)
    }
}

/// See [`Item::fingerprint`]. `serde_json` keeps object keys sorted, so equal values hash equal.
#[must_use]
pub fn fingerprint(deleted: bool, value: &Value) -> String {
    let text = if deleted {
        "deleted".to_owned()
    } else {
        format!("live:{value}")
    };
    let hash = digest(&SHA256, text.as_bytes());
    hash.as_ref().iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// What the encrypted file holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub format: u32,
    /// The machine that wrote the file.
    pub machine: String,
    /// When, in milliseconds since the epoch.
    pub written: i64,
    pub items: BTreeMap<String, Item>,
}

/// What a machine remembers of the last sync of one item: its content and when it changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseEntry {
    pub fingerprint: String,
    pub modified: i64,
    #[serde(default)]
    pub deleted: bool,
    /// The machine that made that change.
    #[serde(default)]
    pub by: String,
}

impl BaseEntry {
    #[must_use]
    pub fn of(item: &Item) -> Self {
        Self {
            fingerprint: item.fingerprint(),
            modified: item.modified,
            deleted: item.deleted,
            by: item.by.clone(),
        }
    }
}

/// Kept on each machine, never synchronised: the state at the last sync, the basis on which
/// a change on both sides is told from a change on one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncState {
    #[serde(default)]
    pub base: BTreeMap<String, BaseEntry>,
    /// When the last sync finished, in milliseconds since the epoch.
    #[serde(default)]
    pub last_sync: Option<i64>,
}

impl SyncState {
    /// The state saved at `path`; a missing or unreadable file reads as a machine that never
    /// synchronised.
    #[must_use]
    pub fn load(path: &std::path::Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Writes the state atomically.
    pub fn save(&self, path: &std::path::Path) -> crate::Result<()> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| crate::SyncError::Invalid(e.to_string()))?;
        crate::file::write_atomic(path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_fingerprint_ignores_time_and_machine_and_key_order() {
        let a = Item::live(json!({"a": 1, "b": [1, 2]}), 5, "x");
        let b = Item::live(json!({"b": [1, 2], "a": 1}), 99, "y");
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_ne!(
            a.fingerprint(),
            Item::live(json!({"a": 2}), 5, "x").fingerprint()
        );
        assert_ne!(a.fingerprint(), Item::tombstone(5, "x").fingerprint());
        assert_eq!(
            Item::tombstone(1, "x").fingerprint(),
            Item::tombstone(2, "y").fingerprint()
        );
    }
}
