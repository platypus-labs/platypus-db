//! The init statements of a connection in a shared project.
//!
//! The project file anyone with write access can edit may carry SQL that runs at every
//! connect. This machine runs it only after its user read that exact text: the approval is
//! kept on this machine (never in the shared file), by connection and a hash of the text, so
//! changed text asks again.

use std::fmt::Write as _;

use ring::digest::{SHA256, digest};
use uuid::Uuid;

/// The setting that holds the approved hash of a connection's init statements. The `shared`
/// section stays on this machine: it is never synced.
#[must_use]
pub fn approval_key(connection: Uuid) -> String {
    format!("shared.init_approved.{connection}")
}

/// A hash of the statements, blind to the line endings and the blank edges an editor or a
/// Git checkout changes.
#[must_use]
pub fn init_hash(statements: &str) -> String {
    let text = statements.replace("\r\n", "\n");
    let hash = digest(&SHA256, text.trim().as_bytes());
    hash.as_ref().iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Whether connecting must first ask: the statements come from a shared project (`shared`),
/// are not empty, and are not the text `approved` (the hash kept for the connection).
/// Statements the user wrote on this machine never ask.
#[must_use]
pub fn needs_approval(shared: bool, statements: &str, approved: Option<&str>) -> bool {
    shared && !statements.trim().is_empty() && approved != Some(init_hash(statements).as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_text_is_approved_and_changed_text_asks_again() {
        let sql = "SET search_path = app;\nSET statement_timeout = 5000;";
        let approved = init_hash(sql);
        assert!(needs_approval(true, sql, None));
        assert!(!needs_approval(true, sql, Some(&approved)));
        assert!(needs_approval(true, "DROP TABLE t;", Some(&approved)));
        assert!(needs_approval(
            true,
            &format!("{sql}\nSELECT 1;"),
            Some(&approved)
        ));
    }

    #[test]
    fn line_endings_and_blank_edges_do_not_change_the_hash() {
        assert_eq!(init_hash("A;\nB;"), init_hash("\r\nA;\r\nB;\r\n\r\n"));
        assert_ne!(init_hash("A;"), init_hash("B;"));
    }

    #[test]
    fn local_statements_and_empty_ones_never_ask() {
        assert!(!needs_approval(false, "SET x = 1;", None));
        assert!(!needs_approval(true, "  \n ", None));
    }

    #[test]
    fn the_key_names_the_connection_in_the_machine_section() {
        let id = Uuid::new_v4();
        let key = approval_key(id);
        assert!(key.starts_with("shared.") && key.ends_with(&id.to_string()));
        assert!(crate::collect::is_machine_setting(&key));
    }
}
