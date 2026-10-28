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

impl Error {
    /// The error with the server it came from, `(MySQL 5.7.44)` after its message, so that a
    /// reason that depends on the version says which. Cancels, closed connections, missing
    /// drivers and messages that name the version already stay as they are.
    #[must_use]
    pub fn with_server(self, server: &crate::ServerInfo) -> Self {
        let version = server
            .version_number()
            .map_or_else(|| server.version.trim().to_owned(), |v| v.to_string());
        if version.is_empty() {
            return self;
        }
        let tag = |message: &mut String| {
            if !message.contains(&version) {
                *message = format!("{message} ({} {version})", server.product);
            }
        };
        match self {
            Self::Database(mut db) => {
                tag(&mut db.message);
                Self::Database(db)
            }
            Self::Unsupported(mut m) => {
                tag(&mut m);
                Self::Unsupported(m)
            }
            Self::Other(mut m) => {
                tag(&mut m);
                Self::Other(m)
            }
            Self::Decode {
                column,
                db_type,
                mut message,
            } => {
                tag(&mut message);
                Self::Decode {
                    column,
                    db_type,
                    message,
                }
            }
            other => other,
        }
    }
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
    use super::*;

    #[test]
    fn errors_name_the_server_they_came_from() {
        let server = crate::ServerInfo {
            product: "MySQL".to_owned(),
            version: "5.7.44-log".to_owned(),
            encrypted: false,
        };
        let db = Error::Database(DbError {
            code: Some("1054".to_owned()),
            message: "Unknown column 'x'".to_owned(),
            position: Some(8),
            ..DbError::default()
        })
        .with_server(&server);
        let Error::Database(db) = db else { panic!() };
        assert_eq!(db.message, "Unknown column 'x' (MySQL 5.7.44)");
        assert_eq!(db.position, Some(8), "the rest stays");
        let named = Error::Unsupported("needs 8.0.18; this server is 5.7.44".to_owned());
        assert_eq!(
            named.with_server(&server).to_string(),
            "unsupported: needs 8.0.18; this server is 5.7.44",
            "named once"
        );
        assert!(matches!(
            Error::Cancelled.with_server(&server),
            Error::Cancelled
        ));
        let pg = crate::ServerInfo {
            product: "PostgreSQL".to_owned(),
            version: "16.4 (Debian 16.4-1.pgdg120+2)".to_owned(),
            encrypted: false,
        };
        assert_eq!(
            Error::Other("boom".to_owned()).with_server(&pg).to_string(),
            "boom (PostgreSQL 16.4)"
        );
    }

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
