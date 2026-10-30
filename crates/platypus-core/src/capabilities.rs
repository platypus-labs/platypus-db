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
    /// [`Connection::search_metadata`](crate::Connection::search_metadata).
    pub metadata_search: bool,
    /// [`Connection::server_sessions`](crate::Connection::server_sessions),
    /// [`server_locks`](crate::Connection::server_locks) and
    /// [`end_server_session`](crate::Connection::end_server_session) with
    /// [`EndSession::Terminate`](crate::EndSession::Terminate).
    pub server_sessions: bool,
    /// [`end_server_session`](crate::Connection::end_server_session) with
    /// [`EndSession::Cancel`](crate::EndSession::Cancel): the statement of another
    /// session can be cancelled without closing it (not on SQL Server).
    pub cancel_server_queries: bool,
    /// [`Connection::top_queries`](crate::Connection::top_queries) and
    /// [`object_statistics`](crate::Connection::object_statistics).
    pub server_statistics: bool,
    /// The operations [`Connection::maintain`](crate::Connection::maintain) runs; empty
    /// when it runs none.
    pub maintenance: &'static [crate::Maintenance],
    /// [`Connection::principals`](crate::Connection::principals) and
    /// [`principal_privileges`](crate::Connection::principal_privileges): the users, roles
    /// and privileges of the server can be read, and `platypus_sql::security` writes the
    /// statements that change them.
    pub security: bool,
    /// [`Connection::bulk_load`](crate::Connection::bulk_load): rows go in with the
    /// database's own bulk loading instead of INSERT statements.
    pub bulk_load: bool,
    /// [`Connection::server_parameters`](crate::Connection::server_parameters).
    pub server_parameters: bool,
    /// [`Connection::server_metrics`](crate::Connection::server_metrics).
    pub server_metrics: bool,
}

/// The error a `Connection` method returns when the driver does not implement it.
#[must_use]
pub fn unsupported(feature: &str) -> Error {
    Error::Unsupported(feature.to_owned())
}

/// Which object [`Connection::object_ddl`](crate::Connection::object_ddl) describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectRef<'a> {
    pub schema: &'a str,
    pub name: &'a str,
    pub kind: ObjectKind,
    /// The table of a trigger.
    pub parent: Option<&'a str>,
}
