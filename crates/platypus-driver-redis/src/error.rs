//! `redis` errors to [`platypus_core::Error`].

use platypus_core::{DbError, Error};
use redis::{RedisError, ServerError};

/// A server's error reply: its code (`ERR`, `WRONGTYPE`, `NOPERM`…) and message.
pub fn server(e: &ServerError) -> Error {
    Error::Database(DbError {
        code: Some(e.code().to_owned()),
        message: e.details().unwrap_or_default().to_owned(),
        detail: None,
        hint: None,
        position: None,
    })
}

/// A client error: the server's reply as [`server`] makes it, a lost connection as
/// [`Error::Closed`] would hide its cause, so it keeps its message.
pub fn map(e: &RedisError) -> Error {
    if let Some(code) = e.code() {
        return Error::Database(DbError {
            code: Some(code.to_owned()),
            message: e.detail().unwrap_or_default().to_owned(),
            detail: None,
            hint: None,
            position: None,
        });
    }
    Error::Other(e.to_string())
}

/// Whether the server could not be reached.
pub fn is_network(e: &RedisError) -> bool {
    e.is_io_error() || e.is_connection_dropped() || e.is_unrecoverable_error()
}

/// A line of the editor that does not parse.
pub fn parse(line: &str, message: &str) -> Error {
    Error::Database(DbError {
        code: None,
        message: format!("{message}: {}", line.trim()),
        detail: None,
        hint: None,
        position: None,
    })
}
