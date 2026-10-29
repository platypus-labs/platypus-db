//! Local persistence for PlatypusDB.
//!
//! Saved connections live in a human-editable `connections.toml`; passwords never touch
//! that file and are kept in the OS keychain, or in a file encrypted with a master password
//! ([`VaultSecrets`]), through a [`SecretStore`]. Executed statements
//! are logged to a local SQLite database by [`HistoryStore`], the calls of external agents
//! by [`AgentCallStore`]. Notes and tags on tables and
//! columns live in a local SQLite database, [`CatalogStore`]. Preferences live in
//! `settings.toml` through [`SettingsStore`].

mod agent_calls;
mod auth;
mod catalog;
mod connections;
mod history;
mod import;
mod scripts;
mod secrets;
mod settings;
mod sqlite;
mod vault;

pub use agent_calls::{
    AGENT_CALLS_LIMIT, AgentCall, AgentCallQuery, AgentCallStore, AgentProposal, AgentSession,
    NewAgentCall, PROPOSAL_LIFETIME, agent_calls_csv, rfc3339,
};
pub use auth::{
    command_password, external_password, external_password_quiet, pgpass_password, pgpass_path,
};
pub use catalog::{
    CatalogEntry, CatalogKey, CatalogStore, DisplayColumn, TagCount, normalize_tags,
};
pub use connections::{
    AgentAccess, ConnectBy, ConnectionFolder, ConnectionStore, ConnectionType, ImportInto,
    ImportRequest, ImportTarget, NetworkProfile, PasswordUpdate, Project, ProjectFolder,
    Restrictions, SavedConnection, SavedInit, SavedSsh, SshAuthKind, TextJson, VirtualKey,
    initials_of,
};
pub use history::{
    HistoryEntry, HistoryOrigin, HistoryQuery, HistoryStatus, HistoryStore, NewHistoryEntry,
};
pub use import::{
    ImportFile, ImportNote, ImportSource, ImportedProject, ImportedSecrets, ProjectNote,
    SkippedConnection, detect as detect_imports, import as import_connections,
};
pub use scripts::{
    LINKS_FILE, ScriptEntry, ScriptText, list_scripts, read_links as read_script_links,
    read_script, relative as script_relative, resolve as resolve_script,
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

    #[error("catalog error: {0}")]
    Catalog(String),

    #[error("keychain error: {0}")]
    Keychain(String),

    #[error("the saved passwords are locked: enter the master password")]
    Locked,

    #[error("no saved connection with id {0}")]
    NotFound(uuid::Uuid),

    #[error("{0}")]
    Invalid(String),
}
