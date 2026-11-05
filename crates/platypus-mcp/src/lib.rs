//! PlatypusDB for external agents (Claude Code, Codex, Cursor and other MCP clients, or
//! scripts through the `platypus` CLI).
//!
//! The [`Agent`] reads the connections saved in the app's profile ([`Profile`]) and runs
//! the tools on them: listing connections, schemas and objects, describing tables, their
//! DDL, searching objects and running one statement at a time within [`Limits`]. What an
//! agent may do on a connection is its [`AgentAccess`]: off, read-only (the default) or
//! read-write, never more than read-only on production and read-only connections.
//! Read-only statements are checked ([`access::check`]) and run in the database's read-only
//! transaction where it has one ([`platypus_sql::Dialect::read_only_begin`]). Passwords stay
//! in the keychain, and no tool or error shows a secret.

pub mod access;
mod across;
mod agent;
mod catalog_view;
mod compare;
mod connect;
mod context;
mod cursor;
mod er;
mod error;
mod estimate;
mod guard;
mod ident;
mod library;
pub mod link;
mod masking;
pub mod output;
pub mod plan;
pub mod policy;
pub mod profile;
pub mod query;
mod redact;
pub mod resource;
mod server;
mod server_views;
mod table_profile;
#[cfg(feature = "http")]
pub use server::http_service;
pub use server::{HELLO_EVERY, PlatypusServer};
pub mod stdio;

pub use access::AgentAccess;
pub use across::{AcrossArgs, AcrossSource};
pub use agent::{
    Agent, ConnectionInfo, ExplainArgs, MAX_PROPOSAL_BYTES, ProposeArgs, QueryArgs, SAMPLE_ROWS,
    SEARCH_RESULTS, SampleArgs, origin_name,
};
pub use compare::CompareArgs;
pub use connect::{AgentBlocker, agent_blocker, connect};
pub use context::{
    CONTEXT_RESULT_BYTES, CONTEXT_ROWS, CONTEXT_TEXT_BYTES, ContextArgs, SharedContext,
};
pub use cursor::PageArgs;
pub use error::{AgentError, Result};
pub use estimate::EstimateArgs;
pub use library::{
    HISTORY_PAGE, HistoryArgs, HistoryItem, ListScriptsArgs, MAX_HISTORY_PAGE, OriginArg,
    ReadScriptArgs, SCRIPT_BYTES, ScriptContent, ScriptInfo,
};
pub use link::{
    ANSWER_TIMEOUT, AppLink, BridgeFile, ContextPart, ContextRequest, ContextResult,
    ContextVerdict, HttpLink, OpenSession, UserContext, Verdict, WriteRequest,
};
pub use output::{Format, QueryResult};
pub use plan::PlanResult;
pub use profile::Profile;
pub use query::Limits;
pub use server_views::{STATEMENT_CHARS, ServerViewArgs};
pub use table_profile::ProfileArgs;
