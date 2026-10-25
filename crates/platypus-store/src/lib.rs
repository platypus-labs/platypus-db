//! Local persistence for PlatypusDB.
//!
//! Saved connections live in a human-editable `connections.toml`; passwords never touch
//! that file and are kept in the OS keychain, or in a file encrypted with a master password
//! ([`VaultSecrets`]), through a [`SecretStore`]. Executed statements
//! are logged to a local SQLite database by [`HistoryStore`]. Preferences live in
//! `settings.toml` through [`SettingsStore`].

mod auth;
mod connections;
mod history;
mod import;
mod secrets;
mod settings;
mod vault;

pub use auth::{command_password, external_password, pgpass_password, pgpass_path};
pub use connections::{
    AgentAccess, ConnectionFolder, ConnectionStore, ConnectionType, NetworkProfile, PasswordUpdate,
    Project, SavedConnection, SavedInit, SavedSsh, SshAuthKind, VirtualKey, initials_of,
};
pub use history::{
    HistoryEntry, HistoryOrigin, HistoryQuery, HistoryStatus, HistoryStore, NewHistoryEntry,
};
pub use import::{
    ImportFile, ImportSource, detect as detect_imports, import as import_connections,
};
pub use secrets::{KeyringSecrets, MemorySecrets, SecretStore};
pub use settings::SettingsStore;
pub use toml;
pub use vault::VaultSecrets;

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("cannot access {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },

    #[error("invalid {path}: {message}")]
    Format {
        path: std::path::PathBuf,
        message: String,
    },

    #[error("history error: {0}")]
    History(String),

    #[error("keychain error: {0}")]
    Keychain(String),

    #[error("the saved passwords are locked: enter the master password")]
    Locked,

    #[error("no saved connection with id {0}")]
    NotFound(uuid::Uuid),

    #[error("{0}")]
    Invalid(String),
}
