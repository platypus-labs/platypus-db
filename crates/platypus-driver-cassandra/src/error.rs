//! The driver's errors as the app's: the server's message and code for a statement it
//! refused, a connection error for a node that cannot be reached.

use std::error::Error as StdError;

use platypus_core::{DbError, Error};
use scylla::errors::{
    DbError as ServerError, ExecutionError, RequestAttemptError, UseKeyspaceError,
};

/// The error for something that went wrong while a statement ran.
pub fn from_driver(error: &(dyn StdError + 'static)) -> Error {
    if let Some(attempt) = attempt_of(error) {
        return from_attempt(attempt);
    }
    Error::Other(platypus_core::without_repeats(&chain(error)))
}

/// The server's answer in `error` or one of its causes. The driver's transparent wrappers
/// hide the error inside from `source`, so they are opened by name.
fn attempt_of<'a>(error: &'a (dyn StdError + 'static)) -> Option<&'a RequestAttemptError> {
    let mut current = Some(error);
    while let Some(e) = current {
        if let Some(attempt) = e.downcast_ref::<RequestAttemptError>() {
            return Some(attempt);
        }
        if let Some(ExecutionError::LastAttemptError(attempt)) = e.downcast_ref::<ExecutionError>()
        {
            return Some(attempt);
        }
        if let Some(UseKeyspaceError::RequestError(attempt)) = e.downcast_ref::<UseKeyspaceError>()
        {
            return Some(attempt);
        }
        current = e.source();
    }
    None
}

/// `a: b: c` for the causes of `error`.
fn chain(error: &(dyn StdError + 'static)) -> String {
    let mut out = error.to_string();
    let mut current = error.source();
    while let Some(e) = current {
        out.push_str(": ");
        out.push_str(&e.to_string());
        current = e.source();
    }
    out
}

fn from_attempt(attempt: &RequestAttemptError) -> Error {
    match attempt {
        RequestAttemptError::DbError(db, message) => Error::Database(DbError {
            code: Some(code(db)),
            message: message.clone(),
            detail: None,
            hint: None,
            position: None,
        }),
        other => Error::Other(platypus_core::without_repeats(&chain(other))),
    }
}

/// The server's error kind as a short code (`SyntaxError`, `Unauthorized`, `AlreadyExists`).
fn code(db: &ServerError) -> String {
    let debug = format!("{db:?}");
    debug
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// A connection that failed, with the cause the driver gives.
pub fn connect_error(error: &(dyn StdError + 'static)) -> Error {
    if let Some(RequestAttemptError::DbError(db, message)) = attempt_of(error) {
        return Error::Connect(format!("{}: {message}", code(db)));
    }
    Error::Connect(platypus_core::without_repeats(&chain(error)))
}

/// A statement of the editor that does not parse.
pub fn parse_error(message: &str) -> Error {
    Error::Database(DbError {
        code: None,
        message: message.to_owned(),
        detail: None,
        hint: None,
        position: None,
    })
}

/// Whether the error says the node cannot be reached, so the session is as good as closed.
pub fn is_connection_loss(error: &Error) -> bool {
    matches!(error, Error::Connect(_))
        || matches!(error, Error::Other(m) if m.contains("onnection") || m.contains("No connections in the pool"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_kind_of_server_error() {
        assert_eq!(code(&ServerError::SyntaxError), "SyntaxError");
        assert_eq!(code(&ServerError::Unauthorized), "Unauthorized");
        let e = from_attempt(&RequestAttemptError::DbError(
            ServerError::Invalid,
            "bad".into(),
        ));
        assert!(
            matches!(&e, Error::Database(d) if d.code.as_deref() == Some("Invalid") && d.message == "bad")
        );
    }
}
