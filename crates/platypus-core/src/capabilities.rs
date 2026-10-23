//! Optional driver features. A [`Connection`](crate::Connection) method that not every
//! database supports has a default that returns [`Error::Unsupported`](crate::Error), and a
//! flag here that says whether the driver overrides it. Drivers add support independently;
//! the UI reads the flags to hide what a connection cannot do.
//!
//! Adding an optional feature:
//! 1. a method on `Connection` whose default calls [`unsupported`];
//! 2. a flag below, `false` by default;
//! 3. a check in `platypus-conformance` that the flag and the method agree.

use serde::Serialize;

use crate::{Error, ObjectKind};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct Capabilities {
    /// Manual commit mode, `commit` and `rollback`.
    pub transactions: bool,
    /// The cancel handle stops a running statement.
    pub cancel: bool,
    /// [`Connection::explain`](crate::Connection::explain).
    pub explain: bool,
    /// [`Connection::object_ddl`](crate::Connection::object_ddl).
    pub object_ddl: bool,
    /// [`Connection::databases`](crate::Connection::databases): other databases of the
    /// server can be opened from the same saved connection.
    pub multiple_databases: bool,
}

/// The error a `Connection` method returns when the driver does not implement it.
#[must_use]
pub fn unsupported(feature: &str) -> Error {
    Error::Unsupported(feature.to_owned())
}

/// A query plan as the server produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Explain {
    pub format: ExplainFormat,
    /// The plan document: JSON text for [`ExplainFormat::Json`], else plain text.
    pub plan: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExplainFormat {
    Json,
    Text,
}

/// Which object [`Connection::object_ddl`](crate::Connection::object_ddl) describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectRef<'a> {
    pub schema: &'a str,
    pub name: &'a str,
    pub kind: ObjectKind,
    /// The table of a trigger.
    pub parent: Option<&'a str>,
}
