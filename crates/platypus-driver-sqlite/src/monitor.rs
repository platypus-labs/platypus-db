//! The pragmas that matter for a SQLite database, with SQLite's documented defaults. There is
//! no server and no dashboard: the library has a handful of switches, each read with
//! `PRAGMA <name>`.

use platypus_core::{
    Connection, Result, ServerParameter, TextRowExt, differs_from, query_text, set_value,
};

/// Where a pragma's value lives, which says how far a change reaches.
#[derive(Clone, Copy)]
enum Scope {
    /// Per connection: it is gone when the connection closes.
    Connection,
    /// Stored in the database file, so every connection sees it.
    File,
    /// Computed by SQLite; it cannot be set.
    ReadOnly,
}

impl Scope {
    fn label(self) -> &'static str {
        match self {
            Scope::Connection => "connection",
            Scope::File => "database file",
            Scope::ReadOnly => "read only",
        }
    }
}

/// Pragma, its default (`None` where it depends on the build or on the database), scope and
/// what it is.
const PRAGMAS: &[(&str, Option<&str>, Scope, &str)] = &[
    (
        "analysis_limit",
        Some("0"),
        Scope::Connection,
        "Rows PRAGMA optimize and ANALYZE look at per index; 0 reads them all.",
    ),
    (
        "application_id",
        Some("0"),
        Scope::File,
        "A number an application stores in the file header to recognize its files.",
    ),
    (
        "auto_vacuum",
        Some("0"),
        Scope::File,
        "Whether freed pages are returned to the file: 0 none, 1 full, 2 incremental.",
    ),
    (
        "automatic_index",
        Some("1"),
        Scope::Connection,
        "Lets the planner build a temporary index for a join.",
    ),
    (
        "busy_timeout",
        Some("0"),
        Scope::Connection,
        "Milliseconds to wait for a lock held by another connection before failing.",
    ),
    (
        "cache_size",
        Some("-2000"),
        Scope::Connection,
        "Pages of cache per connection; a negative number is kibibytes.",
    ),
    (
        "cell_size_check",
        Some("0"),
        Scope::Connection,
        "Checks the size of every cell read, to catch a corrupt file early.",
    ),
    (
        "checkpoint_fullfsync",
        Some("0"),
        Scope::Connection,
        "Uses F_FULLFSYNC for checkpoints on systems that have it.",
    ),
    (
        "data_version",
        None,
        Scope::ReadOnly,
        "Changes when another connection commits to the database.",
    ),
    (
        "defer_foreign_keys",
        Some("0"),
        Scope::Connection,
        "Postpones foreign key checks to the commit of the transaction.",
    ),
    (
        "encoding",
        Some("UTF-8"),
        Scope::File,
        "How the file stores text: UTF-8, UTF-16le or UTF-16be.",
    ),
    (
        "foreign_keys",
        Some("0"),
        Scope::Connection,
        "Enforces foreign key constraints.",
    ),
    (
        "freelist_count",
        None,
        Scope::ReadOnly,
        "Pages of the file that are free.",
    ),
    (
        "fullfsync",
        Some("0"),
        Scope::Connection,
        "Uses F_FULLFSYNC for every sync on systems that have it.",
    ),
    (
        "ignore_check_constraints",
        Some("0"),
        Scope::Connection,
        "Does not enforce CHECK constraints.",
    ),
    (
        "journal_mode",
        Some("delete"),
        Scope::File,
        "How a transaction is logged: delete, truncate, persist, memory, wal or off.",
    ),
    (
        "journal_size_limit",
        Some("-1"),
        Scope::Connection,
        "Bytes of journal or WAL file kept after a transaction; -1 no limit.",
    ),
    (
        "locking_mode",
        Some("normal"),
        Scope::Connection,
        "normal releases the file lock after each transaction; exclusive keeps it.",
    ),
    (
        "mmap_size",
        Some("0"),
        Scope::Connection,
        "Bytes of the file read through memory mapping; 0 turns it off.",
    ),
    ("page_count", None, Scope::ReadOnly, "Pages in the file."),
    (
        "page_size",
        Some("4096"),
        Scope::File,
        "Bytes per page; it applies to a new or vacuumed file.",
    ),
    (
        "query_only",
        Some("0"),
        Scope::Connection,
        "Refuses statements that change the database.",
    ),
    (
        "read_uncommitted",
        Some("0"),
        Scope::Connection,
        "Lets a shared-cache connection read what others did not commit.",
    ),
    (
        "recursive_triggers",
        Some("0"),
        Scope::Connection,
        "Lets a trigger fire itself.",
    ),
    (
        "secure_delete",
        Some("0"),
        Scope::Connection,
        "Overwrites deleted content with zeros.",
    ),
    (
        "synchronous",
        Some("2"),
        Scope::Connection,
        "How often SQLite waits for the disk: 0 off, 1 normal, 2 full, 3 extra.",
    ),
    (
        "temp_store",
        Some("0"),
        Scope::Connection,
        "Where temporary tables live: 0 the build's choice, 1 file, 2 memory.",
    ),
    (
        "threads",
        Some("0"),
        Scope::Connection,
        "Helper threads a statement may use for sorting; 0 none.",
    ),
    (
        "trusted_schema",
        Some("1"),
        Scope::Connection,
        "Trusts the functions and virtual tables the schema uses.",
    ),
    (
        "user_version",
        Some("0"),
        Scope::File,
        "A number an application stores in the file header, such as a schema version.",
    ),
    (
        "wal_autocheckpoint",
        Some("1000"),
        Scope::Connection,
        "Pages of WAL after which a commit checkpoints it.",
    ),
];

pub(crate) async fn parameters(conn: &dyn Connection) -> Result<Vec<ServerParameter>> {
    let mut out = Vec::with_capacity(PRAGMAS.len());
    for (name, default, scope, description) in PRAGMAS {
        // A pragma this SQLite build does not know returns no row: it is left out.
        let Ok(rows) = query_text(conn, &format!("PRAGMA {name}")).await else {
            continue;
        };
        let Some(value) = rows.first().and_then(|r| r.opt(0)) else {
            continue;
        };
        out.push(ServerParameter {
            changed: differs_from(Some(&value), *default),
            statement: set_pragma(name, &value, *scope),
            name: (*name).to_owned(),
            default: default.map(str::to_owned),
            source: Some("pragma".to_owned()),
            context: Some(scope.label().to_owned()),
            description: Some((*description).to_owned()),
            value: Some(value),
            ..ServerParameter::default()
        });
    }
    Ok(out)
}

/// `PRAGMA name = value;`, for the pragmas that can be set.
fn set_pragma(name: &str, value: &str, scope: Scope) -> Option<String> {
    match scope {
        Scope::ReadOnly => None,
        _ => Some(format!("PRAGMA {name} = {};", set_value(value, false))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_pragmas_have_no_statement() {
        assert_eq!(
            set_pragma("foreign_keys", "1", Scope::Connection).unwrap(),
            "PRAGMA foreign_keys = 1;"
        );
        assert_eq!(
            set_pragma("journal_mode", "wal", Scope::File).unwrap(),
            "PRAGMA journal_mode = 'wal';"
        );
        assert!(set_pragma("page_count", "3", Scope::ReadOnly).is_none());
    }

    #[test]
    fn the_list_is_sorted_and_without_repeats() {
        let names: Vec<&str> = PRAGMAS.iter().map(|p| p.0).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names, sorted);
    }
}
