//! The requests that save the grid's changes to the documents of an index, and those that
//! undo them. Each is a statement of the editor ([`crate::command`]), run one by one by
//! `execute_all`, which stops at the first the server refuses:
//!
//! - an edited document is a `POST /<index>/_update/<_id>` with the changed fields (a
//!   script when a field holds an object or an array, which `doc` would merge with the old
//!   one instead of replacing). A NULL sets the field to `null`;
//! - a new document with an `_id` is a `PUT /<index>/_create/<_id>`, which refuses an id that
//!   exists; without one a `POST /<index>/_doc`. Fields left NULL are not sent;
//! - a deleted document is a `DELETE /<index>/_doc/<_id>`.
//!
//! All wait for the change to be visible (`refresh=wait_for`). The `_id` of a document does
//! not change. With an `_index` column (a result from several indices) each document goes to
//! its own index. Values are sent as JSON by the type of their column ([`META_DB_TYPE`]):
//! numbers, booleans, objects and arrays as such, anything else as text.
//!
//! [`META_DB_TYPE`]: platypus_core::META_DB_TYPE

use std::fmt::Write as _;

use serde_json::{Map, Number, Value, json};

use crate::meta::segment;

/// A loaded row changed: its values as loaded, and the new values of some columns (`None`
/// for NULL).
#[derive(Debug, Clone)]
pub struct Update {
    pub loaded: Vec<Option<String>>,
    pub set: Vec<(usize, Option<String>)>,
}

/// The changes to save to the documents of one index.
#[derive(Debug, Clone)]
pub struct Edits<'a> {
    /// The index the result was read from.
    pub index: &'a str,
    /// The result's column names, by index.
    pub columns: &'a [String],
    /// Their types (`long`, `string`, `document`…), by index.
    pub types: &'a [String],
    pub updates: Vec<Update>,
    /// New rows: the values set, by column.
    pub inserts: Vec<Vec<(usize, Option<String>)>>,
    /// Deleted rows, as loaded.
    pub deletes: Vec<Vec<Option<String>>>,
}

/// The requests that save, and the script that puts things back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    pub commands: Vec<String>,
    /// Requests, and `#` comments for what cannot be undone.
    pub undo: Vec<String>,
}

const REFRESH: &str = "refresh=wait_for";
const REPLACE_FIELDS: &str =
    "for (e in params.set.entrySet()) { ctx._source[e.getKey()] = e.getValue(); }";

/// A document id in a path.
fn encode_id(id: &str) -> String {
    let mut out = String::new();
    for b in id.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// The value of a cell as JSON, by the type of its column.
fn json_value(text: Option<&str>, db_type: &str) -> Result<Value, String> {
    let Some(text) = text else {
        return Ok(Value::Null);
    };
    let t = text.trim();
    match db_type {
        "long" | "integer" | "short" | "byte" | "unsigned_long" => t
            .parse::<i64>()
            .map(Value::from)
            .or_else(|_| t.parse::<u64>().map(Value::from))
            .map_err(|_| format!("invalid integer `{t}`")),
        "double" | "float" | "half_float" | "scaled_float" => t
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| format!("invalid number `{t}`")),
        "boolean" => match t.to_ascii_lowercase().as_str() {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err(format!("invalid boolean `{t}`: true or false")),
        },
        "document" | "array" | "object" | "nested" | "flattened" => serde_json::from_str(t)
            .map_err(|e| format!("invalid JSON for the {db_type} `{t}`: {e}")),
        // Both numbers and text were seen in the column: what parses as JSON is.
        "mixed" => Ok(serde_json::from_str(t).unwrap_or_else(|_| Value::String(text.to_owned()))),
        _ => Ok(Value::String(text.to_owned())),
    }
}

/// A row's values by column name.
struct Row<'a> {
    edits: &'a Edits<'a>,
    values: Vec<(usize, Option<&'a str>)>,
}

impl<'a> Row<'a> {
    fn loaded(edits: &'a Edits<'a>, loaded: &'a [Option<String>]) -> Self {
        Self {
            edits,
            values: loaded
                .iter()
                .enumerate()
                .map(|(i, v)| (i, v.as_deref()))
                .collect(),
        }
    }

    fn set(edits: &'a Edits<'a>, set: &'a [(usize, Option<String>)]) -> Self {
        Self {
            edits,
            values: set.iter().map(|(i, v)| (*i, v.as_deref())).collect(),
        }
    }

    fn text(&self, column: &str) -> Option<&'a str> {
        let index = self.edits.columns.iter().position(|c| c == column)?;
        self.values
            .iter()
            .find(|(i, _)| *i == index)
            .and_then(|(_, v)| *v)
    }

    /// The `_id`, which a document edit cannot do without.
    fn id(&self) -> Result<&'a str, String> {
        if !self.edits.columns.iter().any(|c| c == "_id") {
            return Err("the result has no _id column: edit the documents of an index".to_owned());
        }
        self.text("_id")
            .filter(|id| !id.is_empty())
            .ok_or_else(|| "the document has no _id".to_owned())
    }

    /// The index the row's document is in.
    fn index(&self) -> &'a str {
        self.text("_index").unwrap_or(self.edits.index)
    }

    /// The fields of the row but `_id` and `_index`, NULL ones included when `nulls`.
    fn fields(&self, nulls: bool) -> Result<Map<String, Value>, String> {
        let mut out = Map::new();
        for &(i, value) in &self.values {
            let name = &self.edits.columns[i];
            if name == "_id" || name == "_index" || (value.is_none() && !nulls) {
                continue;
            }
            out.insert(name.clone(), json_value(value, &self.edits.types[i])?);
        }
        Ok(out)
    }
}

fn target(index: &str, api: &str, id: Option<&str>) -> String {
    let id = id.map_or(String::new(), |id| format!("/{}", encode_id(id)));
    format!("/{}/{api}{id}?{REFRESH}", segment(index))
}

fn request(method: &str, target: &str, body: Option<Value>) -> String {
    match body {
        Some(body) => format!("{method} {target}\n{body}"),
        None => format!("{method} {target}"),
    }
}

/// The update of a document to `fields`.
fn update_request(target: &str, fields: &Map<String, Value>) -> String {
    let structured = fields.values().any(|v| v.is_object() || v.is_array());
    let body = if structured {
        json!({ "script": { "lang": "painless", "source": REPLACE_FIELDS, "params": { "set": fields } } })
    } else {
        json!({ "doc": fields })
    };
    request("POST", target, Some(body))
}

fn check_lengths(edits: &Edits<'_>) -> Result<(), String> {
    if edits.types.len() == edits.columns.len() {
        Ok(())
    } else {
        Err("every column needs its type".to_owned())
    }
}

/// The requests that save `edits`, and the ones that undo them.
pub fn script(edits: &Edits<'_>) -> Result<Script, String> {
    check_lengths(edits)?;
    let mut commands = Vec::new();
    let mut undo = Vec::new();
    for loaded in &edits.deletes {
        let row = Row::loaded(edits, loaded);
        let (index, id) = (row.index(), row.id()?);
        commands.push(request("DELETE", &target(index, "_doc", Some(id)), None));
        undo.push(request(
            "PUT",
            &target(index, "_create", Some(id)),
            Some(Value::Object(row.fields(false)?)),
        ));
    }
    for update in &edits.updates {
        let row = Row::loaded(edits, &update.loaded);
        let (index, id) = (row.index(), row.id()?);
        let mut fields = Map::new();
        let mut old = Map::new();
        for (i, value) in &update.set {
            let name = &edits.columns[*i];
            if name == "_id" || name == "_index" {
                if value.as_deref() != row.text(name) {
                    return Err(format!("the {name} of a document can not be changed"));
                }
                continue;
            }
            fields.insert(
                name.clone(),
                json_value(value.as_deref(), &edits.types[*i])?,
            );
            old.insert(name.clone(), json_value(row.text(name), &edits.types[*i])?);
        }
        if fields.is_empty() {
            continue;
        }
        let target = target(index, "_update", Some(id));
        commands.push(update_request(&target, &fields));
        undo.push(update_request(&target, &old));
    }
    for set in &edits.inserts {
        let row = Row::set(edits, set);
        let index = row.index();
        let fields = Value::Object(row.fields(false)?);
        if let Some(id) = row.text("_id").filter(|id| !id.is_empty()) {
            commands.push(request(
                "PUT",
                &target(index, "_create", Some(id)),
                Some(fields),
            ));
            undo.push(request("DELETE", &target(index, "_doc", Some(id)), None));
        } else {
            commands.push(request("POST", &target(index, "_doc", None), Some(fields)));
            undo.push(format!(
                "# the new document of {index} has a generated _id: delete it by hand"
            ));
        }
    }
    Ok(Script { commands, undo })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Body, Command, Kind, parse};

    fn names(columns: &[&str]) -> Vec<String> {
        columns.iter().map(|c| (*c).to_owned()).collect()
    }

    #[allow(clippy::unnecessary_wraps)]
    fn some(text: &str) -> Option<String> {
        Some(text.to_owned())
    }

    fn edits<'a>(columns: &'a [String], types: &'a [String]) -> Edits<'a> {
        Edits {
            index: "orders",
            columns,
            types,
            updates: Vec::new(),
            inserts: Vec::new(),
            deletes: Vec::new(),
        }
    }

    #[test]
    fn updates_by_id_and_undoes() {
        let columns = names(&["_id", "total", "note", "paid", "tags"]);
        let types = names(&["string", "long", "string", "boolean", "array"]);
        let mut e = edits(&columns, &types);
        e.updates.push(Update {
            loaded: vec![
                some("a/1"),
                some("5"),
                some("hi"),
                some("false"),
                some("[1]"),
            ],
            set: vec![(1, some("7")), (2, None), (3, some("TRUE"))],
        });
        let s = script(&e).unwrap();
        assert_eq!(
            s.commands,
            [r#"POST /orders/_update/a%2F1?refresh=wait_for
{"doc":{"note":null,"paid":true,"total":7}}"#]
        );
        assert_eq!(
            s.undo,
            [r#"POST /orders/_update/a%2F1?refresh=wait_for
{"doc":{"note":"hi","paid":false,"total":5}}"#]
        );
        // The statements parse as the editor reads them.
        for text in s.commands.iter().chain(&s.undo) {
            let Command::Request(r) = parse(text).unwrap() else {
                panic!()
            };
            assert_eq!(r.kind(), Kind::Dml);
            assert!(matches!(r.body, Body::Json(_)));
        }
    }

    #[test]
    fn replaces_objects_with_a_script() {
        let columns = names(&["_id", "owner"]);
        let types = names(&["string", "document"]);
        let mut e = edits(&columns, &types);
        e.updates.push(Update {
            loaded: vec![some("1"), some(r#"{"a":1,"b":2}"#)],
            set: vec![(1, some(r#"{"a":2}"#))],
        });
        let s = script(&e).unwrap();
        assert!(s.commands[0].contains(r#""script""#), "{}", s.commands[0]);
        assert!(s.commands[0].contains(r#""set":{"owner":{"a":2}}"#));
        assert!(s.undo[0].contains(r#""set":{"owner":{"a":1,"b":2}}"#));
        e.updates[0].set = vec![(1, some("{oops"))];
        assert!(script(&e).unwrap_err().contains("invalid JSON"));
    }

    #[test]
    fn inserts_and_deletes() {
        let columns = names(&["_id", "_index", "total", "note"]);
        let types = names(&["string", "string", "long", "string"]);
        let mut e = edits(&columns, &types);
        e.inserts
            .push(vec![(0, some("n1")), (2, some("3")), (3, None)]);
        e.inserts.push(vec![(1, some("archive")), (3, some("x"))]);
        e.deletes
            .push(vec![some("d1"), some("orders"), some("9"), None]);
        let s = script(&e).unwrap();
        assert_eq!(
            s.commands,
            [
                "DELETE /orders/_doc/d1?refresh=wait_for",
                "PUT /orders/_create/n1?refresh=wait_for\n{\"total\":3}",
                "POST /archive/_doc?refresh=wait_for\n{\"note\":\"x\"}",
            ]
        );
        assert_eq!(
            s.undo[0],
            "PUT /orders/_create/d1?refresh=wait_for\n{\"total\":9}"
        );
        assert_eq!(s.undo[1], "DELETE /orders/_doc/n1?refresh=wait_for");
        assert!(s.undo[2].starts_with("# the new document of archive"));
    }

    #[test]
    fn refuses_what_cannot_be_saved() {
        let columns = names(&["_id", "total"]);
        let types = names(&["string", "long"]);
        let mut e = edits(&columns, &types);
        e.updates.push(Update {
            loaded: vec![some("1"), some("1")],
            set: vec![(1, some("x"))],
        });
        assert!(script(&e).unwrap_err().contains("invalid integer"));
        e.updates[0].set = vec![(0, some("2"))];
        assert!(script(&e).unwrap_err().contains("_id of a document"));
        e.updates.clear();
        e.deletes.push(vec![None, None]);
        assert!(script(&e).unwrap_err().contains("no _id"));
        let columns = names(&["total"]);
        let types = names(&["long"]);
        let mut e = edits(&columns, &types);
        e.deletes.push(vec![some("1")]);
        assert!(script(&e).unwrap_err().contains("no _id column"));
    }
}
