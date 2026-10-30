//! Core abstractions shared by every PlatypusDB driver and frontend.
//!
//! A [`Driver`] opens a [`Connection`]; executing SQL yields an [`ExecResult`], whose row
//! variant is a [`RecordBatchStream`] of Arrow record batches. Frontends never see
//! driver-specific types.

mod admin;
mod capabilities;
mod config;
mod connection;
mod debug;
mod env;
mod error;
mod explain;
pub mod geometry;
mod init;
mod metadata;
mod metrics;
mod parameters;
mod security;
pub mod special;
mod transaction;
mod version;

pub use admin::{
    EndSession, Maintenance, ObjectStatistics, ServerLock, ServerQuery, ServerSession,
    StatisticsKind, TOP_QUERIES, query_locks, query_statements, refuse_own_session,
    run_maintenance, session_number, sort_statistics,
};
pub use capabilities::{Capabilities, ObjectRef, unsupported};
pub use config::{
    AFTER_DISCONNECT, AUTH, AWS_PROFILE, AWS_REGION, Auth, BEFORE_CONNECT, Bridge,
    ConnectionConfig, DriverKind, FORWARD, JdbcUrl, K8S_CONTEXT, K8S_NAMESPACE, K8S_TARGET, POOLER,
    ParseNote, ParsedConnection, SSM_TARGET, Secret, SshAuth, SshConfig, SslMode,
};
pub use connection::{
    BulkRows, CancelHandle, Canceller, Connection, Driver, ExecOptions, ExecResult, Executed,
    NextResult, RecordBatchStream, ServerInfo, execute_each,
};
pub use debug::{
    DEBUG_RESULT_ROWS, DebugArgument, DebugAvailability, DebugFrame, DebugOutcome, DebugParameter,
    DebugPosition, DebugRoutine, DebugSession, DebugState, DebugStep, DebugVariable, Debugger,
    ParameterMode, RoutineRef, VariableKind, debug_break_id,
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
    FieldMeta, ForeignKeyMeta, IndexMeta, MatchKind, MetadataMatch, NamedRow, ObjectKind,
    ObjectMeta, Property, ReferencingKey, SEARCH_LIMIT, SchemaMeta, TableDetails, TextRow,
    TextRowExt, group_sorted, query_catalog, query_named, query_text, text_rows,
};
pub use metrics::{
    DatabaseSize, Metric, ReplicationRow, ServerMetrics, float_of, query_row, query_sizes,
};
pub use parameters::{
    ServerParameter, differs_from, same_value, set_value, sort_parameters, string_literal,
};
pub use security::{
    DropOptions, GrantAction, GrantKind, GrantTarget, Grantee, Principal, PrincipalFlag,
    PrincipalKind, PrincipalSpec, PrivilegeGrant, SecurityChange, TargetClass, memberships,
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
