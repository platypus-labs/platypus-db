use platypus_core::Error;
use platypus_store::StoreError;
use serde::Serialize;

/// Error shape sent to the UI: a stable `kind` to branch on plus display details.
#[derive(Debug, Serialize)]
pub struct CommandError {
    kind: &'static str,
    message: String,
    /// Vendor error code, e.g. a PostgreSQL SQLSTATE.
    code: Option<String>,
    detail: Option<String>,
    hint: Option<String>,
    /// 1-based character offset in the SQL.
    position: Option<u32>,
}

impl CommandError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            code: None,
            detail: None,
            hint: None,
            position: None,
        }
    }

    pub fn not_found(what: &str, id: impl std::fmt::Display) -> Self {
        Self::new("not_found", format!("{what} {id} not found"))
    }
}

impl From<Error> for CommandError {
    fn from(e: Error) -> Self {
        match e {
            Error::Database(db) => Self {
                kind: "database",
                message: db.message,
                code: db.code,
                detail: db.detail,
                hint: db.hint,
                position: db.position,
            },
            Error::Cancelled => Self::new("cancelled", "Query cancelled"),
            Error::Connect(m) => Self::new("connect", m),
            Error::Config(m) => Self::new("config", m),
            Error::Closed => Self::new("closed", "Connection closed"),
            other => Self::new("other", other.to_string()),
        }
    }
}

impl From<StoreError> for CommandError {
    fn from(e: StoreError) -> Self {
        Self::new("store", e.to_string())
    }
}

impl From<arrow_schema::ArrowError> for CommandError {
    fn from(e: arrow_schema::ArrowError) -> Self {
        Self::new("other", e.to_string())
    }
}

pub type CommandResult<T> = Result<T, CommandError>;
