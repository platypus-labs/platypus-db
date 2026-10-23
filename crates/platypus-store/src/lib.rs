//! Local persistence for PlatypusDB.
//!
//! Saved connections live in a human-editable `connections.toml`; passwords never touch
//! that file and are kept in the OS keychain through a [`SecretStore`].

mod connections;
mod secrets;

pub use connections::{ConnectionStore, PasswordUpdate, SavedConnection};
pub use secrets::{KeyringSecrets, MemorySecrets, SecretStore};

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

    #[error("keychain error: {0}")]
    Keychain(String),

    #[error("no saved connection with id {0}")]
    NotFound(uuid::Uuid),

    #[error("{0}")]
    Invalid(String),
}
