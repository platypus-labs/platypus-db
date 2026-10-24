//! Core abstractions shared by every PlatypusDB driver and frontend.
//!
//! A [`Driver`] opens a [`Connection`]; executing SQL yields an [`ExecResult`], whose row
//! variant is a [`RecordBatchStream`] of Arrow record batches. Frontends never see
//! driver-specific types.

mod capabilities;
mod config;
mod connection;
mod error;
mod metadata;
mod transaction;

pub use capabilities::{Capabilities, Explain, ExplainFormat, ObjectRef, unsupported};
pub use config::{ConnectionConfig, DriverKind, Secret, SshAuth, SshConfig, SslMode};
pub use connection::{
    CancelHandle, Canceller, Connection, Driver, ExecOptions, ExecResult, RecordBatchStream,
    ServerInfo,
};
pub use error::{DbError, Error, Result};
pub use metadata::{
    CATALOG_LIMIT, CatalogColumn, ColumnMeta, ConstraintKind, ConstraintMeta, ForeignKeyMeta,
    IndexMeta, ObjectKind, ObjectMeta, Property, ReferencingKey, SchemaMeta, TableDetails, TextRow,
    TextRowExt, group_sorted, query_catalog, query_text,
};
pub use transaction::{
    TransactionState, TxEffect, TxTracker, classify, leading_words, open_transaction_error,
};

/// Re-exported so frontends use the same Arrow version as the drivers.
pub use arrow_array;
pub use arrow_schema;

/// Version of the PlatypusDB core, shared by the CLI and the desktop app.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Arrow field metadata key holding the column's original database type name
/// (e.g. `jsonb`, `numeric(12,2)`), so the UI can show it even when the Arrow type is `Utf8`.
pub const META_DB_TYPE: &str = "platypus.db_type";
