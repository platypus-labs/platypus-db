//! The app's profile on disk: the same folders the desktop app uses, so agents see the
//! connections saved there.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use platypus_store::{
    ConnectionStore, HistoryStore, KeyringSecrets, SecretStore, SettingsStore, VaultSecrets,
};

use crate::error::{AgentError, Result};

/// The desktop app's bundle identifier, which names its folders.
const APP_IDENTIFIER: &str = "dev.platypusdb.desktop";

/// The environment variable that puts the whole profile in one folder, as for the app.
pub const DATA_DIR_ENV: &str = "PLATYPUS_DATA_DIR";

/// Where the connections, settings and history live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// `connections.toml`, `settings.toml` and the master password's `secrets.vault`.
    config_dir: PathBuf,
    /// `history.db`.
    data_dir: PathBuf,
}

impl Profile {
    /// The app's profile: `PLATYPUS_DATA_DIR` when set, else the OS folders the app uses
    /// (`%APPDATA%\dev.platypusdb.desktop` on Windows).
    pub fn from_env() -> Result<Self> {
        if let Some(dir) = std::env::var_os(DATA_DIR_ENV) {
            return Ok(Self::at(dir));
        }
        let missing =
            || AgentError::Store("cannot find the user's configuration folder".to_owned());
        let data_dir = dirs::data_dir().ok_or_else(missing)?.join(APP_IDENTIFIER);
        // The drivers the app downloaded, for the databases reached through ADBC.
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

    /// The app's settings; an unreadable file reads as the defaults.
    #[must_use]
    pub fn settings(&self) -> Option<SettingsStore> {
        SettingsStore::open(self.config_dir.join("settings.toml")).ok()
    }

    pub fn history(&self) -> Result<HistoryStore> {
        Ok(HistoryStore::open(&self.data_dir.join("history.db"))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
