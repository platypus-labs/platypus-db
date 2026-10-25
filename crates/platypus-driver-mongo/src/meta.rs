//! Navigator metadata: databases as schemas, collections and views as objects, fields
//! sampled from the documents as columns.

use std::fmt::Write as _;

use futures::TryStreamExt;
use mongodb::bson::{Bson, Document, doc};
use platypus_core::{
    CatalogColumn, ColumnMeta, Error, IndexMeta, ObjectKind, ObjectMeta, ObjectRef, Property,
    Result, SchemaMeta, TableDetails,
};

use crate::MongoConnection;
use crate::literal::{integer, to_shell};

/// Documents read to guess the fields of a collection.
const SAMPLE: i32 = 100;
/// Documents per collection, and collections at most, read for autocompletion.
const CATALOG_SAMPLE: i32 = 20;
const CATALOG_COLLECTIONS: usize = 500;

const SYSTEM_DATABASES: &[&str] = &["admin", "config", "local"];

pub fn schemas(mut names: Vec<String>, current: &str) -> Vec<SchemaMeta> {
    // A database exists once it has data: `use new_db` should still show it.
    if !names.iter().any(|n| n == current) {
        names.push(current.to_owned());
    }
    names.sort();
    names
        .into_iter()
        .map(|name| SchemaMeta {
            system: SYSTEM_DATABASES.contains(&name.as_str()),
            current: name == current,
            name,
        })
        .collect()
}

/// The `listCollections` entries of a database, sorted by name, without `system.*`.
async fn collections(conn: &MongoConnection, db: &str) -> Result<Vec<Document>> {
    let cursor = conn.track(
        conn.client
            .database(db)
            .run_cursor_command(doc! { "listCollections": 1, "authorizedCollections": true })
            .await,
    )?;
    let mut docs: Vec<Document> = conn.track(cursor.try_collect().await)?;
    docs.retain(|d| !d.get_str("name").unwrap_or_default().starts_with("system."));
    docs.sort_by(|a, b| a.get_str("name").ok().cmp(&b.get_str("name").ok()));
    Ok(docs)
}

fn kind_of(entry: &Document) -> ObjectKind {
    if entry.get_str("type") == Ok("view") {
        ObjectKind::View
    } else {
        ObjectKind::Table
    }
}

pub async fn objects(conn: &MongoConnection, schema: &str) -> Result<Vec<ObjectMeta>> {
    Ok(collections(conn, schema)
        .await?
        .iter()
        .map(|entry| {
            let comment = entry
                .get_document("options")
                .ok()
                .and_then(|o| o.get_str("viewOn").ok())
                .map(|source| format!("view on {source}"));
            ObjectMeta::new(
                entry.get_str("name").unwrap_or_default(),
                kind_of(entry),
                comment,
            )
        })
        .collect())
}

/// Up to `size` random documents of a collection or view.
async fn sample(conn: &MongoConnection, db: &str, coll: &str, size: i32) -> Result<Vec<Document>> {
    let cursor = conn.track(
        conn.client
            .database(db)
            .collection::<Document>(coll)
            .aggregate(vec![doc! { "$sample": { "size": size } }])
            .await,
    )?;
    conn.track(cursor.try_collect().await)
}

async fn indexes(conn: &MongoConnection, db: &str, coll: &str) -> Result<Vec<Document>> {
    let cursor = conn.track(
        conn.client
            .database(db)
            .run_cursor_command(doc! { "listIndexes": coll })
            .await,
    )?;
    conn.track(cursor.try_collect().await)
}

pub async fn table_details(
    conn: &MongoConnection,
    schema: &str,
    table: &str,
) -> Result<TableDetails> {
    let docs = sample(conn, schema, table, SAMPLE).await?;
    let fields = crate::docs::infer_schema(&docs);
    let columns = fields
        .fields()
        .iter()
        .map(|f| ColumnMeta {
            name: f.name().clone(),
            data_type: f
                .metadata()
                .get(platypus_core::META_DB_TYPE)
                .cloned()
                .unwrap_or_default(),
            nullable: f.name() != "_id",
            default: None,
            primary_key: f.name() == "_id",
            comment: None,
            enum_values: Vec::new(),
            fields: Vec::new(),
        })
        .collect();
    let is_view = collections(conn, schema)
        .await?
        .iter()
        .any(|c| c.get_str("name") == Ok(table) && kind_of(c) == ObjectKind::View);
    let indexes = if is_view {
        Vec::new()
    } else {
        indexes(conn, schema, table)
            .await?
            .iter()
            .map(|ix| {
                let name = ix.get_str("name").unwrap_or_default().to_owned();
                IndexMeta {
                    columns: ix
                        .get_document("key")
                        .map(|k| k.keys().cloned().collect())
                        .unwrap_or_default(),
                    unique: ix.get_bool("unique").unwrap_or(false) || name == "_id_",
                    primary: name == "_id_",
                    size: None,
                    name,
                }
            })
            .collect()
    };
    Ok(TableDetails {
        columns,
        indexes,
        ..TableDetails::default()
    })
}

pub async fn catalog_columns(conn: &MongoConnection) -> Result<Vec<CatalogColumn>> {
    let db = conn.current_database();
    let mut out = Vec::new();
    for entry in collections(conn, &db)
        .await?
        .iter()
        .take(CATALOG_COLLECTIONS)
    {
        let name = entry.get_str("name").unwrap_or_default();
        let docs = sample(conn, &db, name, CATALOG_SAMPLE).await?;
        let schema = crate::docs::infer_schema(&docs);
        if schema.fields().is_empty() {
            // Still a completion for the collection itself.
            out.push(CatalogColumn {
                schema: db.clone(),
                table: name.to_owned(),
                column: "_id".to_owned(),
            });
        }
        out.extend(schema.fields().iter().map(|f| CatalogColumn {
            schema: db.clone(),
            table: name.to_owned(),
            column: f.name().clone(),
        }));
    }
    Ok(out)
}

/// The default name of an index on `keys`, as the shell makes it: `a_1_b_-1`.
pub fn index_name(keys: &Document) -> String {
    keys.iter()
        .map(|(k, v)| {
            let v = match v {
                Bson::String(s) => s.clone(),
                Bson::Double(d) if d.fract() == 0.0 => format!("{d:.0}"),
                other => other.to_string(),
            };
            format!("{k}_{v}")
        })
        .collect::<Vec<_>>()
        .join("_")
}

/// A collection in mongosh: `db.name`, or `db.getCollection("name")` for other names.
pub fn collection_ref(name: &str) -> String {
    let plain = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    if plain {
        format!("db.{name}")
    } else {
        format!(
            "db.getCollection({})",
            to_shell(&Bson::String(name.to_owned()))
        )
    }
}

fn entry_of<'a>(entries: &'a [Document], name: &str) -> Result<&'a Document> {
    entries
        .iter()
        .find(|c| c.get_str("name") == Ok(name))
        .ok_or_else(|| Error::Other(format!("collection `{name}` not found")))
}

pub async fn object_ddl(conn: &MongoConnection, object: &ObjectRef<'_>) -> Result<String> {
    let entries = collections(conn, object.schema).await?;
    let entry = entry_of(&entries, object.name)?;
    let name = to_shell(&Bson::String(object.name.to_owned()));
    let mut options = entry.get_document("options").cloned().unwrap_or_default();
    let mut out = String::new();
    if kind_of(entry) == ObjectKind::View {
        let source = options.get_str("viewOn").unwrap_or_default().to_owned();
        let pipeline = options
            .get("pipeline")
            .cloned()
            .unwrap_or(Bson::Array(Vec::new()));
        let _ = writeln!(
            out,
            "db.createView({name}, {}, {});",
            to_shell(&Bson::String(source)),
            to_shell(&pipeline)
        );
        return Ok(out);
    }
    if options.is_empty() {
        let _ = writeln!(out, "db.createCollection({name});");
    } else {
        options.remove("uuid");
        let _ = writeln!(
            out,
            "db.createCollection({name}, {});",
            to_shell(&Bson::Document(options))
        );
    }
    let target = collection_ref(object.name);
    for index in indexes(conn, object.schema, object.name).await? {
        let name = index.get_str("name").unwrap_or_default().to_owned();
        if name == "_id_" {
            continue;
        }
        let keys = index.get_document("key").cloned().unwrap_or_default();
        let mut options = Document::new();
        if name != index_name(&keys) {
            options.insert("name", name);
        }
        for (k, v) in &index {
            if !matches!(k.as_str(), "v" | "key" | "name" | "ns") {
                options.insert(k.clone(), v.clone());
            }
        }
        let keys = to_shell(&Bson::Document(keys));
        if options.is_empty() {
            let _ = writeln!(out, "{target}.createIndex({keys});");
        } else {
            let _ = writeln!(
                out,
                "{target}.createIndex({keys}, {});",
                to_shell(&Bson::Document(options))
            );
        }
    }
    Ok(out)
}

pub async fn object_properties(
    conn: &MongoConnection,
    object: &ObjectRef<'_>,
) -> Result<Vec<Property>> {
    let entries = collections(conn, object.schema).await?;
    let entry = entry_of(&entries, object.name)?;
    let options = entry.get_document("options").cloned().unwrap_or_default();
    let mut out = vec![Property::new(
        "Type",
        entry.get_str("type").unwrap_or("collection"),
    )];
    if let Ok(source) = options.get_str("viewOn") {
        out.push(Property::new("View on", source));
        return Ok(out);
    }
    let stats = conn.track(
        conn.client
            .database(object.schema)
            .collection::<Document>(object.name)
            .aggregate(vec![doc! { "$collStats": { "storageStats": {} } }])
            .await,
    )?;
    let stats: Vec<Document> = conn.track(stats.try_collect().await)?;
    if let Some(storage) = stats
        .first()
        .and_then(|s| s.get_document("storageStats").ok())
    {
        for (label, key) in [
            ("Documents", "count"),
            ("Data size", "size"),
            ("Storage size", "storageSize"),
            ("Indexes", "nindexes"),
            ("Index size", "totalIndexSize"),
        ] {
            if let Some(n) = storage.get(key).and_then(integer) {
                out.push(Property::new(label, n.to_string()));
            }
        }
        if storage.get_bool("capped").unwrap_or(false) {
            out.push(Property::new("Capped", "yes"));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_indexes_and_collections() {
        assert_eq!(index_name(&doc! { "a": 1, "b": -1 }), "a_1_b_-1");
        assert_eq!(index_name(&doc! { "t": "text" }), "t_text");
        assert_eq!(collection_ref("users"), "db.users");
        assert_eq!(collection_ref("my-coll"), "db.getCollection(\"my-coll\")");
        assert_eq!(collection_ref("1x"), "db.getCollection(\"1x\")");
    }

    #[test]
    fn lists_databases_as_schemas() {
        let s = schemas(vec!["shop".into(), "admin".into()], "new");
        let names: Vec<_> = s
            .iter()
            .map(|s| (s.name.as_str(), s.system, s.current))
            .collect();
        assert_eq!(
            names,
            [
                ("admin", true, false),
                ("new", false, true),
                ("shop", false, false)
            ]
        );
    }
}
