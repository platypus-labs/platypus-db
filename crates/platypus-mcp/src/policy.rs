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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use platypus_core::Secret;
use platypus_store::{AgentAccess, KeyringSecrets, SavedConnection, SecretStore, SettingsStore};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The file of the approved policy, in the folder of `settings.toml`.
pub const POLICY_FILE: &str = "agent-policy.json";

/// The keychain service that holds the seals, apart from the saved passwords.
const SEAL_SERVICE: &str = "platypusdb-agent-policy";

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
}

impl Default for AgentPolicy {
    /// The app's defaults: a connection not listed gets read-only access at most.
    fn default() -> Self {
        Self {
            connections: BTreeMap::new(),
            confirm_writes: true,
            unfiltered_allowed: false,
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
        }
    }

    /// What this policy gives agents beyond `approved`, with the connections' names from
    /// `connections`.
    #[must_use]
    pub fn loosenings(&self, approved: &Self, connections: &[SavedConnection]) -> Vec<Loosening> {
        let mut found: Vec<Loosening> = self
            .connections
            .iter()
            .filter(|(id, access)| **access > approved.access(**id))
            .map(|(id, access)| Loosening::Access {
                id: *id,
                name: connections
                    .iter()
                    .find(|c| c.id == *id)
                    .map(|c| c.name.clone())
                    .unwrap_or_default(),
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
    /// seal); nothing without a seal.
    #[must_use]
    pub fn loosenings(
        &self,
        current: &AgentPolicy,
        connections: &[SavedConnection],
    ) -> Vec<Loosening> {
        match self {
            Self::Nothing => Vec::new(),
            Self::Approved(approved) => current.loosenings(approved, connections),
            Self::Broken => current.loosenings(&AgentPolicy::default(), connections),
        }
    }
}

/// The seal of a profile's approved policy: the file, and its hash in a secret store.
#[derive(Clone)]
pub struct Seal {
    file: PathBuf,
    id: Uuid,
    secrets: Arc<dyn SecretStore>,
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
            id: crate::profile::keychain_id(config_dir),
            secrets,
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
        assert_eq!(Sealed::Broken.loosenings(&current, &[a]).len(), 2);
        // A removed file is no better.
        std::fs::remove_file(dir.path().join(POLICY_FILE)).unwrap();
        assert_eq!(seal.read(), Sealed::Broken);
        // Each profile has a seal of its own.
        let other = tempfile::tempdir().unwrap();
        assert_eq!(Seal::with(other.path(), secrets).read(), Sealed::Nothing);
    }
}
