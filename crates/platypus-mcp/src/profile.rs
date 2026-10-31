//! The app's profile on disk: the same folders the desktop app uses, so agents see the
//! connections saved there.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use platypus_store::{
    AgentCallStore, ConnectionStore, HistoryStore, KeyringSecrets, SecretStore, SettingsStore,
    VaultSecrets,
};

use crate::error::{AgentError, Result};

/// The desktop app's bundle identifier, which names its folders.
const APP_IDENTIFIER: &str = "dev.platypusdb.desktop";

/// The file of the agents' tool calls, in the data folder.
pub const AGENT_CALLS_FILE: &str = "agent-calls.db";

/// The environment variable that puts the whole profile in one folder, as for the app.
pub const DATA_DIR_ENV: &str = "PLATYPUS_DATA_DIR";

/// The folder of a portable copy of the app: a `data` folder next to its executable, as VS
/// Code has. Everything the app keeps goes there, the web view's storage too, so the copy
/// moves with its folder (saved passwords stay in the system keychain, unless a master
/// password keeps them in the folder's vault). The app and its MCP server (`--mcp`, the
/// same executable) both use it.
#[must_use]
pub fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    portable_dir_of(&exe)
}

fn portable_dir_of(exe: &Path) -> Option<PathBuf> {
    let data = exe.parent()?.join("data");
    data.is_dir().then_some(data)
}

/// Which of the profiles on this computer the running app uses: they all share the system
/// keychain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileKind {
    /// The OS folders of the installed app.
    Installed,
    /// The `data` folder of a portable copy ([`portable_dir`]).
    Portable,
    /// The folder `PLATYPUS_DATA_DIR` names: a throwaway one, as the end-to-end tests use.
    Separate,
}

impl ProfileKind {
    /// The kind of profile [`Profile::from_env`] (and the app) picks.
    #[must_use]
    pub fn from_env() -> Self {
        if std::env::var_os(DATA_DIR_ENV).is_some() {
            Self::Separate
        } else if portable_dir().is_some() {
            Self::Portable
        } else {
            Self::Installed
        }
    }
}

/// The id of the profile whose `connections.toml` and `settings.toml` are in `config_dir`:
/// what it keeps in the system keychain beside its connections' passwords (the sync
/// passphrase, the agent policy's seal) goes under this name, so that a profile (a portable
/// copy, a test profile) never reads or writes another's. The first 16 bytes of the SHA-256
/// of the folder, lowercased, as `canonicalize` gives it.
#[must_use]
pub fn keychain_id(config_dir: &Path) -> uuid::Uuid {
    let dir = std::fs::canonicalize(config_dir).unwrap_or_else(|_| config_dir.to_owned());
    let hash = ring::digest::digest(
        &ring::digest::SHA256,
        dir.to_string_lossy().to_lowercase().as_bytes(),
    );
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&hash.as_ref()[..16]);
    uuid::Uuid::from_bytes(bytes)
}

/// Where the connections, settings and history live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// `connections.toml`, `settings.toml` and the master password's `secrets.vault`.
    config_dir: PathBuf,
    /// `history.db` and `agent-calls.db`.
    data_dir: PathBuf,
}

impl Profile {
    /// The app's profile: `PLATYPUS_DATA_DIR` when set, else the `data` folder of a
    /// portable copy, else the OS folders the app uses (`%APPDATA%\dev.platypusdb.desktop`
    /// on Windows).
    pub fn from_env() -> Result<Self> {
        if let Some(dir) = std::env::var_os(DATA_DIR_ENV) {
            return Ok(Self::at(dir));
        }
        if let Some(dir) = portable_dir() {
            return Ok(Self::at(dir));
        }
        let missing =
            || AgentError::Store("cannot find the user's configuration folder".to_owned());
        let data_dir = dirs::data_dir().ok_or_else(missing)?.join(APP_IDENTIFIER);
        // The drivers and client libraries the app downloaded (ADBC, Oracle Instant Client).
        platypus_drivers::set_data_dir(&data_dir);
        Ok(Self {
            config_dir: dirs::config_dir().ok_or_else(missing)?.join(APP_IDENTIFIER),
            data_dir,
        })
    }

    /// Everything in `dir`, as with `PLATYPUS_DATA_DIR`.
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        platypus_drivers::set_data_dir(&dir);
        Self {
            config_dir: dir.clone(),
            data_dir: dir,
        }
    }

    #[must_use]
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Where the open app writes its bridge ([`BRIDGE_FILE`](crate::link::BRIDGE_FILE)).
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The saved connections, read again at each call so changes made in the app apply
    /// at once. With a master password their secrets stay locked: agents never enter it.
    pub fn connections(&self) -> Result<ConnectionStore> {
        let vault = self.config_dir.join("secrets.vault");
        let secrets: Arc<dyn SecretStore> = if vault.exists() {
            Arc::new(VaultSecrets::locked(vault))
        } else {
            Arc::new(KeyringSecrets::default())
        };
        Ok(ConnectionStore::open(
            self.config_dir.join("connections.toml"),
            secrets,
        )?)
    }

    /// The user's notes, tags and sensitive marks (`catalog.db`); `None` before the app made it.
    ///
    /// # Errors
    /// When the file is there but cannot be read.
    pub fn catalog(&self) -> Result<Option<platypus_store::CatalogStore>> {
        let path = self.data_dir.join("catalog.db");
        if !path.exists() {
            return Ok(None);
        }
        platypus_store::CatalogStore::open(&path)
            .map(Some)
            .map_err(|e| AgentError::Store(format!("cannot read the catalog: {e}")))
    }

    /// The app's settings; an unreadable file reads as the defaults.
    #[must_use]
    pub fn settings(&self) -> Option<SettingsStore> {
        SettingsStore::open(self.config_dir.join("settings.toml")).ok()
    }

    pub fn history(&self) -> Result<HistoryStore> {
        Ok(HistoryStore::open(&self.data_dir.join("history.db"))?)
    }

    /// The log of the agents' tool calls, which the app shows under Agents.
    pub fn agent_calls(&self) -> Result<AgentCallStore> {
        Ok(AgentCallStore::open(&self.data_dir.join(AGENT_CALLS_FILE))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_data_folder_next_to_the_executable_makes_a_portable_copy() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("platypus-db.exe");
        std::fs::write(&exe, "").unwrap();
        assert_eq!(portable_dir_of(&exe), None);
        // A file named `data` is not the folder.
        std::fs::write(dir.path().join("data"), "").unwrap();
        assert_eq!(portable_dir_of(&exe), None);
        std::fs::remove_file(dir.path().join("data")).unwrap();
        std::fs::create_dir(dir.path().join("data")).unwrap();
        assert_eq!(portable_dir_of(&exe), Some(dir.path().join("data")));
    }

    #[test]
    fn a_data_dir_holds_the_whole_profile() {
        let p = Profile::at("/tmp/x");
        assert_eq!(p.config_dir, p.data_dir);
        let os = Profile {
            config_dir: dirs::config_dir().unwrap().join(APP_IDENTIFIER),
            data_dir: dirs::data_dir().unwrap().join(APP_IDENTIFIER),
        };
        if std::env::var_os(DATA_DIR_ENV).is_none() {
            assert_eq!(Profile::from_env().unwrap(), os);
        }
    }

    #[test]
    fn each_folder_has_its_own_keychain_id() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        assert_eq!(keychain_id(&a), keychain_id(&a));
        assert_ne!(keychain_id(&a), keychain_id(&b));
        // The same folder spelled another way is the same profile.
        assert_eq!(keychain_id(&a), keychain_id(&b.join("..").join("a")));
    }

    /// `forgetKeychainEntries` in `e2e/run.mjs` computes the id this way to remove what a test
    /// profile left in the Credential Manager.
    #[cfg(windows)]
    #[test]
    fn the_keychain_id_is_the_hash_of_the_verbatim_lowercased_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(dir.path()).unwrap();
        let text = path.to_string_lossy().to_lowercase();
        assert!(text.starts_with(r"\\?\"), "{text}");
        let hash = ring::digest::digest(&ring::digest::SHA256, text.as_bytes());
        assert_eq!(keychain_id(dir.path()).as_bytes(), &hash.as_ref()[..16],);
        let upper = dir.path().to_string_lossy().to_uppercase();
        assert_eq!(keychain_id(Path::new(&upper)), keychain_id(dir.path()));
    }
}
