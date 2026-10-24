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
mod error;
pub mod output;
pub mod profile;
pub mod query;
mod redact;

pub use access::AgentAccess;
pub use agent::{Agent, ConnectionInfo, QueryArgs, SEARCH_RESULTS};
pub use connect::connect;
pub use error::{AgentError, Result};
pub use output::{Format, QueryResult};
pub use profile::Profile;
pub use query::Limits;
