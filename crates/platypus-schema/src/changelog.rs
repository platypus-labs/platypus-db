//! A schema comparison script as a migration file for a migration tool: a Flyway versioned
//! migration or a Liquibase "formatted SQL" changelog.

use std::fmt::Write;

use platypus_sql::Dialect;
use serde::{Deserialize, Serialize};

use crate::Script;

/// The migration tool whose file [`changelog`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangelogFormat {
    /// A versioned SQL migration (`V<version>__<description>.sql`).
    Flyway,
    /// A changelog in Liquibase's "formatted SQL" syntax.
    Liquibase,
}

/// The text of the migration file for `script`.
///
/// `description` heads a Flyway file and comments the first Liquibase changeset; `author` and
/// `id` name the Liquibase changesets (`<id>-<n>`, one per statement). The statements are not
/// wrapped in `BEGIN`/`COMMIT` as [`Script::text`] does: both tools run each migration in a
/// transaction of their own where the database allows it. `dialect` is not needed to write
/// either format today, but it is part of the signature so that a quirk of one database can
/// be added without changing the callers.
#[must_use]
pub fn changelog(
    format: ChangelogFormat,
    _dialect: Dialect,
    script: &Script,
    description: &str,
    author: &str,
    id: &str,
) -> String {
    match format {
        ChangelogFormat::Flyway => flyway(script, description),
        ChangelogFormat::Liquibase => liquibase(script, description, author, id),
    }
}

/// A `-- ` comment line per line of `text`, so a multi-line text never leaves a bare SQL line.
fn comment(out: &mut String, text: &str) {
    for line in text.lines() {
        let _ = writeln!(out, "-- {line}");
    }
}

fn flyway(script: &Script, description: &str) -> String {
    let mut out = String::new();
    comment(&mut out, description);
    if !script.before.is_empty() || !script.after.is_empty() {
        comment(
            &mut out,
            "Runs outside a transaction (executeInTransaction=false in the .conf file next to \
             it): SQLite ignores PRAGMA foreign_keys inside one.",
        );
    }
    for w in &script.warnings {
        comment(&mut out, w);
    }
    for s in script
        .before
        .iter()
        .chain(&script.statements)
        .chain(&script.after)
    {
        let _ = writeln!(out, "{s};");
    }
    out
}

fn liquibase(script: &Script, description: &str, author: &str, id: &str) -> String {
    let mut out = String::from("-- liquibase formatted sql\n");
    for w in &script.warnings {
        comment(&mut out, w);
    }
    // The statements that run outside the transaction are those of `before` and `after`
    // (SQLite's `PRAGMA foreign_keys`, which a transaction silently ignores). A script that is
    // not `transactional` (MySQL) needs no flag: MySQL commits each DDL statement by itself.
    let all = script
        .before
        .iter()
        .map(|s| (s, false))
        .chain(script.statements.iter().map(|s| (s, true)))
        .chain(script.after.iter().map(|s| (s, false)));
    for (n, (statement, in_transaction)) in all.enumerate() {
        // One statement per changeset, so the tool must not split a trigger body at its `;`.
        let _ = write!(
            out,
            "\n-- changeset {author}:{id}-{} splitStatements:false",
            n + 1
        );
        if !in_transaction {
            out.push_str(" runInTransaction:false");
        }
        out.push('\n');
        if n == 0 {
            let first = description.lines().next().unwrap_or_default();
            if !first.is_empty() {
                let _ = writeln!(out, "--comment: {first}");
            }
        }
        let _ = writeln!(out, "{statement};");
    }
    out
}

/// The name of a Flyway versioned migration: `V<version>__<description>.sql`, the description
/// with every run of characters other than letters and digits as one `_`.
#[must_use]
pub fn flyway_file_name(version: &str, description: &str) -> String {
    let mut words = String::new();
    for c in description.chars() {
        if c.is_ascii_alphanumeric() {
            words.push(c);
        } else if !words.ends_with('_') {
            words.push('_');
        }
    }
    let words = words.trim_matches('_');
    format!(
        "V{version}__{}.sql",
        if words.is_empty() { "changes" } else { words }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(before: &[&str], statements: &[&str], after: &[&str], transactional: bool) -> Script {
        let owned = |l: &[&str]| l.iter().map(|s| (*s).to_owned()).collect();
        Script {
            before: owned(before),
            statements: owned(statements),
            after: owned(after),
            transactional,
            warnings: vec![],
        }
    }

    fn sqlite() -> Script {
        script(
            &["PRAGMA foreign_keys = OFF"],
            &["CREATE TABLE a (id INTEGER)", "DROP TABLE b"],
            &["PRAGMA foreign_keys = ON"],
            true,
        )
    }

    #[test]
    fn flyway_has_no_transaction_wrapper() {
        let text = changelog(
            ChangelogFormat::Flyway,
            Dialect::Sqlite,
            &sqlite(),
            "make b like a",
            "me",
            "1",
        );
        assert_eq!(
            text,
            "-- make b like a\n-- Runs outside a transaction (executeInTransaction=false in the .conf \
             file next to it): SQLite ignores PRAGMA foreign_keys inside one.\nPRAGMA foreign_keys = OFF;\nCREATE TABLE a (id INTEGER);\nDROP TABLE b;\nPRAGMA foreign_keys = ON;\n"
        );
        assert!(!text.contains("BEGIN") && !text.contains("COMMIT"));
    }

    #[test]
    fn flyway_lists_warnings_after_the_header() {
        let mut s = script(&[], &["DROP TABLE b"], &[], true);
        s.warnings = vec!["drops data".to_owned()];
        let text = changelog(
            ChangelogFormat::Flyway,
            Dialect::Postgres,
            &s,
            "d",
            "me",
            "1",
        );
        assert_eq!(text, "-- d\n-- drops data\nDROP TABLE b;\n");
    }

    #[test]
    fn liquibase_runs_pragmas_outside_a_transaction() {
        let text = changelog(
            ChangelogFormat::Liquibase,
            Dialect::Sqlite,
            &sqlite(),
            "make b like a",
            "platypusdb",
            "2026",
        );
        assert_eq!(
            text,
            "-- liquibase formatted sql\n\
             \n-- changeset platypusdb:2026-1 splitStatements:false runInTransaction:false\n--comment: make b like a\nPRAGMA foreign_keys = OFF;\n\
             \n-- changeset platypusdb:2026-2 splitStatements:false\nCREATE TABLE a (id INTEGER);\n\
             \n-- changeset platypusdb:2026-3 splitStatements:false\nDROP TABLE b;\n\
             \n-- changeset platypusdb:2026-4 splitStatements:false runInTransaction:false\nPRAGMA foreign_keys = ON;\n"
        );
    }

    #[test]
    fn liquibase_on_postgres_comments_the_first_changeset_and_lists_warnings_first() {
        let mut s = script(&[], &["CREATE TABLE a (id int)", "DROP TABLE b"], &[], true);
        s.warnings = vec!["drops data".to_owned()];
        let text = changelog(
            ChangelogFormat::Liquibase,
            Dialect::Postgres,
            &s,
            "d",
            "me",
            "x",
        );
        assert_eq!(
            text,
            "-- liquibase formatted sql\n-- drops data\n\
             \n-- changeset me:x-1 splitStatements:false\n--comment: d\nCREATE TABLE a (id int);\n\
             \n-- changeset me:x-2 splitStatements:false\nDROP TABLE b;\n"
        );
        assert!(!text.contains("runInTransaction"));
    }

    #[test]
    fn mysql_needs_no_transaction_flag() {
        let s = script(&[], &["ALTER TABLE a ADD b int"], &[], false);
        let text = changelog(
            ChangelogFormat::Liquibase,
            Dialect::MySql,
            &s,
            "d",
            "me",
            "x",
        );
        assert!(!text.contains("runInTransaction"));
    }

    #[test]
    fn empty_script() {
        let s = Script::default();
        assert_eq!(
            changelog(
                ChangelogFormat::Flyway,
                Dialect::Sqlite,
                &s,
                "nothing",
                "me",
                "1"
            ),
            "-- nothing\n"
        );
        assert_eq!(
            changelog(
                ChangelogFormat::Liquibase,
                Dialect::Sqlite,
                &s,
                "nothing",
                "me",
                "1"
            ),
            "-- liquibase formatted sql\n"
        );
    }

    #[test]
    fn flyway_file_names() {
        assert_eq!(
            flyway_file_name("20260101120000", "make b like a"),
            "V20260101120000__make_b_like_a.sql"
        );
        assert_eq!(
            flyway_file_name("1", "  --Make: \"main\" → main!  "),
            "V1__Make_main_main.sql"
        );
        assert_eq!(flyway_file_name("1", ""), "V1__changes.sql");
        assert_eq!(flyway_file_name("1", "***"), "V1__changes.sql");
    }
}
