//! `mongodb` errors to [`platypus_core::Error`].

use mongodb::error::{ErrorKind, WriteFailure};
use platypus_core::{DbError, Error};

use crate::literal::ParseError;

/// The message of an error without the driver's labels and source chain.
pub fn message(e: &mongodb::error::Error) -> String {
    match &*e.kind {
        ErrorKind::Command(c) => c.message.clone(),
        ErrorKind::Write(WriteFailure::WriteError(w)) => w.message.clone(),
        ErrorKind::Write(WriteFailure::WriteConcernError(w)) => w.message.clone(),
        ErrorKind::Authentication { message, .. }
        | ErrorKind::ServerSelection { message, .. }
        | ErrorKind::InvalidArgument { message, .. }
        | ErrorKind::DnsResolve { message, .. }
        | ErrorKind::InvalidTlsConfig { message, .. } => message.clone(),
        ErrorKind::Io(io) => io.to_string(),
        _ => e.to_string(),
    }
}

/// Whether the server could not be reached.
pub fn is_network(e: &mongodb::error::Error) -> bool {
    matches!(
        &*e.kind,
        ErrorKind::Io(_)
            | ErrorKind::ServerSelection { .. }
            | ErrorKind::ConnectionPoolCleared { .. }
    )
}

fn db_error(code: Option<String>, message: String) -> Error {
    Error::Database(DbError {
        code,
        message,
        ..DbError::default()
    })
}

pub fn map_error(e: &mongodb::error::Error) -> Error {
    match &*e.kind {
        ErrorKind::Command(c) => db_error(
            Some(if c.code_name.is_empty() {
                c.code.to_string()
            } else {
                c.code_name.clone()
            }),
            c.message.clone(),
        ),
        ErrorKind::Write(WriteFailure::WriteError(w)) => db_error(
            Some(w.code_name.clone().unwrap_or_else(|| w.code.to_string())),
            w.message.clone(),
        ),
        ErrorKind::Write(WriteFailure::WriteConcernError(w)) => {
            db_error(Some(w.code_name.clone()), w.message.clone())
        }
        ErrorKind::InsertMany(many) => {
            let first = many.write_errors.as_ref().and_then(|w| w.first());
            match first {
                Some(w) => db_error(
                    Some(w.code_name.clone().unwrap_or_else(|| w.code.to_string())),
                    format!("document {}: {}", w.index, w.message),
                ),
                None => Error::Other(e.to_string()),
            }
        }
        ErrorKind::Authentication { message, .. } => Error::Connect(message.clone()),
        _ => Error::Other(message(e)),
    }
}

/// A syntax error of a statement, with its 1-based character position.
pub fn parse_error(src: &str, e: &ParseError) -> Error {
    // An offset inside a character counts the characters before it.
    let offset = e.offset.min(src.len());
    let chars = src.char_indices().take_while(|&(i, _)| i < offset).count();
    Error::Database(DbError {
        code: Some("SyntaxError".to_owned()),
        message: e.message.clone(),
        position: u32::try_from(chars + 1).ok(),
        ..DbError::default()
    })
}
