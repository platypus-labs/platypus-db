//! Local persistence for PlatypusDB.
//!
//! Saved connections live in a human-editable `connections.toml`; passwords never touch
//! that file and are kept in the OS keychain through a [`SecretStore`]. Executed statements
//! are logged to a local SQLite database by [`HistoryStore`]. Preferences live in
//! `settings.toml` through [`SettingsStore`].

mod connections;
mod history;
mod secrets;
mod settings;

pub use connections::{ConnectionStore, PasswordUpdate, SavedConnection};
pub use history::{HistoryEntry, HistoryQuery, HistoryStatus, HistoryStore, NewHistoryEntry};
pub use secrets::{KeyringSecrets, MemorySecrets, SecretStore};
pub use settings::SettingsStore;
pub use toml;

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

    #[error("no saved connection with id {0}")]
    NotFound(uuid::Uuid),

    #[error("{0}")]
    Invalid(String),
}
