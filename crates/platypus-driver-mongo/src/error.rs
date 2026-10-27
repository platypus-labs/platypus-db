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
        ErrorKind::ServerSelection { message, .. } => server_selection(message),
        ErrorKind::Authentication { message, .. }
        | ErrorKind::InvalidArgument { message, .. }
        | ErrorKind::DnsResolve { message, .. }
        | ErrorKind::InvalidTlsConfig { message, .. } => message.clone(),
        ErrorKind::Io(io) => io.to_string(),
        _ => e.to_string(),
    }
}

/// A server selection failure without the topology the driver describes after it, but for
/// the first server's error: `Server selection timeout: No available servers. Topology:
/// { Type: Unknown, Servers: [ { Address: h:27017, Type: Unknown, Error: Kind: I/O error:
/// …, labels: {} } ] }` gives `Server selection timeout: No available servers (h:27017:
/// I/O error: …)`.
fn server_selection(message: &str) -> String {
    let Some((head, topology)) = message.split_once("Topology:") else {
        return message.to_owned();
    };
    let head = head.trim_end().trim_end_matches(['.', ',']);
    let field = |name: &str| {
        let start = topology.find(name)? + name.len();
        let rest = &topology[start..];
        let end = [", labels:", ", Type:", ", Error:", " }"]
            .iter()
            .filter_map(|stop| rest.find(stop))
            .min()
            .unwrap_or(rest.len());
        Some(rest[..end].trim().trim_start_matches("Kind: ").to_owned())
    };
    match (field("Address: "), field("Error: ")) {
        (Some(address), Some(error)) => format!("{head} ({address}: {error})"),
        (None, Some(error)) => format!("{head} ({error})"),
        _ => head.to_owned(),
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

#[cfg(test)]
mod tests {
    use super::server_selection;

    #[test]
    fn server_selection_errors_leave_out_the_topology() {
        let message = "Server selection timeout: No available servers. Topology: { Type: \
                       Unknown, Servers: [ { Address: localhost:57017, Type: Unknown, Error: \
                       Kind: I/O error: received corrupt message, labels: {} } ] }";
        assert_eq!(
            server_selection(message),
            "Server selection timeout: No available servers (localhost:57017: I/O error: \
             received corrupt message)"
        );
        assert_eq!(server_selection("no topology"), "no topology");
    }
}
