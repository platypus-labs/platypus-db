//! The mongosh statements of the navigator's object actions, in place of the SQL that
//! `platypus_sql::Dialect` generates for the other databases.

use mongodb::bson::{Bson, Document, doc};
use platypus_core::ObjectKind;

use crate::literal::to_shell;

/// What to generate for a collection or view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement<'a> {
    Select { limit: Option<u64> },
    Insert,
    Update,
    Delete,
    Upsert,
    Drop,
    Truncate,
    Rename { new_name: &'a str },
}

fn plain(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// A collection in mongosh: `db.name`, `db.getCollection("other name")`, with
/// `getSiblingDB("db")` first when `db` is not the session's database.
#[must_use]
pub fn collection_ref(db: Option<&str>, name: &str) -> String {
    let quote = |s: &str| to_shell(&Bson::String(s.to_owned()));
    let base = db.map_or_else(
        || "db".to_owned(),
        |d| format!("db.getSiblingDB({})", quote(d)),
    );
    // Names that read as methods of the database need `getCollection` too.
    if plain(name) && !name.starts_with("get") {
        format!("{base}.{name}")
    } else {
        format!("{base}.getCollection({})", quote(name))
    }
}

/// A document with every field of `columns` but `_id` set to `null`.
fn placeholders(columns: &[&str]) -> String {
    let doc: Document = columns
        .iter()
        .filter(|&&c| c != "_id")
        .map(|&c| (c.to_owned(), Bson::Null))
        .collect();
    to_shell(&Bson::Document(doc))
}

/// The statement for `request`, or `None` when it does not apply to the object (views
/// cannot be written, renamed or emptied).
#[must_use]
pub fn statement(
    db: Option<&str>,
    name: &str,
    kind: ObjectKind,
    columns: &[&str],
    request: &Statement<'_>,
) -> Option<String> {
    let c = collection_ref(db, name);
    let writable = kind == ObjectKind::Table;
    let id = to_shell(&Bson::Document(doc! { "_id": Bson::Null }));
    Some(match request {
        Statement::Select { limit: Some(n) } => format!("{c}.find({{}}).limit({n});"),
        Statement::Select { limit: None } => format!("{c}.find({{}});"),
        Statement::Drop if matches!(kind, ObjectKind::Table | ObjectKind::View) => {
            format!("{c}.drop();")
        }
        _ if !writable => return None,
        Statement::Insert => format!("{c}.insertOne({});", placeholders(columns)),
        Statement::Update => format!(
            "{c}.updateOne({id}, {{ $set: {} }});",
            placeholders(columns)
        ),
        Statement::Upsert => format!(
            "{c}.updateOne({id}, {{ $set: {} }}, {{ upsert: true }});",
            placeholders(columns)
        ),
        Statement::Delete => format!("{c}.deleteOne({id});"),
        Statement::Truncate => format!("{c}.deleteMany({{}});"),
        Statement::Rename { new_name } => format!(
            "{c}.renameCollection({});",
            to_shell(&Bson::String((*new_name).to_owned()))
        ),
        Statement::Drop => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::{Command, parse};

    #[test]
    fn writes_collection_references() {
        assert_eq!(collection_ref(None, "users"), "db.users");
        assert_eq!(collection_ref(None, "a-b"), "db.getCollection(\"a-b\")");
        assert_eq!(
            collection_ref(None, "getName"),
            "db.getCollection(\"getName\")"
        );
        assert_eq!(
            collection_ref(Some("shop"), "users"),
            "db.getSiblingDB(\"shop\").users"
        );
    }

    #[test]
    fn generates_statements_that_parse() {
        let columns = ["_id", "name", "e-mail"];
        let all = [
            Statement::Select { limit: Some(200) },
            Statement::Select { limit: None },
            Statement::Insert,
            Statement::Update,
            Statement::Delete,
            Statement::Upsert,
            Statement::Drop,
            Statement::Truncate,
            Statement::Rename { new_name: "people" },
        ];
        for request in &all {
            let src =
                statement(Some("shop"), "users", ObjectKind::Table, &columns, request).unwrap();
            let parsed = parse(src.trim_end_matches(';'));
            assert!(
                matches!(parsed, Ok(Command::Collection { ref db, .. }) if db.as_deref() == Some("shop")),
                "{src}: {parsed:?}"
            );
        }
        assert_eq!(
            statement(
                None,
                "users",
                ObjectKind::Table,
                &columns,
                &Statement::Insert
            )
            .unwrap(),
            "db.users.insertOne({ name: null, \"e-mail\": null });"
        );
        assert!(statement(None, "v", ObjectKind::View, &columns, &Statement::Truncate).is_none());
        assert!(statement(None, "v", ObjectKind::View, &columns, &Statement::Drop).is_some());
    }
}
