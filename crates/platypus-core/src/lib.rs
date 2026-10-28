//! Core abstractions shared by every PlatypusDB driver and frontend.
//!
//! A [`Driver`] opens a [`Connection`]; executing SQL yields an [`ExecResult`], whose row
//! variant is a [`RecordBatchStream`] of Arrow record batches. Frontends never see
//! driver-specific types.

mod admin;
mod capabilities;
mod config;
mod connection;
mod error;
mod explain;
pub mod geometry;
mod init;
mod metadata;
pub mod special;
mod transaction;

pub use admin::{EndSession, ServerLock, ServerSession, session_number};
pub use capabilities::{Capabilities, ObjectRef, unsupported};
pub use config::{
    Bridge, ConnectionConfig, DriverKind, JdbcUrl, ParseNote, ParsedConnection, Secret, SshAuth,
    SshConfig, SslMode,
};
pub use connection::{
    CancelHandle, Canceller, Connection, Driver, ExecOptions, ExecResult, Executed, NextResult,
    RecordBatchStream, ServerInfo, execute_each,
};
pub use error::{DbError, Error, Result, without_repeats};
pub use explain::{
    Explain, ExplainFormat, FullScan, LARGE_SCAN_ROWS, PlanNode, explain_document, plan_from_text,
};
pub use init::{SessionInit, run_init};
pub use metadata::{
    CATALOG_LIMIT, CatalogColumn, CatalogForeignKey, ColumnMeta, ConstraintKind, ConstraintMeta,
    FieldMeta, ForeignKeyMeta, IndexMeta, MatchKind, MetadataMatch, ObjectKind, ObjectMeta,
    Property, ReferencingKey, SEARCH_LIMIT, SchemaMeta, TableDetails, TextRow, TextRowExt,
    group_sorted, query_catalog, query_text,
};
pub use transaction::{
    TransactionState, TxControl, TxEffect, TxTracker, classify, leading_words,
    open_transaction_error,
};

/// Re-exported so frontends use the same Arrow version as the drivers.
pub use arrow_array;
pub use arrow_schema;

/// Version of the PlatypusDB core, shared by the CLI and the desktop app.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Arrow field metadata key holding the column's original database type name
/// (e.g. `jsonb`, `numeric(12,2)`), so the UI can show it even when the Arrow type is `Utf8`.
pub const META_DB_TYPE: &str = "platypus.db_type";
