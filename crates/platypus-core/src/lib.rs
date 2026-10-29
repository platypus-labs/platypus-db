//! Core abstractions shared by every PlatypusDB driver and frontend.
//!
//! A [`Driver`] opens a [`Connection`]; executing SQL yields an [`ExecResult`], whose row
//! variant is a [`RecordBatchStream`] of Arrow record batches. Frontends never see
//! driver-specific types.

mod admin;
mod capabilities;
mod config;
mod connection;
mod env;
mod error;
mod explain;
pub mod geometry;
mod init;
mod metadata;
pub mod special;
mod transaction;
mod version;

pub use admin::{
    EndSession, ServerLock, ServerSession, query_locks, refuse_own_session, session_number,
};
pub use capabilities::{Capabilities, ObjectRef, unsupported};
pub use config::{
    AFTER_DISCONNECT, BEFORE_CONNECT, Bridge, ConnectionConfig, DriverKind, JdbcUrl, POOLER,
    ParseNote, ParsedConnection, Secret, SshAuth, SshConfig, SslMode,
};
pub use connection::{
    CancelHandle, Canceller, Connection, Driver, ExecOptions, ExecResult, Executed, NextResult,
    RecordBatchStream, ServerInfo, execute_each,
};
pub use env::{ENV_FILE, EnvVars, expand as expand_env, parse_dotenv, without_references};
pub use error::{DbError, Error, Result, without_repeats};
pub use explain::{
    Explain, ExplainFormat, FullScan, LARGE_SCAN_ROWS, PlanNode, explain_document, plain_plan,
    plan_from_text, step_figures,
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

pub use version::{ServerVersion, minimum_version};

/// Re-exported so frontends use the same Arrow version as the drivers.
pub use arrow_array;
pub use arrow_schema;

/// Version of the PlatypusDB core, shared by the CLI and the desktop app.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Arrow field metadata key holding the column's original database type name
/// (e.g. `jsonb`, `numeric(12,2)`), so the UI can show it even when the Arrow type is `Utf8`.
pub const META_DB_TYPE: &str = "platypus.db_type";
