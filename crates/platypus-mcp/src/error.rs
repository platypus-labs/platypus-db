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
    #[error("cancelled after {0:?}: raise timeout_seconds or narrow the query")]
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
        match e {
            // A parse error quotes the file's lines, which may hold a password command.
            platypus_store::StoreError::Format { path, message } => Self::Store(format!(
                "invalid {}: {}",
                path.display(),
                message.lines().next().unwrap_or_default()
            )),
            other => Self::Store(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_errors_do_not_quote_the_file() {
        let e = AgentError::from(platypus_store::StoreError::Format {
            path: "connections.toml".into(),
            message: "TOML parse error at line 3\n  |\n3 | password_command = \"echo s3cret\""
                .to_owned(),
        });
        assert_eq!(
            e.to_string(),
            "invalid connections.toml: TOML parse error at line 3"
        );
        assert_eq!(e.kind(), "store");
        assert_eq!(
            AgentError::Timeout(Duration::from_millis(1500)).to_string(),
            "cancelled after 1.5s: raise timeout_seconds or narrow the query"
        );
    }
}
