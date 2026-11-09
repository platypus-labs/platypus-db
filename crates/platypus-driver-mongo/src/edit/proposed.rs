//! Documents an agent proposes to change (`propose_edits`), checked by the rules of the grid's
//! save before the data viewer opens them: the `find` that reads the documents the proposal
//! names, and each `_id` as the grid shows it, for the app to find the rows by.
//!
//! An `_id` arrives as text and is read by the type of the collection's `_id` field: in an
//! `objectId` collection a 24-hex string is an `ObjectId`, in an `int` one `"7"` is 7.

use std::collections::HashSet;

use mongodb::bson::{Bson, doc};

use crate::docs;
use crate::generate::collection_ref;
use crate::literal::to_shell;

use super::{cell, id_value};

/// The data viewer's statement for a proposal, and the `_id`s it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedFind {
    /// The collection as the statement addresses it, with its database:
    /// `db.getSiblingDB("shop").orders`.
    pub collection: String,
    /// `<collection>.find({ _id: { $in: [ … ] } })`, or `find({})` without `_id`s.
    pub statement: String,
    /// The `_id`s, in their order, as the grid shows them: an `ObjectId` in lower-case hex, a
    /// number without the zeros and signs that do not change it.
    pub ids: Vec<String>,
}

/// The `find` of the documents of `collection` in database `db` whose `_id` is one of `ids`,
/// each read by `id_type`, the type of the collection's `_id` field (its `META_DB_TYPE`). Each
/// `_id` is written as a mongosh literal of that type (`ObjectId("…")`, a number, a string).
///
/// # Errors
/// An `_id` that is not a value of `id_type`, a type the grid cannot write back (`mixed`,
/// `binData`), or a document named twice.
pub fn find_proposed(
    db: &str,
    collection: &str,
    id_type: &str,
    ids: &[&str],
) -> Result<ProposedFind, String> {
    let values = ids
        .iter()
        .map(|id| id_value(id_type, id))
        .collect::<Result<Vec<_>, _>>()?;
    let shown: Vec<String> = values
        .iter()
        .map(|v| docs::text(v).unwrap_or_default())
        .collect();
    let mut seen = HashSet::new();
    if let Some(twice) = shown.iter().find(|id| !seen.insert(id.as_str())) {
        return Err(format!(
            "the document with _id {twice} is named twice: each document is changed or deleted once"
        ));
    }
    let filter = if values.is_empty() {
        doc! {}
    } else {
        doc! { "_id": { "$in": values } }
    };
    let target = collection_ref(Some(db), collection);
    Ok(ProposedFind {
        statement: format!("{target}.find({})", to_shell(&Bson::Document(filter))),
        collection: target,
        ids: shown,
    })
}

/// Checks a value an agent proposes for field `field`, of type `db_type`, by the rules of the
/// grid's save: a field `$set` takes as it is, of a type the grid writes back, and a value of
/// that type (`None` is null). An `_id` is read as one, which only a new document gives.
///
/// # Errors
/// What is wrong, naming the field.
pub fn check_proposed_cell(field: &str, db_type: &str, text: Option<&str>) -> Result<(), String> {
    if field == "_id" {
        return text.map_or(Ok(()), |t| id_value(db_type, t).map(drop));
    }
    cell(field, db_type, text).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "65a1b2c3d4e5f60718293a4b";

    #[test]
    fn object_ids_are_found_as_object_ids_and_shown_in_lower_case() {
        let upper = ID.to_ascii_uppercase();
        let found = find_proposed(
            "shop",
            "orders",
            "objectId",
            &[&upper, "65a1b2c3d4e5f60718293a4c"],
        )
        .unwrap();
        assert_eq!(found.collection, r#"db.getSiblingDB("shop").orders"#);
        assert_eq!(
            found.statement,
            format!(
                r#"db.getSiblingDB("shop").orders.find({{ _id: {{ $in: [ObjectId("{ID}"), ObjectId("65a1b2c3d4e5f60718293a4c")] }} }})"#
            )
        );
        assert_eq!(found.ids, [ID, "65a1b2c3d4e5f60718293a4c"]);
        // The shell reads it as a plain `find`, whose documents the grid edits.
        let source = crate::edit::source(&found.statement).unwrap();
        assert_eq!(
            (source.db.as_deref(), source.collection.as_str()),
            (Some("shop"), "orders")
        );
        let wrong = find_proposed("shop", "orders", "objectId", &["7"]).unwrap_err();
        assert!(wrong.contains("ObjectId"), "{wrong}");
    }

    #[test]
    fn numbers_and_strings_keep_their_type() {
        let ints = find_proposed("shop", "orders", "int", &["007", "-3"]).unwrap();
        assert!(
            ints.statement.ends_with("find({ _id: { $in: [7, -3] } })"),
            "{}",
            ints.statement
        );
        assert_eq!(ints.ids, ["7", "-3"]);
        let longs = find_proposed("shop", "orders", "long", &["12"]).unwrap();
        assert!(
            longs.statement.contains("NumberLong(12)"),
            "{}",
            longs.statement
        );
        assert!(find_proposed("shop", "orders", "int", &["1.5"]).is_err());
        let text = find_proposed("shop", "my orders", "string", &["a\"b", "7"]).unwrap();
        assert_eq!(
            text.statement,
            r#"db.getSiblingDB("shop").getCollection("my orders").find({ _id: { $in: ["a\"b", "7"] } })"#
        );
        assert_eq!(text.ids, ["a\"b", "7"]);
    }

    #[test]
    fn only_inserts_read_the_whole_collection() {
        let all = find_proposed("shop", "orders", "objectId", &[]).unwrap();
        assert_eq!(all.statement, r#"db.getSiblingDB("shop").orders.find({})"#);
        assert_eq!(all.ids, Vec::<String>::new());
    }

    #[test]
    fn what_the_grid_cannot_write_is_refused() {
        let mixed = find_proposed("shop", "orders", "mixed", &["1"]).unwrap_err();
        assert!(mixed.contains("several types"), "{mixed}");
        let twice = find_proposed("shop", "orders", "int", &["1", "01"]).unwrap_err();
        assert!(twice.contains("named twice"), "{twice}");
        assert!(check_proposed_cell("bytes", "binData", Some("x")).is_err());
        assert!(check_proposed_cell("a.b", "string", Some("x")).is_err());
        assert!(check_proposed_cell("$a", "string", Some("x")).is_err());
        assert!(check_proposed_cell("n", "int", Some("many")).is_err());
        assert!(check_proposed_cell("n", "int", Some("5")).is_ok());
        assert!(check_proposed_cell("n", "int", None).is_ok());
        assert!(check_proposed_cell("when", "date", Some("2024-01-02 10:30:00")).is_ok());
        assert!(check_proposed_cell("_id", "objectId", Some(ID)).is_ok());
        assert!(check_proposed_cell("_id", "objectId", Some("x")).is_err());
        assert!(check_proposed_cell("_id", "objectId", None).is_ok());
    }
}
