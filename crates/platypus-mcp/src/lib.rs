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
mod agent;
mod connect;
mod context;
mod error;
mod ident;
mod library;
pub mod link;
pub mod output;
pub mod plan;
pub mod profile;
pub mod query;
mod redact;
pub mod resource;
mod server;
#[cfg(feature = "http")]
pub use server::http_service;
pub use server::{HELLO_EVERY, PlatypusServer};
pub mod stdio;

pub use access::AgentAccess;
pub use agent::{
    Agent, ConnectionInfo, ExplainArgs, MAX_PROPOSAL_BYTES, ProposeArgs, QueryArgs, SAMPLE_ROWS,
    SEARCH_RESULTS, SampleArgs, origin_name,
};
pub use connect::{AgentBlocker, agent_blocker, connect};
pub use context::{
    CONTEXT_RESULT_BYTES, CONTEXT_ROWS, CONTEXT_TEXT_BYTES, ContextArgs, SharedContext,
};
pub use error::{AgentError, Result};
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
