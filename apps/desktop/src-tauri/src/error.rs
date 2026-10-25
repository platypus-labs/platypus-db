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

    /// Says where the error happened, e.g. which generated statement failed. The position
    /// is dropped: it points into SQL the user did not type.
    #[must_use]
    pub fn context(mut self, context: impl std::fmt::Display) -> Self {
        self.message = format!("{context}: {}", self.message);
        self.position = None;
        self
    }
}

impl From<&Error> for CommandError {
    fn from(e: &Error) -> Self {
        match e {
            Error::Database(db) => Self {
                kind: "database",
                message: db.message.clone(),
                code: db.code.clone(),
                detail: db.detail.clone(),
                hint: db.hint.clone(),
                position: db.position,
            },
            Error::Cancelled => Self::new("cancelled", "Query cancelled"),
            Error::Connect(m) => Self::new("connect", m.clone()),
            Error::Config(m) => Self::new("config", m.clone()),
            Error::Closed => Self::new("closed", "Connection closed"),
            // The UI offers to download the driver, named by `code`, then tries again.
            Error::DriverMissing { driver } => Self {
                code: serde_json::to_value(driver)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned)),
                ..Self::new("driver_missing", e.to_string())
            },
            other => Self::new("other", other.to_string()),
        }
    }
}

impl From<Error> for CommandError {
    fn from(e: Error) -> Self {
        Self::from(&e)
    }
}

impl From<StoreError> for CommandError {
    fn from(e: StoreError) -> Self {
        // The UI asks for the master password on `locked`, then tries again.
        let kind = if matches!(e, StoreError::Locked) {
            "locked"
        } else {
            "store"
        };
        Self::new(kind, e.to_string())
    }
}

impl From<arrow_schema::ArrowError> for CommandError {
    fn from(e: arrow_schema::ArrowError) -> Self {
        Self::new("other", e.to_string())
    }
}

pub type CommandResult<T> = Result<T, CommandError>;
