//! What the agent tools report when they fail. Messages never carry a secret: errors
//! from the database go through [`Redactor`](crate::redact::Redactor) first.

use std::time::Duration;

use serde::Serialize;

pub type Result<T, E = AgentError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// No connection by that name is open to agents (unknown, or its access is off).
    #[error("{0}")]
    NotFound(String),

    /// The connection's agent access, or a safety setting, does not allow it.
    #[error("{0}")]
    Denied(String),

    /// The saved passwords are behind the master password, which agents cannot enter.
    #[error(
        "the saved password of `{0}` is locked by PlatypusDB's master password, which agents cannot enter: \
         use a password command or .pgpass for this connection, or turn the master password off"
    )]
    Locked(String),

    /// The arguments of the call are wrong.
    #[error("{0}")]
    Invalid(String),

    /// Connecting failed.
    #[error("cannot connect to `{name}`: {message}")]
    Connect { name: String, message: String },

    /// The database refused the statement or the request.
    #[error("{0}")]
    Database(String),

    /// The statement ran longer than allowed and was cancelled.
    #[error("cancelled after {} s: raise timeout_seconds or narrow the query", .0.as_secs())]
    Timeout(Duration),

    /// The profile's files cannot be read.
    #[error("{0}")]
    Store(String),
}

impl AgentError {
    /// A stable code for scripts: `not_found`, `denied`, `locked`, `invalid`, `connect`,
    /// `database`, `timeout` or `store`.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::Denied(_) => "denied",
            Self::Locked(_) => "locked",
            Self::Invalid(_) => "invalid",
            Self::Connect { .. } => "connect",
            Self::Database(_) => "database",
            Self::Timeout(_) => "timeout",
            Self::Store(_) => "store",
        }
    }

    /// The error as the CLI prints it: `{"error": {"kind": …, "message": …}}`.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        #[derive(Serialize)]
        struct Body<'a> {
            kind: &'a str,
            message: String,
        }
        serde_json::json!({ "error": Body { kind: self.kind(), message: self.to_string() } })
    }
}

impl From<platypus_store::StoreError> for AgentError {
    fn from(e: platypus_store::StoreError) -> Self {
        Self::Store(e.to_string())
    }
}
