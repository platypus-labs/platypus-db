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

    /// The driver of a database that is not built in has not been downloaded yet.
    #[error("the {} driver is not installed", .driver.name())]
    DriverMissing { driver: crate::DriverKind },

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

/// A message made of a chain of causes (`a: b: c`) without the causes that repeat an
/// earlier one, as some libraries print theirs (`Input/output error: Input/output error:
/// …`).
#[must_use]
pub fn without_repeats(message: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    for part in message.split(": ") {
        if !kept
            .iter()
            .any(|k| k.trim().eq_ignore_ascii_case(part.trim()))
        {
            kept.push(part);
        }
    }
    kept.join(": ")
}

#[cfg(test)]
mod tests {
    use super::without_repeats;

    #[test]
    fn drops_repeated_causes() {
        assert_eq!(
            without_repeats("Input/output error: Input/output error: invalid peer certificate"),
            "Input/output error: invalid peer certificate"
        );
        assert_eq!(
            without_repeats(
                "An error occured during the attempt of performing I/O: An error occured \
                 during the attempt of performing I/O: reset"
            ),
            "An error occured during the attempt of performing I/O: reset"
        );
        assert_eq!(without_repeats("a: b: c"), "a: b: c");
        assert_eq!(without_repeats("plain"), "plain");
    }
}
