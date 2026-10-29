//! `redis` errors to [`platypus_core::Error`].

use platypus_core::{DbError, Error, Result};
use redis::{RedisError, ServerError, Value};

fn database(code: Option<&str>, message: String) -> Error {
    Error::Database(DbError {
        code: code.map(str::to_owned),
        message,
        detail: None,
        hint: None,
        position: None,
    })
}

/// A server's error reply: its code (`ERR`, `WRONGTYPE`, `NOPERM`…) and message.
pub fn server(e: &ServerError) -> Error {
    database(Some(e.code()), e.details().unwrap_or_default().to_owned())
}

/// A client error: the server's reply as [`server`] makes it, a lost connection as
/// [`Error::Closed`] would hide its cause, so it keeps its message.
pub fn map(e: &RedisError) -> Error {
    match e.code() {
        Some(code) => database(Some(code), e.detail().unwrap_or_default().to_owned()),
        None => Error::Other(e.to_string()),
    }
}

/// A reply, an error reply as [`server`] makes it.
pub fn reply(value: Value) -> Result<Value> {
    match value {
        Value::ServerError(e) => Err(server(&e)),
        value => Ok(value),
    }
}

/// Whether the server could not be reached.
pub fn is_network(e: &RedisError) -> bool {
    e.is_io_error() || e.is_connection_dropped() || e.is_unrecoverable_error()
}

/// A line of the editor that does not parse.
pub fn parse(line: &str, message: &str) -> Error {
    database(None, format!("{message}: {}", line.trim()))
}
