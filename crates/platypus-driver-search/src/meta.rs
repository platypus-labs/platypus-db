//! The catalog. A cluster is one schema (named after it); its tables are the indices, its
//! views the aliases and data streams. The columns of a table are the fields of its mapping
//! (`a.b.c` for the fields of objects, `title.raw` for multi-fields), after `_id`, the
//! primary key.

use platypus_core::{
    CATALOG_LIMIT, CatalogColumn, ColumnMeta, Error, IndexMeta, ObjectKind, ObjectMeta, ObjectRef,
    Property, Result, SchemaMeta, TableDetails,
};
use serde_json::{Map, Value};

use crate::http::Payload;
use crate::session::Shared;

/// Columns an index shows in the catalog, as `(name, type, comment)`.
pub type Field = (String, String, Option<String>);

/// A path segment: the characters that would end it, encoded.
#[must_use]
pub fn segment(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '%' => out.push_str("%25"),
            '/' => out.push_str("%2F"),
            '?' => out.push_str("%3F"),
            '#' => out.push_str("%23"),
            ' ' => out.push_str("%20"),
            c => out.push(c),
        }
    }
    out
}

#[must_use]
pub fn schemas(cluster: &str) -> Vec<SchemaMeta> {
    vec![SchemaMeta {
        name: cluster.to_owned(),
        system: false,
        current: true,
    }]
}

pub async fn get(shared: &Shared, target: &str) -> Result<Value> {
    let reply = shared.call("GET", target, Payload::None, None).await?;
    serde_json::from_str(&reply.body).map_err(|e| Error::Other(format!("unexpected reply: {e}")))
}

fn text<'a>(row: &'a Value, key: &str) -> Option<&'a str> {
    row.get(key).and_then(Value::as_str)
}

fn count(row: &Value, key: &str) -> Option<u64> {
    text(row, key)?.parse().ok()
}

/// `1 shard`, `2 replicas`.
fn plural(n: &str, noun: &str) -> String {
    if n == "1" {
        format!("{n} {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// The indices matching `pattern` (all when empty), the aliases and the data streams of the
/// cluster. System indices (`.kibana`…) say so in their comment.
pub async fn objects(shared: &Shared, pattern: &str) -> Result<Vec<ObjectMeta>> {
    let which = if pattern.is_empty() {
        String::new()
    } else {
        format!("/{}", segment(pattern))
    };
    let indices = get(
        shared,
        &format!(
            "/_cat/indices{which}?format=json&bytes=b&expand_wildcards=open,closed\
             &h=index,health,status,docs.count,store.size,pri,rep"
        ),
    )
    .await?;
    let mut objects: Vec<ObjectMeta> = indices
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let name = text(row, "index")?;
            let mut notes = Vec::new();
            if name.starts_with('.') {
                notes.push("system".to_owned());
            }
            match text(row, "status") {
                Some("close") => notes.push("closed".to_owned()),
                _ => notes.extend(text(row, "health").map(str::to_owned)),
            }
            if let (Some(pri), Some(rep)) = (text(row, "pri"), text(row, "rep")) {
                notes.push(plural(pri, "shard"));
                notes.push(plural(rep, "replica"));
            }
            let mut object = ObjectMeta::new(name, ObjectKind::Table, Some(notes.join(", ")));
            object.rows = count(row, "docs.count");
            object.size = count(row, "store.size");
            Some(object)
        })
        .collect();
    // Aliases and data streams are best effort: a user may see indices and not them.
    if let Ok(aliases) = get(shared, "/_cat/aliases?format=json&h=alias,index").await {
        let mut targets: Vec<(String, Vec<String>)> = Vec::new();
        for row in aliases.as_array().into_iter().flatten() {
            let (Some(alias), Some(index)) = (text(row, "alias"), text(row, "index")) else {
                continue;
            };
            if !pattern.is_empty() && !objects.iter().any(|o| o.name == index) {
                continue;
            }
            match targets.iter_mut().find(|(a, _)| a == alias) {
                Some((_, indices)) => indices.push(index.to_owned()),
                None => targets.push((alias.to_owned(), vec![index.to_owned()])),
            }
        }
        objects.extend(targets.into_iter().map(|(alias, indices)| {
            ObjectMeta::new(
                alias,
                ObjectKind::View,
                Some(format!("alias of {}", indices.join(", "))),
            )
        }));
    }
    if let Ok(streams) = get(shared, "/_data_stream").await {
        for stream in streams
            .get("data_streams")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(name) = text(stream, "name")
                && (pattern.is_empty() || !objects.iter().any(|o| o.name == name))
            {
                objects.push(ObjectMeta::new(
                    name,
                    ObjectKind::View,
                    Some("data stream".to_owned()),
                ));
            }
        }
    }
    objects.sort_by(|a, b| a.name.cmp(&b.name));
    objects.dedup_by(|a, b| a.name == b.name);
    Ok(objects)
}

/// The fields of the `properties` of a mapping, the leaves of objects, in `out`.
pub fn flatten(properties: &Map<String, Value>, prefix: &str, out: &mut Vec<Field>) {
    for (name, def) in properties {
        let full = format!("{prefix}{name}");
        if let Some(Value::Object(inner)) = def.get("properties") {
            if inner.is_empty() {
                out.push((full, "object".to_owned(), None));
            } else {
                flatten(inner, &format!("{full}."), out);
            }
            continue;
        }
        let kind = def.get("type").and_then(Value::as_str).unwrap_or("object");
        let ty = match (kind, def.get("dims").and_then(Value::as_u64)) {
            ("dense_vector", Some(dims)) => format!("dense_vector({dims})"),
            _ => kind.to_owned(),
        };
        let comment = (kind == "alias")
            .then(|| def.get("path").and_then(Value::as_str))
            .flatten()
            .map(|path| format!("alias of {path}"));
        out.push((full.clone(), ty, comment));
        if let Some(Value::Object(sub)) = def.get("fields") {
            for (sub_name, sub_def) in sub {
                let ty = sub_def
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("object");
                out.push((
                    format!("{full}.{sub_name}"),
                    ty.to_owned(),
                    Some(format!("sub-field of {full}")),
                ));
            }
        }
    }
}

/// The fields of every index in a `_mapping` reply, the first of a name winning.
#[must_use]
pub fn mapped_fields(reply: &Value) -> Vec<Field> {
    let mut fields: Vec<Field> = Vec::new();
    // Indices in name order and the fields of each sorted by name, whichever way
    // `serde_json` is built.
    let mut indices: Vec<(&String, &Value)> = reply.as_object().into_iter().flatten().collect();
    indices.sort_by(|a, b| a.0.cmp(b.0));
    for (_, mapping) in indices {
        let Some(Value::Object(properties)) = mapping.pointer("/mappings/properties") else {
            continue;
        };
        let mut found = Vec::new();
        flatten(properties, "", &mut found);
        found.sort_by(|a, b| a.0.cmp(&b.0));
        for field in found {
            if !fields.iter().any(|(n, _, _)| *n == field.0) {
                fields.push(field);
            }
        }
    }
    fields
}

/// The top-level fields of every index in a `_mapping` reply as `(name, type)`, the first
/// of a name winning. Objects are `document`, nested fields `array`, as the grid shows
/// them; `_id` and `_index` are not among them.
#[must_use]
pub fn top_level_fields(reply: &Value) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = Vec::new();
    for mapping in reply.as_object().into_iter().flat_map(Map::values) {
        let Some(Value::Object(properties)) = mapping.pointer("/mappings/properties") else {
            continue;
        };
        for (name, def) in properties {
            if fields.iter().any(|(n, _)| n == name) {
                continue;
            }
            let ty = match def.get("type").and_then(Value::as_str) {
                None | Some("object" | "flattened") => "document",
                Some("nested") => "array",
                Some(other) => other,
            };
            fields.push((name.clone(), ty.to_owned()));
        }
    }
    fields
}

pub async fn table_details(shared: &Shared, table: &str) -> Result<TableDetails> {
    let reply = get(shared, &format!("/{}/_mapping", segment(table))).await?;
    let mut columns = vec![ColumnMeta {
        name: "_id".to_owned(),
        data_type: "keyword".to_owned(),
        nullable: false,
        default: None,
        primary_key: true,
        comment: None,
        enum_values: Vec::new(),
        fields: Vec::new(),
    }];
    columns.extend(
        mapped_fields(&reply)
            .into_iter()
            .map(|(name, ty, comment)| ColumnMeta {
                name,
                data_type: ty,
                nullable: true,
                default: None,
                primary_key: false,
                comment,
                enum_values: Vec::new(),
                fields: Vec::new(),
            }),
    );
    Ok(TableDetails {
        columns,
        indexes: vec![IndexMeta {
            name: "_id".to_owned(),
            columns: vec!["_id".to_owned()],
            unique: true,
            primary: true,
            partial: false,
            size: None,
        }],
        foreign_keys: Vec::new(),
        constraints: Vec::new(),
        referenced_by: Vec::new(),
    })
}

/// Every field of every index that is not a system one, for autocompletion.
pub async fn catalog_columns(
    shared: &Shared,
    cluster: &str,
    pattern: &str,
) -> Result<Vec<CatalogColumn>> {
    let which = if pattern.is_empty() {
        String::new()
    } else {
        format!("/{}", segment(pattern))
    };
    let reply = get(shared, &format!("{which}/_mapping?expand_wildcards=open")).await?;
    let mut out = Vec::new();
    'indices: for (index, mapping) in reply.as_object().into_iter().flatten() {
        if index.starts_with('.') {
            continue;
        }
        let one = Value::Object(Map::from_iter([(index.clone(), mapping.clone())]));
        let fields = mapped_fields(&one);
        for column in std::iter::once("_id".to_owned()).chain(fields.into_iter().map(|f| f.0)) {
            if out.len() >= CATALOG_LIMIT {
                break 'indices;
            }
            out.push(CatalogColumn {
                schema: cluster.to_owned(),
                table: index.clone(),
                column,
            });
        }
    }
    Ok(out)
}

/// The request that creates an index again, or that adds an alias; pretty printed.
pub async fn object_ddl(shared: &Shared, object: &ObjectRef<'_>) -> Result<String> {
    let name = object.name;
    if object.kind == ObjectKind::View {
        let reply = get(shared, &format!("/_alias/{}", segment(name))).await;
        let Ok(Value::Object(indices)) = reply else {
            return Ok(format!(
                "# `{name}` is a data stream: PUT /_data_stream/{name}"
            ));
        };
        let actions: Vec<Value> = indices
            .keys()
            .map(|index| serde_json::json!({ "add": { "index": index, "alias": name } }))
            .collect();
        return pretty("POST /_aliases", &serde_json::json!({ "actions": actions }));
    }
    let reply = get(shared, &format!("/{}", segment(name))).await?;
    let Some(Value::Object(index)) = reply.get(name).cloned() else {
        return Err(Error::Other(format!("no such index `{name}`")));
    };
    let mut body = Map::new();
    if let Some(Value::Object(mut settings)) = index.get("settings").cloned() {
        if let Some(Value::Object(inner)) = settings.get_mut("index") {
            for generated in ["uuid", "creation_date", "version", "provided_name"] {
                inner.remove(generated);
            }
        }
        body.insert("settings".to_owned(), Value::Object(settings));
    }
    for key in ["mappings", "aliases"] {
        if let Some(value) = index.get(key)
            && !value.as_object().is_some_and(Map::is_empty)
        {
            body.insert(key.to_owned(), value.clone());
        }
    }
    pretty(&format!("PUT /{name}"), &Value::Object(body))
}

fn pretty(head: &str, body: &Value) -> Result<String> {
    let body = serde_json::to_string_pretty(body).map_err(|e| Error::Other(e.to_string()))?;
    Ok(format!("{head}\n{body}"))
}

/// Health, shards, documents and size of an index.
pub async fn object_properties(shared: &Shared, object: &ObjectRef<'_>) -> Result<Vec<Property>> {
    if object.kind != ObjectKind::Table {
        return Ok(Vec::new());
    }
    let rows = get(
        shared,
        &format!(
            "/_cat/indices/{}?format=json&bytes=b&expand_wildcards=all\
             &h=health,status,uuid,pri,rep,docs.count,docs.deleted,store.size,creation.date.string",
            segment(object.name)
        ),
    )
    .await?;
    let Some(row) = rows.get(0) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (label, key) in [
        ("Health", "health"),
        ("Status", "status"),
        ("Primary shards", "pri"),
        ("Replicas", "rep"),
        ("Documents", "docs.count"),
        ("Deleted documents", "docs.deleted"),
        ("Size (bytes)", "store.size"),
        ("Created", "creation.date.string"),
        ("UUID", "uuid"),
    ] {
        if let Some(value) = text(row, key) {
            out.push(Property::new(label, value));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattens_mappings() {
        let reply: Value = serde_json::from_str(
            r#"{"orders":{"mappings":{"properties":{
                "title":{"type":"text","fields":{"raw":{"type":"keyword"}}},
                "customer":{"properties":{"name":{"type":"keyword"},"address":{"properties":{"zip":{"type":"keyword"}}}}},
                "empty":{"properties":{}},
                "vec":{"type":"dense_vector","dims":3},
                "short":{"type":"alias","path":"title"},
                "n":{"type":"long"}}}},
              "other":{"mappings":{"properties":{"n":{"type":"keyword"},"extra":{"type":"date"}}}}}"#,
        )
        .unwrap();
        let fields: Vec<(String, String)> = mapped_fields(&reply)
            .into_iter()
            .map(|(n, t, _)| (n, t))
            .collect();
        let want = [
            ("customer.address.zip", "keyword"),
            ("customer.name", "keyword"),
            ("empty", "object"),
            ("n", "long"),
            ("short", "alias"),
            ("title", "text"),
            ("title.raw", "keyword"),
            ("vec", "dense_vector(3)"),
            ("extra", "date"),
        ];
        assert_eq!(fields, want.map(|(n, t)| (n.to_owned(), t.to_owned())));
    }

    #[test]
    fn lists_the_top_level_fields() {
        let reply: Value = serde_json::from_str(
            r#"{"a":{"mappings":{"properties":{"id":{"type":"keyword"},"owner":{"properties":{"x":{"type":"long"}}},"items":{"type":"nested"}}}},
                "b":{"mappings":{"properties":{"id":{"type":"long"},"late":{"type":"date"}}}},
                "c":{"mappings":{}}}"#,
        )
        .unwrap();
        let want = [
            ("id", "keyword"),
            ("items", "array"),
            ("owner", "document"),
            ("late", "date"),
        ];
        // The order of a mapping's fields depends on how `serde_json` is built.
        let mut found = top_level_fields(&reply);
        found.sort();
        let mut want = want.map(|(n, t)| (n.to_owned(), t.to_owned()));
        want.sort();
        assert_eq!(found, want);
    }

    #[test]
    fn encodes_path_segments() {
        assert_eq!(segment("logs-2024.01"), "logs-2024.01");
        assert_eq!(segment("a b/c?d"), "a%20b%2Fc%3Fd");
    }
}
