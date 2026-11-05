//! What agents may do, as the user approved it in the app.
//!
//! Agent access lives in `connections.toml`, and whether agents' changes wait for the user's yes
//! (`agents.confirm_writes`) and may skip the `WHERE` (`safety.unfiltered`) in `settings.toml`:
//! files a coding agent with file tools can edit to give itself more. So the app keeps the policy
//! the user approved in `agent-policy.json` beside them, sealed by its SHA-256 in the system
//! keychain, which no file edit reaches. Agents get what the files say, but never more than
//! the sealed policy: a loosening made outside the app waits until the user accepts it there
//! ([`AgentPolicy::loosenings`]), while a tightening applies at once.
//!
//! Without a seal (a new profile, a system without a keychain) the files are taken as they
//! are, and the app seals them as it starts. A seal that no longer matches its file leaves
//! agents the defaults at most (read-only, changes asked about, `WHERE` required) until the
//! user accepts the current settings.
//!
//! What masks values for agents is sealed too: see [`crate::masking_seal`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use platypus_core::Secret;
use platypus_store::{
    AgentAccess, AgentLimits, AgentValues, KeyringSecrets, SavedConnection, SecretStore,
    SettingsStore,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::masking_seal::{MARKS_FILE, MaskedBy, MaskingRules, SealedMarks};

/// The file of the approved policy, in the folder of `settings.toml`.
pub const POLICY_FILE: &str = "agent-policy.json";

/// The keychain service that holds the seals, apart from the saved passwords.
pub const SEAL_SERVICE: &str = "platypusdb-agent-policy";

/// The setting that says whether agents' changes wait for the user's yes.
pub const CONFIRM_WRITES: &str = "agents.confirm_writes";
/// The setting that says what happens to `UPDATE` or `DELETE` without `WHERE`.
pub const UNFILTERED: &str = "safety.unfiltered";

/// What agents may do: each connection's agent access as saved (production and read-only
/// connections cap it further), and the two Safety settings that apply to agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPolicy {
    pub connections: BTreeMap<Uuid, AgentAccess>,
    /// Agents' changes wait for the user's yes in the app.
    pub confirm_writes: bool,
    /// `UPDATE` or `DELETE` without `WHERE` and `TRUNCATE` run without asking.
    pub unfiltered_allowed: bool,
    /// The connections that show agents the values of their sensitive columns; the others mask
    /// them. Not in the files of older versions.
    #[serde(default)]
    pub values_shown: BTreeSet<Uuid>,
    /// The limits of the connections that have any; the others have none beyond the fixed
    /// caps. Not in the files of older versions.
    #[serde(default)]
    pub limits: BTreeMap<Uuid, AgentLimits>,
    /// The rules that tell the sensitive columns. `None` in the files of older versions, until
    /// the app takes the files as the first seal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masking: Option<MaskingRules>,
    /// The SHA-256 of the file of the approved marks ([`MARKS_FILE`]); `None` when there are
    /// none. The files say nothing here: the marks are compared where they are read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marks: Option<String>,
}

impl Default for AgentPolicy {
    /// The app's defaults: a connection not listed gets read-only access at most, and masks.
    fn default() -> Self {
        Self {
            connections: BTreeMap::new(),
            confirm_writes: true,
            unfiltered_allowed: false,
            values_shown: BTreeSet::new(),
            limits: BTreeMap::new(),
            masking: Some(MaskingRules::default()),
            marks: None,
        }
    }
}

/// One way the current policy gives agents more than the approved one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Loosening {
    /// A connection's agent access went up.
    Access {
        id: Uuid,
        name: String,
        from: AgentAccess,
        to: AgentAccess,
    },
    /// Agents' changes no longer wait for the user's yes.
    ConfirmWrites,
    /// Statements without `WHERE` no longer wait.
    Unfiltered,
    /// A connection shows agents the values of its sensitive columns.
    ValuesShown { id: Uuid, name: String },
    /// A connection's limits were raised or removed.
    AgentLimits { id: Uuid, name: String },
    /// A masking rule lost these patterns: removed or narrowed.
    MaskingRules { by: MaskedBy, patterns: Vec<String> },
    /// Sensitive marks on a connection were removed or lifted: how many, and some of the columns.
    SensitiveMarks {
        id: Uuid,
        name: String,
        count: usize,
        columns: Vec<String>,
    },
    /// The seal no longer matches: every value reads masked until the user answers.
    MaskingLost,
}

impl AgentPolicy {
    /// The policy the files say: `connections` as saved, and `settings`.
    #[must_use]
    pub fn current(connections: &[SavedConnection], settings: Option<&SettingsStore>) -> Self {
        Self {
            connections: connections.iter().map(|c| (c.id, c.agent_access)).collect(),
            confirm_writes: settings
                .and_then(|s| s.get::<bool>(CONFIRM_WRITES))
                .unwrap_or(true),
            unfiltered_allowed: settings
                .and_then(|s| s.get::<String>(UNFILTERED))
                .is_some_and(|v| v == "off"),
            values_shown: connections
                .iter()
                .filter(|c| c.agent_values == AgentValues::Shown)
                .map(|c| c.id)
                .collect(),
            limits: connections
                .iter()
                .filter(|c| !c.agent_limits.is_empty())
                .map(|c| (c.id, c.agent_limits))
                .collect(),
            masking: Some(MaskingRules::of(settings)),
            marks: None,
        }
    }

    /// The agent access approved for connection `id`: read-only at most for one not listed.
    #[must_use]
    pub fn access(&self, id: Uuid) -> AgentAccess {
        self.connections
            .get(&id)
            .copied()
            .unwrap_or(AgentAccess::ReadOnly)
    }

    /// What connection `id` does with the values of its sensitive columns: masks them unless
    /// it is listed.
    #[must_use]
    pub fn values(&self, id: Uuid) -> AgentValues {
        if self.values_shown.contains(&id) {
            AgentValues::Shown
        } else {
            AgentValues::Masked
        }
    }

    /// The limits of connection `id`: none beyond the fixed caps unless it is listed.
    #[must_use]
    pub fn limits(&self, id: Uuid) -> AgentLimits {
        self.limits.get(&id).copied().unwrap_or_default()
    }

    /// This policy, never looser than `approved`: the stricter of the two, item by item.
    #[must_use]
    pub fn within(&self, approved: &Self) -> Self {
        Self {
            connections: self
                .connections
                .iter()
                .map(|(id, access)| (*id, (*access).min(approved.access(*id))))
                .collect(),
            confirm_writes: self.confirm_writes || approved.confirm_writes,
            unfiltered_allowed: self.unfiltered_allowed && approved.unfiltered_allowed,
            values_shown: self
                .values_shown
                .intersection(&approved.values_shown)
                .copied()
                .collect(),
            limits: self
                .limits
                .keys()
                .chain(approved.limits.keys())
                .map(|id| (*id, self.limits(*id).stricter(&approved.limits(*id))))
                .filter(|(_, limits)| !limits.is_empty())
                .collect(),
            masking: match (&self.masking, &approved.masking) {
                (Some(files), Some(sealed)) => Some(files.union(sealed)),
                (files, None) => files.clone(),
                (None, sealed) => sealed.clone(),
            },
            marks: approved.marks.clone(),
        }
    }

    /// What this policy gives agents beyond `approved`, with the connections' names from
    /// `connections`.
    #[must_use]
    pub fn loosenings(&self, approved: &Self, connections: &[SavedConnection]) -> Vec<Loosening> {
        let name_of = |id: &Uuid| {
            connections
                .iter()
                .find(|c| c.id == *id)
                .map(|c| c.name.clone())
                .unwrap_or_default()
        };
        let mut found: Vec<Loosening> = self
            .connections
            .iter()
            .filter(|(id, access)| **access > approved.access(**id))
            .map(|(id, access)| Loosening::Access {
                id: *id,
                name: name_of(id),
                from: approved.access(*id),
                to: *access,
            })
            .collect();
        if !self.confirm_writes && approved.confirm_writes {
            found.push(Loosening::ConfirmWrites);
        }
        if self.unfiltered_allowed && !approved.unfiltered_allowed {
            found.push(Loosening::Unfiltered);
        }
        found.extend(
            self.values_shown
                .difference(&approved.values_shown)
                .map(|id| Loosening::ValuesShown {
                    id: *id,
                    name: name_of(id),
                }),
        );
        found.extend(
            self.connections
                .keys()
                .filter(|id| self.limits(**id).loosens(&approved.limits(**id)))
                .map(|id| Loosening::AgentLimits {
                    id: *id,
                    name: name_of(id),
                }),
        );
        if let (Some(files), Some(sealed)) = (&self.masking, &approved.masking) {
            found.extend(files.removed_from(sealed));
        }
        found
    }
}

/// What the keychain and the policy file hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sealed {
    /// Nothing sealed yet, or no keychain to keep a seal in.
    Nothing,
    /// The policy the user approved.
    Approved(AgentPolicy),
    /// The file no longer matches its seal: changed, replaced or removed outside the app.
    Broken,
}

impl Sealed {
    /// The policy agents get when the files say `current`.
    #[must_use]
    pub fn honored(&self, current: &AgentPolicy) -> AgentPolicy {
        match self {
            Self::Nothing => current.clone(),
            Self::Approved(approved) => current.within(approved),
            Self::Broken => current.within(&AgentPolicy::default()),
        }
    }

    /// What `current` gives agents beyond what was approved (the defaults, with a broken
    /// seal, which always asks: every value masks until the user answers); nothing without a
    /// seal.
    #[must_use]
    pub fn loosenings(
        &self,
        current: &AgentPolicy,
        connections: &[SavedConnection],
    ) -> Vec<Loosening> {
        match self {
            Self::Nothing => Vec::new(),
            Self::Approved(approved) => current.loosenings(approved, connections),
            Self::Broken => {
                let mut found = vec![Loosening::MaskingLost];
                found.extend(current.loosenings(&AgentPolicy::default(), connections));
                found
            }
        }
    }
}

/// The marks last read, with their hash.
type LoadedMarks = (String, Arc<SealedMarks>);

/// The seal of a profile's approved policy: the file, and its hash in a secret store.
#[derive(Clone)]
pub struct Seal {
    file: PathBuf,
    marks_file: PathBuf,
    id: Uuid,
    secrets: Arc<dyn SecretStore>,
    /// Read again only when the sealed hash changes.
    loaded: Arc<Mutex<Option<LoadedMarks>>>,
}

impl std::fmt::Debug for Seal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Seal")
            .field("file", &self.file)
            .finish_non_exhaustive()
    }
}

/// The SHA-256 of `bytes`, in hex.
fn digest(bytes: &[u8]) -> String {
    use std::fmt::Write;
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

impl Seal {
    /// The seal of the profile whose `settings.toml` is in `config_dir`, kept in the system
    /// keychain under a name of that folder's own, so that each profile (a portable copy, a
    /// test profile) has its seal.
    #[must_use]
    pub fn of(config_dir: &Path) -> Self {
        Self::with(config_dir, Arc::new(KeyringSecrets::new(SEAL_SERVICE)))
    }

    /// [`Seal::of`] with the hash in `secrets`.
    #[must_use]
    pub fn with(config_dir: &Path, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            file: config_dir.join(POLICY_FILE),
            marks_file: config_dir.join(MARKS_FILE),
            id: crate::profile::keychain_id(config_dir),
            secrets,
            loaded: Arc::default(),
        }
    }

    /// What is sealed. A keychain that cannot be read seals nothing.
    #[must_use]
    pub fn read(&self) -> Sealed {
        let Ok(Some(hash)) = self.secrets.get(self.id) else {
            return Sealed::Nothing;
        };
        match std::fs::read(&self.file) {
            Ok(bytes) if digest(&bytes) == hash.expose() => {
                serde_json::from_slice(&bytes).map_or(Sealed::Broken, Sealed::Approved)
            }
            _ => Sealed::Broken,
        }
    }

    /// Seals `policy` as the approved one.
    ///
    /// # Errors
    /// When the file cannot be written or the keychain refuses the hash.
    pub fn write(&self, policy: &AgentPolicy) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(policy).map_err(|e| e.to_string())?;
        std::fs::write(&self.file, &bytes)
            .map_err(|e| format!("cannot write {}: {e}", self.file.display()))?;
        self.secrets
            .set(self.id, &Secret::new(digest(&bytes)))
            .map_err(|e| e.to_string())
    }

    /// The marks `policy` seals; `None` when their file no longer matches. A process reads the
    /// file once for each sealed hash: what it verified needs no second look.
    #[must_use]
    pub fn marks(&self, policy: &AgentPolicy) -> Option<Arc<SealedMarks>> {
        let Some(hash) = &policy.marks else {
            return Some(Arc::default());
        };
        let mut loaded = self.loaded.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((known, marks)) = &*loaded
            && known == hash
        {
            return Some(Arc::clone(marks));
        }
        let bytes = std::fs::read(&self.marks_file).ok()?;
        if digest(&bytes) != *hash {
            return None;
        }
        let marks = Arc::new(SealedMarks::from_bytes(&bytes)?);
        *loaded = Some((hash.clone(), Arc::clone(&marks)));
        Some(marks)
    }

    /// Seals `policy` as the approved one, with `marks`.
    ///
    /// # Errors
    /// When a file cannot be written or the keychain refuses the hash.
    pub fn write_with_marks(
        &self,
        policy: &AgentPolicy,
        marks: &SealedMarks,
    ) -> Result<(), String> {
        let mut policy = policy.clone();
        if marks.count() == 0 {
            policy.marks = None;
            match std::fs::remove_file(&self.marks_file) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(format!("cannot remove {}: {e}", self.marks_file.display()));
                }
                _ => {}
            }
        } else {
            let bytes = marks.to_bytes()?;
            std::fs::write(&self.marks_file, &bytes)
                .map_err(|e| format!("cannot write {}: {e}", self.marks_file.display()))?;
            policy.marks = Some(digest(&bytes));
        }
        self.write(&policy)
    }

    /// The seal as a new process sees it: nothing of its marks read yet.
    #[cfg(test)]
    pub(crate) fn without_loaded_marks(&self) -> Self {
        Self {
            loaded: Arc::default(),
            ..self.clone()
        }
    }
}

#[cfg(test)]
mod values_tests {
    use super::*;

    fn saved(values: AgentValues) -> SavedConnection {
        let mut c = SavedConnection::new("c", platypus_core::DriverKind::Sqlite);
        c.agent_values = values;
        c
    }

    #[test]
    fn showing_values_is_a_loosening_that_waits_for_the_user() {
        let shown = saved(AgentValues::Shown);
        let connections = std::slice::from_ref(&shown);
        let current = AgentPolicy::current(connections, None);
        assert_eq!(current.values(shown.id), AgentValues::Shown);

        let approved = AgentPolicy::default();
        assert_eq!(
            current.loosenings(&approved, connections),
            [Loosening::ValuesShown {
                id: shown.id,
                name: "c".to_owned(),
            }]
        );
        let sealed = Sealed::Approved(approved);
        for (seal, values) in [
            (&sealed, AgentValues::Masked),
            (&Sealed::Broken, AgentValues::Masked),
            (&Sealed::Nothing, AgentValues::Shown),
        ] {
            assert_eq!(seal.honored(&current).values(shown.id), values);
        }

        let sealed = Sealed::Approved(current.clone());
        assert_eq!(
            sealed.honored(&current).values(shown.id),
            AgentValues::Shown
        );
        assert_eq!(sealed.loosenings(&current, connections), []);

        // Going back to masked needs no question, and applies at once.
        let masked = AgentPolicy::current(&[saved(AgentValues::Masked)], None);
        assert_eq!(masked.loosenings(&current, &[]), []);
    }

    #[test]
    fn a_policy_file_of_an_older_version_masks() {
        let old = r#"{"connections":{},"confirm_writes":true,"unfiltered_allowed":false}"#;
        let policy: AgentPolicy = serde_json::from_str(old).unwrap();
        assert!(policy.values_shown.is_empty());
    }
}

#[cfg(test)]
mod limits_tests {
    use super::*;

    fn limited(rows: Option<u64>, calls: Option<u64>) -> SavedConnection {
        let mut c = SavedConnection::new("c", platypus_core::DriverKind::Sqlite);
        c.agent_limits = AgentLimits {
            max_rows: rows,
            calls_per_minute: calls,
            ..AgentLimits::default()
        };
        c
    }

    /// `c` as the same connection with other limits.
    fn edited(c: &SavedConnection, rows: Option<u64>, calls: Option<u64>) -> SavedConnection {
        let mut e = limited(rows, calls);
        e.id = c.id;
        e
    }

    #[test]
    fn the_stricter_limit_wins_and_a_missing_one_is_the_loosest() {
        let a = AgentLimits {
            max_rows: Some(50),
            max_bytes: Some(2000),
            ..AgentLimits::default()
        };
        let b = AgentLimits {
            max_rows: Some(80),
            timeout_seconds: Some(5),
            ..AgentLimits::default()
        };
        let both = a.stricter(&b);
        assert_eq!(both.max_rows, Some(50));
        assert_eq!(both.max_bytes, Some(2000));
        assert_eq!(both.timeout_seconds, Some(5));
        assert_eq!(both.calls_per_minute, None);
        assert_eq!(a.stricter(&AgentLimits::default()), a);
        assert!(!a.loosens(&a) && !a.loosens(&AgentLimits::default()));
        assert!(AgentLimits::default().loosens(&a));
        assert!(
            b.loosens(&a),
            "a limit the approved one lacks is not enough"
        );
    }

    #[test]
    fn raising_or_removing_a_limit_waits_for_the_user() {
        let approved_c = limited(Some(50), Some(10));
        let approved = AgentPolicy::current(std::slice::from_ref(&approved_c), None);
        let check = |rows, calls| {
            let connections = [edited(&approved_c, rows, calls)];
            let current = AgentPolicy::current(&connections, None);
            (
                current.loosenings(&approved, &connections),
                current.within(&approved).limits(approved_c.id),
            )
        };
        // Raised: the approved limit holds, and the user is asked.
        let (found, honored) = check(Some(500), Some(10));
        assert_eq!(
            found,
            [Loosening::AgentLimits {
                id: approved_c.id,
                name: "c".to_owned(),
            }]
        );
        assert_eq!(honored, approved_c.agent_limits);
        // Removed in one field.
        let (found, honored) = check(Some(50), None);
        assert_eq!(found.len(), 1);
        assert_eq!(honored.calls_per_minute, Some(10));
        // Tightened: applies at once, nothing to ask.
        let (found, honored) = check(Some(20), Some(10));
        assert_eq!(found, []);
        assert_eq!(honored.max_rows, Some(20));
    }

    #[test]
    fn seals_decide_which_limits_agents_get() {
        let c = limited(Some(500), None);
        let connections = std::slice::from_ref(&c);
        let current = AgentPolicy::current(connections, None);
        // No seal, or a broken one that approves no limit: a limit in the file only tightens.
        for seal in [Sealed::Nothing, Sealed::Broken] {
            assert_eq!(seal.honored(&current).limits(c.id).max_rows, Some(500));
            let asked = [Loosening::MaskingLost];
            let expected: &[Loosening] = if seal == Sealed::Broken { &asked } else { &[] };
            assert_eq!(seal.loosenings(&current, connections), expected);
        }
        // A sealed policy with a lower limit holds against the raised file.
        let mut approved = AgentPolicy::default();
        approved.limits.insert(
            c.id,
            AgentLimits {
                max_rows: Some(50),
                ..AgentLimits::default()
            },
        );
        let sealed = Sealed::Approved(approved);
        assert_eq!(sealed.honored(&current).limits(c.id).max_rows, Some(50));
        assert_eq!(sealed.loosenings(&current, connections).len(), 1);
        // And against the file with the limit removed.
        let none_c = edited(&c, None, None);
        let none = AgentPolicy::current(std::slice::from_ref(&none_c), None);
        assert_eq!(sealed.honored(&none).limits(c.id).max_rows, Some(50));
        assert_eq!(
            sealed
                .loosenings(&none, std::slice::from_ref(&none_c))
                .len(),
            1
        );
        assert_eq!(
            Sealed::Broken.loosenings(&none, connections),
            [Loosening::MaskingLost]
        );
    }

    #[test]
    fn a_policy_file_of_an_older_version_has_no_limits() {
        let old = r#"{"connections":{},"confirm_writes":true,"unfiltered_allowed":false}"#;
        let policy: AgentPolicy = serde_json::from_str(old).unwrap();
        assert!(policy.limits.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use platypus_core::DriverKind;
    use platypus_store::MemorySecrets;

    use super::*;

    fn connection(access: AgentAccess) -> SavedConnection {
        let mut c = SavedConnection::new("Db", DriverKind::Sqlite);
        c.agent_access = access;
        c
    }

    #[test]
    fn a_loosening_waits_and_a_tightening_applies_at_once() {
        let (a, b) = (
            connection(AgentAccess::ReadOnly),
            connection(AgentAccess::Off),
        );
        let approved = AgentPolicy::current(&[a.clone(), b.clone()], None);
        let mut a2 = a.clone();
        a2.agent_access = AgentAccess::ReadWrite;
        let mut b2 = b.clone();
        b2.agent_access = AgentAccess::ReadOnly;
        let fresh = connection(AgentAccess::ReadWrite);
        let all = [a2, b2, fresh.clone()];
        let mut current = AgentPolicy::current(&all, None);
        current.confirm_writes = false;
        current.unfiltered_allowed = true;

        let honored = current.within(&approved);
        assert_eq!(honored.access(a.id), AgentAccess::ReadOnly);
        assert_eq!(honored.access(b.id), AgentAccess::Off);
        // A connection the approved policy does not know gets the default at most.
        assert_eq!(honored.access(fresh.id), AgentAccess::ReadOnly);
        assert!(honored.confirm_writes && !honored.unfiltered_allowed);
        let loose = current.loosenings(&approved, &all);
        assert_eq!(loose.len(), 5, "{loose:?}");
        assert!(loose.contains(&Loosening::ConfirmWrites));
        assert!(loose.contains(&Loosening::Unfiltered));

        // Stricter than approved: taken as it is, nothing to ask.
        let mut off = a.clone();
        off.agent_access = AgentAccess::Off;
        let stricter = AgentPolicy::current(&[off, b], None);
        assert_eq!(stricter.within(&approved), stricter);
        assert_eq!(stricter.loosenings(&approved, &[]), Vec::new());
    }

    #[test]
    fn the_seal_finds_an_edited_file() {
        let dir = tempfile::tempdir().unwrap();
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        let seal = Seal::with(dir.path(), Arc::clone(&secrets));
        assert_eq!(seal.read(), Sealed::Nothing);
        let a = connection(AgentAccess::ReadOnly);
        let approved = AgentPolicy::current(std::slice::from_ref(&a), None);
        seal.write(&approved).unwrap();
        assert_eq!(seal.read(), Sealed::Approved(approved.clone()));

        // An agent rewrites the file to approve itself: the seal no longer matches.
        let mut forged = approved.clone();
        forged.connections.insert(a.id, AgentAccess::ReadWrite);
        forged.confirm_writes = false;
        std::fs::write(
            dir.path().join(POLICY_FILE),
            serde_json::to_vec_pretty(&forged).unwrap(),
        )
        .unwrap();
        assert_eq!(seal.read(), Sealed::Broken);
        let current = forged;
        let honored = Sealed::Broken.honored(&current);
        assert_eq!(honored.access(a.id), AgentAccess::ReadOnly);
        assert!(honored.confirm_writes);
        // Access and asking, and that every value masks.
        assert_eq!(Sealed::Broken.loosenings(&current, &[a]).len(), 3);
        // A removed file is no better.
        std::fs::remove_file(dir.path().join(POLICY_FILE)).unwrap();
        assert_eq!(seal.read(), Sealed::Broken);
        // Each profile has a seal of its own.
        let other = tempfile::tempdir().unwrap();
        assert_eq!(Seal::with(other.path(), secrets).read(), Sealed::Nothing);
    }
}
