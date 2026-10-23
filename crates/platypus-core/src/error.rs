use std::fmt;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid connection configuration: {0}")]
    Config(String),

    #[error("connection failed: {0}")]
    Connect(String),

    #[error(transparent)]
    Database(#[from] DbError),

    #[error("query cancelled")]
    Cancelled,

    #[error("connection closed")]
    Closed,

    #[error("unsupported: {0}")]
    Unsupported(String),

    #[error("failed to decode column `{column}` ({db_type}): {message}")]
    Decode {
        column: String,
        db_type: String,
        message: String,
    },

    #[error(transparent)]
    Arrow(#[from] arrow_schema::ArrowError),

    #[error("{0}")]
    Other(String),
}

/// An error reported by the database server, with the details editors need to point at it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DbError {
    /// Vendor error code (SQLSTATE for PostgreSQL, error number for MySQL).
    pub code: Option<String>,
    pub message: String,
    pub detail: Option<String>,
    pub hint: Option<String>,
    /// 1-based character offset into the submitted SQL, when the server reports one.
    pub position: Option<u32>,
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.code {
            Some(code) => write!(f, "{code}: {}", self.message)?,
            None => f.write_str(&self.message)?,
        }
        if let Some(detail) = &self.detail {
            write!(f, "\nDETAIL: {detail}")?;
        }
        if let Some(hint) = &self.hint {
            write!(f, "\nHINT: {hint}")?;
        }
        Ok(())
    }
}

impl std::error::Error for DbError {}
