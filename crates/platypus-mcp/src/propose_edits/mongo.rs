//! `propose_edits` on MongoDB: the documents of a collection are found by `_id` alone, and the
//! values are checked by the rules of the grid's save ([`platypus_driver_mongo::edit`]), so
//! that the data viewer opens only what it can save.

use platypus_core::TableDetails;
use platypus_driver_mongo::edit::{check_proposed_cell, find_proposed};
use platypus_sql::proposed::{self, Edits, Row, Viewer};

/// What the data viewer opens rows proposed on a MongoDB collection with: the `find` of the
/// documents the updates and deletes name (the whole collection with only inserts), the
/// collection as that statement addresses it, and `edits` with the `_id` of each key written
/// as the grid shows it (an `ObjectId` in lower-case hex), which the app finds the rows by.
///
/// An `_id` is read by the type the collection's `_id` field has in `details`: in an
/// `objectId` collection its 24 hexadecimal digits are an `ObjectId`.
///
/// # Errors
/// When `edits` do not [`proposed::validate`] with `_id` as the only key, change an `_id`,
/// name a field or a value the grid cannot write, or name a document twice.
pub fn mongo_viewer(edits: &Edits, details: &TableDetails) -> Result<(Viewer, Edits), String> {
    // A document is found by its `_id`, never by the fields of a unique index.
    let by_id = TableDetails {
        columns: details.columns.clone(),
        ..TableDetails::default()
    };
    proposed::validate(edits, &by_id)?;
    let type_of = |field: &str| {
        details
            .columns
            .iter()
            .find(|c| c.name == field)
            .map_or("", |c| c.data_type.as_str())
    };
    let check = |what: &str, row: &Row| {
        row.iter().try_for_each(|(field, value)| {
            check_proposed_cell(field, type_of(field), value.as_deref())
                .map_err(|e| format!("{what}: {e}"))
        })
    };
    for (i, row) in edits.inserts.iter().enumerate() {
        check(&format!("inserts[{i}]"), row)?;
    }
    for (i, update) in edits.updates.iter().enumerate() {
        if update.set.iter().any(|(field, _)| field == "_id") {
            return Err(format!(
                "updates[{i}].set changes _id, which MongoDB never changes: delete the document \
                 and insert it again"
            ));
        }
        check(&format!("updates[{i}].set"), &update.set)?;
    }
    // Validated: each key is `_id` alone, not null.
    let ids: Vec<&str> = edits
        .updates
        .iter()
        .map(|u| &u.key)
        .chain(&edits.deletes)
        .flat_map(|key| key.iter().filter_map(|(_, v)| v.as_deref()))
        .collect();
    let found = find_proposed(&edits.schema, &edits.table, type_of("_id"), &ids)?;
    let mut out = edits.clone();
    let keys = out
        .updates
        .iter_mut()
        .map(|u| &mut u.key)
        .chain(out.deletes.iter_mut());
    for (cell, shown) in keys.flatten().zip(found.ids) {
        cell.1 = Some(shown);
    }
    let viewer = Viewer {
        sql: found.statement,
        table: found.collection,
    };
    Ok((viewer, out))
}

#[cfg(test)]
mod tests {
    use platypus_core::{ColumnMeta, IndexMeta};
    use platypus_sql::proposed::Update;

    use super::*;

    const ID: &str = "65a1b2c3d4e5f60718293a4b";

    fn field(name: &str, data_type: &str) -> ColumnMeta {
        ColumnMeta {
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            nullable: name != "_id",
            default: None,
            primary_key: name == "_id",
            comment: None,
            enum_values: Vec::new(),
            fields: Vec::new(),
            hidden: false,
        }
    }

    /// `orders`: an `ObjectId` `_id`, a unique `code`, a number, a binary field.
    fn orders(id_type: &str) -> TableDetails {
        TableDetails {
            columns: vec![
                field("_id", id_type),
                field("code", "string"),
                field("qty", "int"),
                field("scan", "binData"),
            ],
            indexes: vec![IndexMeta {
                name: "code_1".to_owned(),
                columns: vec!["code".to_owned()],
                unique: true,
                primary: false,
                partial: false,
                size: None,
                detail: None,
            }],
            ..TableDetails::default()
        }
    }

    fn row(cells: &[(&str, Option<&str>)]) -> Row {
        cells
            .iter()
            .map(|(c, v)| ((*c).to_owned(), v.map(str::to_owned)))
            .collect()
    }

    fn edits() -> Edits {
        Edits {
            schema: "shop".to_owned(),
            table: "orders".to_owned(),
            ..Edits::default()
        }
    }

    #[test]
    fn finds_documents_by_id_as_the_grid_shows_it() {
        let mut e = edits();
        e.updates.push(Update {
            key: row(&[("_id", Some(&ID.to_ascii_uppercase()))]),
            set: row(&[("qty", Some("3")), ("code", None)]),
        });
        e.deletes
            .push(row(&[("_id", Some("65a1b2c3d4e5f60718293a4c"))]));
        e.inserts
            .push(row(&[("code", Some("x")), ("qty", Some("1"))]));
        let (viewer, shown) = mongo_viewer(&e, &orders("objectId")).unwrap();
        assert_eq!(viewer.table, r#"db.getSiblingDB("shop").orders"#);
        assert_eq!(
            viewer.sql,
            format!(
                r#"db.getSiblingDB("shop").orders.find({{ _id: {{ $in: [ObjectId("{ID}"), ObjectId("65a1b2c3d4e5f60718293a4c")] }} }})"#
            )
        );
        assert_eq!(shown.updates[0].key, row(&[("_id", Some(ID))]));
        assert_eq!(shown.updates[0].set, e.updates[0].set);
        assert_eq!(shown.deletes, e.deletes);
        assert_eq!(shown.inserts, e.inserts);
    }

    #[test]
    fn only_inserts_open_the_whole_collection() {
        let mut e = edits();
        e.inserts
            .push(row(&[("_id", Some("7")), ("qty", Some("1"))]));
        let (viewer, _) = mongo_viewer(&e, &orders("int")).unwrap();
        assert_eq!(viewer.sql, r#"db.getSiblingDB("shop").orders.find({})"#);
    }

    #[test]
    fn refuses_what_the_grid_could_not_save() {
        let details = orders("objectId");
        let refused = |e: &Edits| mongo_viewer(e, &details).unwrap_err();
        let update = |key: Row, set: Row| {
            let mut e = edits();
            e.updates.push(Update { key, set });
            e
        };
        let by_id = row(&[("_id", Some(ID))]);
        // A unique index is not a key here: only `_id` is.
        let by_code = refused(&update(
            row(&[("code", Some("x"))]),
            row(&[("qty", Some("1"))]),
        ));
        assert!(by_code.contains("(_id)"), "{by_code}");
        let not_hex = refused(&update(
            row(&[("_id", Some("7"))]),
            row(&[("qty", Some("1"))]),
        ));
        assert!(not_hex.contains("ObjectId"), "{not_hex}");
        let new_id = refused(&update(by_id.clone(), row(&[("_id", Some(ID))])));
        assert!(new_id.contains("never changes"), "{new_id}");
        let word = refused(&update(by_id.clone(), row(&[("qty", Some("many"))])));
        assert!(
            word.contains("updates[0].set") && word.contains("qty"),
            "{word}"
        );
        let binary = refused(&update(by_id.clone(), row(&[("scan", Some("x"))])));
        assert!(binary.contains("scan"), "{binary}");
        let mut twice = update(by_id.clone(), row(&[("qty", Some("1"))]));
        twice
            .deletes
            .push(row(&[("_id", Some(&ID.to_ascii_uppercase()))]));
        assert!(refused(&twice).contains("named twice"));
        let mixed = mongo_viewer(
            &update(row(&[("_id", Some("1"))]), row(&[("qty", Some("1"))])),
            &orders("mixed"),
        )
        .unwrap_err();
        assert!(mixed.contains("several types"), "{mixed}");
        let mut insert = edits();
        insert.inserts.push(row(&[("qty", Some("1.5x"))]));
        assert!(refused(&insert).contains("inserts[0]"));
    }
}
