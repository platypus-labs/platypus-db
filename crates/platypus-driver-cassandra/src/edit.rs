//! The statements that save the grid's changes to the rows of a table, and those that undo
//! them. Each is a statement of the editor ([`crate::cql`]), run one by one by `execute_all`:
//!
//! - an edited row is an `UPDATE … SET … WHERE <full primary key>`;
//! - a new row is an `INSERT INTO … VALUES …`, which needs every key column;
//! - a deleted row is a `DELETE FROM … WHERE <full primary key>`.
//!
//! A row is found by its whole primary key, partition and clustering columns, as CQL has no
//! other way to name one. Key columns cannot change (delete the row and insert another), and
//! counters change by increments the grid cannot write, so edits to them are refused. CQL
//! writes are upserts: an update of a row somebody deleted meanwhile writes it again, and an
//! insert over an existing row replaces the columns it names.
//!
//! Values are written by the CQL type of their column ([`META_DB_TYPE`]): text quoted,
//! numbers, booleans, uuids and durations bare, collections, tuples and user-defined types
//! from the JSON the grid shows them as.
//!
//! [`META_DB_TYPE`]: platypus_core::META_DB_TYPE

use std::collections::HashMap;

use serde_json::Value;

use crate::cql::{qualified, quote_ident, quote_string};

/// The fields of user-defined types, by type name: `(field, type)` in declaration order.
pub type Udts = HashMap<String, Vec<(String, String)>>;

/// A loaded row changed: its values as loaded, and the new values of some columns (`None`
/// for NULL).
#[derive(Debug, Clone)]
pub struct Update {
    pub loaded: Vec<Option<String>>,
    pub set: Vec<(usize, Option<String>)>,
}

/// The changes to save to the rows of one table.
#[derive(Debug, Clone)]
pub struct Edits<'a> {
    pub keyspace: &'a str,
    pub table: &'a str,
    /// The result's column names, by index.
    pub columns: &'a [String],
    /// Their CQL types, by index.
    pub types: &'a [String],
    /// The result columns that make the primary key.
    pub key: &'a [usize],
    /// User-defined types the columns use.
    pub udts: &'a Udts,
    pub updates: Vec<Update>,
    /// New rows: the values set, by column.
    pub inserts: Vec<Vec<(usize, Option<String>)>>,
    /// Deleted rows, as loaded.
    pub deletes: Vec<Vec<Option<String>>>,
}

/// The statements that save, and the script that puts things back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Script {
    pub commands: Vec<String>,
    /// Statements, and `--` comments for what cannot be undone.
    pub undo: Vec<String>,
}

/// Whether any of the CQL types `types` is, or holds, a user-defined type, whose fields the
/// edits need to know ([`Udts`]).
#[must_use]
pub fn uses_udts(types: &[String]) -> bool {
    fn holds(ty: &Ty) -> bool {
        match ty {
            Ty::Native(_) => false,
            Ty::Udt(_) => true,
            Ty::List(item) | Ty::Set(item) | Ty::Vector(item) => holds(item),
            Ty::Map(key, value) => holds(key) || holds(value),
            Ty::Tuple(items) => items.iter().any(holds),
        }
    }
    types.iter().any(|t| holds(&parse_type(t)))
}

/// A CQL type.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Ty {
    Native(String),
    List(Box<Ty>),
    Set(Box<Ty>),
    Map(Box<Ty>, Box<Ty>),
    Tuple(Vec<Ty>),
    Vector(Box<Ty>),
    Udt(String),
}

const NATIVE: &[&str] = &[
    "ascii",
    "bigint",
    "blob",
    "boolean",
    "counter",
    "date",
    "decimal",
    "double",
    "duration",
    "float",
    "inet",
    "int",
    "smallint",
    "text",
    "time",
    "timestamp",
    "timeuuid",
    "tinyint",
    "uuid",
    "varchar",
    "varint",
];

/// Splits `a, map<b, c>, d` at the commas outside `<…>`.
pub(crate) fn split_args(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0usize, 0);
    for (i, c) in text.char_indices() {
        match c {
            '<' | '(' => depth += 1,
            '>' | ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = text[start..].trim();
    if !last.is_empty() {
        out.push(last);
    }
    out
}

/// The type a type name writes (`frozen<…>` and the keyspace of a user type are dropped).
fn parse_type(text: &str) -> Ty {
    let text = text.trim();
    let (head, args) = match text.split_once('<') {
        Some((head, rest)) => (head.trim(), rest.strip_suffix('>').unwrap_or(rest)),
        None => (text, ""),
    };
    let args: Vec<Ty> = split_args(args).into_iter().map(parse_type).collect();
    let lower = head.to_ascii_lowercase();
    match (lower.as_str(), args.as_slice()) {
        ("frozen", [inner]) => inner.clone(),
        ("list", [item]) => Ty::List(Box::new(item.clone())),
        ("set", [item]) => Ty::Set(Box::new(item.clone())),
        ("map", [key, value]) => Ty::Map(Box::new(key.clone()), Box::new(value.clone())),
        ("tuple", items) => Ty::Tuple(items.to_vec()),
        // The dimension is not a type.
        ("vector", [item, ..]) => Ty::Vector(Box::new(item.clone())),
        _ if NATIVE.contains(&lower.as_str()) => Ty::Native(lower),
        _ => Ty::Udt(
            head.rsplit('.')
                .next()
                .unwrap_or(head)
                .trim_matches('"')
                .to_owned(),
        ),
    }
}

fn is_uuid(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(&parts)
            .all(|(n, p)| p.len() == *n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn is_integer(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// A number CQL reads as a `decimal`: digits, a fraction and an exponent.
fn is_decimal(text: &str) -> bool {
    let (mantissa, exponent) = text
        .split_once(['e', 'E'])
        .map_or((text, None), |(m, e)| (m, Some(e)));
    let mantissa = mantissa.strip_prefix('-').unwrap_or(mantissa);
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, "0"));
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    digits(whole)
        && digits(fraction)
        && exponent.is_none_or(|e| is_integer(e.strip_prefix('+').unwrap_or(e)))
}

/// A scalar from its text.
fn scalar(text: &str, name: &str) -> Result<String, String> {
    let t = text.trim();
    let bad = |what: &str| Err(format!("invalid {what} `{t}`"));
    match name {
        "ascii" | "text" | "varchar" => Ok(quote_string(text)),
        "tinyint" | "smallint" | "int" | "bigint" | "varint" => {
            if is_integer(t) {
                Ok(t.to_owned())
            } else {
                bad("integer")
            }
        }
        "counter" => Err("a counter changes by increments, which the grid cannot write".to_owned()),
        // CQL spells the special values `NaN`, `Infinity` and `-Infinity`; the grid shows them
        // the way Arrow prints them (`NaN`, `inf`, `-inf`).
        "float" | "double" => match t {
            "NaN" | "Infinity" | "-Infinity" => Ok(t.to_owned()),
            "inf" | "+inf" => Ok("Infinity".to_owned()),
            "-inf" => Ok("-Infinity".to_owned()),
            "nan" => Ok("NaN".to_owned()),
            _ if t.parse::<f64>().is_ok_and(f64::is_finite) => Ok(t.to_owned()),
            _ => bad("number"),
        },
        "decimal" => {
            if is_decimal(t) {
                Ok(t.to_owned())
            } else {
                bad("decimal")
            }
        }
        "boolean" => match t.to_ascii_lowercase().as_str() {
            v @ ("true" | "false") => Ok(v.to_owned()),
            _ => Err(format!("invalid boolean `{t}`: true or false")),
        },
        "uuid" | "timeuuid" => {
            if is_uuid(t) {
                Ok(t.to_owned())
            } else {
                bad("uuid")
            }
        }
        "blob" => {
            let hex = t
                .strip_prefix("0x")
                .or_else(|| t.strip_prefix("0X"))
                .unwrap_or(t);
            if hex.len().is_multiple_of(2) && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                Ok(format!("0x{hex}"))
            } else {
                bad("blob (hexadecimal bytes)")
            }
        }
        "duration" => {
            let ok = !t.is_empty()
                && t.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b':' | b'.'));
            if ok {
                Ok(t.to_owned())
            } else {
                bad("duration")
            }
        }
        // timestamp, date, time, inet: a string constant.
        _ => Ok(quote_string(t)),
    }
}

/// The text of a JSON scalar, as a cell would hold it.
fn json_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// A JSON value as a literal of type `ty`.
fn json_literal(value: &Value, ty: &Ty, udts: &Udts) -> Result<String, String> {
    if value.is_null() {
        return Ok("null".to_owned());
    }
    let mismatch = || format!("`{value}` does not fit the type");
    let items = |value: &Value, item: &Ty| -> Result<Vec<String>, String> {
        value
            .as_array()
            .ok_or_else(mismatch)?
            .iter()
            .map(|v| json_literal(v, item, udts))
            .collect()
    };
    match ty {
        Ty::Native(name) => scalar(&json_text(value), name),
        Ty::List(item) | Ty::Vector(item) => Ok(format!("[{}]", items(value, item)?.join(", "))),
        Ty::Set(item) => Ok(format!("{{{}}}", items(value, item)?.join(", "))),
        Ty::Tuple(types) => {
            let values = value.as_array().ok_or_else(mismatch)?;
            if values.len() != types.len() {
                return Err(format!(
                    "a tuple of {} values, not {}",
                    types.len(),
                    values.len()
                ));
            }
            let parts: Result<Vec<String>, String> = values
                .iter()
                .zip(types)
                .map(|(v, t)| json_literal(v, t, udts))
                .collect();
            Ok(format!("({})", parts?.join(", ")))
        }
        Ty::Map(key, item) => {
            let entries = value.as_object().ok_or_else(mismatch)?;
            let parts: Result<Vec<String>, String> = entries
                .iter()
                .map(|(k, v)| {
                    let k = match key.as_ref() {
                        Ty::Native(name) => scalar(k, name)?,
                        other => json_literal(
                            &serde_json::from_str(k).map_err(|e| e.to_string())?,
                            other,
                            udts,
                        )?,
                    };
                    Ok(format!("{k}: {}", json_literal(v, item, udts)?))
                })
                .collect();
            Ok(format!("{{{}}}", parts?.join(", ")))
        }
        Ty::Udt(name) => {
            let fields = udts
                .get(name)
                .ok_or_else(|| format!("the type {name} is not known"))?;
            let entries = value.as_object().ok_or_else(mismatch)?;
            let parts: Result<Vec<String>, String> = entries
                .iter()
                .map(|(k, v)| {
                    let (_, ty) = fields
                        .iter()
                        .find(|(f, _)| f == k)
                        .ok_or_else(|| format!("the type {name} has no field {k}"))?;
                    Ok(format!(
                        "{}: {}",
                        quote_ident(k),
                        json_literal(v, &parse_type(ty), udts)?
                    ))
                })
                .collect();
            Ok(format!("{{{}}}", parts?.join(", ")))
        }
    }
}

/// The literal of a cell's text, `null` for NULL.
fn literal(text: Option<&str>, type_name: &str, udts: &Udts) -> Result<String, String> {
    let Some(text) = text else {
        return Ok("null".to_owned());
    };
    match parse_type(type_name) {
        Ty::Native(name) => scalar(text, &name),
        other => {
            let json: Value = serde_json::from_str(text)
                .map_err(|e| format!("invalid JSON for the {type_name} `{text}`: {e}"))?;
            json_literal(&json, &other, udts)
        }
    }
}

/// The statements for a table's edits.
struct Builder<'a> {
    edits: &'a Edits<'a>,
    target: String,
}

impl Builder<'_> {
    fn name(&self, column: usize) -> &str {
        &self.edits.columns[column]
    }

    fn value(&self, column: usize, text: Option<&str>) -> Result<String, String> {
        let type_name = self.edits.types.get(column).map_or("text", String::as_str);
        literal(text, type_name, self.edits.udts)
            .map_err(|e| format!("column {}: {e}", self.name(column)))
    }

    fn is_key(&self, column: usize) -> bool {
        self.edits.key.contains(&column)
    }

    /// `k1 = … AND k2 = …` from a loaded row.
    fn where_clause(&self, loaded: &[Option<String>]) -> Result<String, String> {
        let parts: Result<Vec<String>, String> = self
            .edits
            .key
            .iter()
            .map(|&c| {
                let text = loaded.get(c).and_then(Option::as_deref).ok_or_else(|| {
                    format!(
                        "the primary key column {} of the row is empty",
                        self.name(c)
                    )
                })?;
                Ok(format!(
                    "{} = {}",
                    quote_ident(self.name(c)),
                    self.value(c, Some(text))?
                ))
            })
            .collect();
        let parts = parts?;
        if parts.is_empty() {
            return Err("the table has no primary key to find its rows by".to_owned());
        }
        Ok(parts.join(" AND "))
    }

    fn assignments(&self, values: &[(usize, Option<&str>)]) -> Result<String, String> {
        let parts: Result<Vec<String>, String> = values
            .iter()
            .map(|&(c, text)| {
                Ok(format!(
                    "{} = {}",
                    quote_ident(self.name(c)),
                    self.value(c, text)?
                ))
            })
            .collect();
        Ok(parts?.join(", "))
    }

    fn insert(&self, values: &[(usize, Option<&str>)]) -> Result<String, String> {
        let mut names = Vec::new();
        let mut literals = Vec::new();
        for &(c, text) in values.iter().filter(|(_, t)| t.is_some()) {
            names.push(quote_ident(self.name(c)));
            literals.push(self.value(c, text)?);
        }
        Ok(format!(
            "INSERT INTO {} ({}) VALUES ({})",
            self.target,
            names.join(", "),
            literals.join(", ")
        ))
    }

    fn update(&self, update: &Update) -> Result<(String, String), String> {
        if let Some(&(c, _)) = update.set.iter().find(|(c, _)| self.is_key(*c)) {
            return Err(format!(
                "the primary key column {} cannot change: delete the row and insert another",
                self.name(c)
            ));
        }
        let new: Vec<(usize, Option<&str>)> =
            update.set.iter().map(|(c, v)| (*c, v.as_deref())).collect();
        let old: Vec<(usize, Option<&str>)> = update
            .set
            .iter()
            .map(|(c, _)| (*c, update.loaded.get(*c).and_then(Option::as_deref)))
            .collect();
        let at = self.where_clause(&update.loaded)?;
        Ok((
            format!(
                "UPDATE {} SET {} WHERE {at}",
                self.target,
                self.assignments(&new)?
            ),
            format!(
                "UPDATE {} SET {} WHERE {at}",
                self.target,
                self.assignments(&old)?
            ),
        ))
    }

    fn new_row(&self, set: &[(usize, Option<String>)]) -> Result<(String, String), String> {
        let values: Vec<(usize, Option<&str>)> =
            set.iter().map(|(c, v)| (*c, v.as_deref())).collect();
        let mut row: Vec<Option<String>> = vec![None; self.edits.columns.len()];
        for &(c, text) in &values {
            row[c] = text.map(str::to_owned);
        }
        for &c in self.edits.key {
            if row[c].is_none() {
                return Err(format!(
                    "a new row needs a value for the primary key column {}",
                    self.name(c)
                ));
            }
        }
        Ok((
            self.insert(&values)?,
            format!(
                "DELETE FROM {} WHERE {}",
                self.target,
                self.where_clause(&row)?
            ),
        ))
    }

    fn deleted(&self, loaded: &[Option<String>]) -> Result<(String, Option<String>), String> {
        let values: Vec<(usize, Option<&str>)> = loaded
            .iter()
            .enumerate()
            .filter(|(_, v)| v.is_some())
            .map(|(c, v)| (c, v.as_deref()))
            .collect();
        // CQL refuses an INSERT on a counter table, so such a row has no undo but the note
        // that `script` adds.
        let undo = if self.counter_table() {
            None
        } else {
            Some(self.insert(&values)?)
        };
        Ok((
            format!(
                "DELETE FROM {} WHERE {}",
                self.target,
                self.where_clause(loaded)?
            ),
            undo,
        ))
    }

    /// Whether the table has counter columns, and so takes no INSERT.
    fn counter_table(&self) -> bool {
        self.edits.types.iter().any(|t| t == "counter")
    }
}

/// The statements that save `edits`, and the ones that put the rows back. Deletes come first,
/// so that a key freed by a delete can be written again.
pub fn script(edits: &Edits<'_>) -> Result<Script, String> {
    let builder = Builder {
        edits,
        target: qualified(edits.keyspace, edits.table),
    };
    let mut deletes = Vec::new();
    let mut updates = Vec::new();
    let mut inserts = Vec::new();
    let (mut undo_deletes, mut undo_updates, mut undo_inserts) =
        (Vec::new(), Vec::new(), Vec::new());
    for loaded in &edits.deletes {
        let (command, undo) = builder.deleted(loaded)?;
        deletes.push(command);
        undo_deletes.extend(undo);
        if builder.counter_table() {
            undo_deletes
                .push("-- the counters of the deleted rows cannot be written back".to_owned());
        }
    }
    for update in &edits.updates {
        let (command, undo) = builder.update(update)?;
        updates.push(command);
        undo_updates.push(undo);
    }
    for set in &edits.inserts {
        let (command, undo) = builder.new_row(set)?;
        inserts.push(command);
        undo_inserts.push(undo);
    }
    if !inserts.is_empty() {
        undo_inserts.push(
            "-- an insert over a row that existed replaced its columns: those values are not restored"
                .to_owned(),
        );
    }
    let mut commands = deletes;
    commands.extend(updates);
    commands.extend(inserts);
    // The undo goes the other way: inserts first.
    let mut undo = undo_inserts;
    undo.extend(undo_updates);
    undo.extend(undo_deletes);
    Ok(Script { commands, undo })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[allow(clippy::unnecessary_wraps)]
    fn some(text: &str) -> Option<String> {
        Some(text.to_owned())
    }

    #[test]
    fn writes_scalars_by_type() {
        let none = Udts::new();
        let lit = |text: &str, ty: &str| literal(Some(text), ty, &none);
        assert_eq!(lit("it's", "text").unwrap(), "'it''s'");
        assert_eq!(lit(" 42 ", "int").unwrap(), "42");
        assert!(lit("4.2", "int").is_err());
        assert_eq!(lit("-1.5e3", "decimal").unwrap(), "-1.5e3");
        assert_eq!(lit("NaN", "double").unwrap(), "NaN");
        assert_eq!(lit("inf", "double").unwrap(), "Infinity");
        assert_eq!(lit("-inf", "float").unwrap(), "-Infinity");
        assert_eq!(lit("Infinity", "double").unwrap(), "Infinity");
        assert!(lit("infinity!", "double").is_err());
        assert_eq!(lit("TRUE", "boolean").unwrap(), "true");
        assert_eq!(
            lit("123e4567-e89b-12d3-a456-426614174000", "uuid").unwrap(),
            "123e4567-e89b-12d3-a456-426614174000"
        );
        assert!(lit("123", "uuid").is_err());
        assert_eq!(lit("0xDEAD", "blob").unwrap(), "0xDEAD");
        assert_eq!(lit("beef", "blob").unwrap(), "0xbeef");
        assert!(lit("0xabc", "blob").is_err());
        assert_eq!(
            lit("2024-03-09T13:05:09.250Z", "timestamp").unwrap(),
            "'2024-03-09T13:05:09.250Z'"
        );
        assert_eq!(lit("1y2mo3d", "duration").unwrap(), "1y2mo3d");
        assert!(lit("1", "counter").is_err());
        assert_eq!(literal(None, "int", &none).unwrap(), "null");
    }

    #[test]
    fn writes_collections_from_json() {
        let mut udts = Udts::new();
        udts.insert(
            "address".into(),
            vec![
                ("street".into(), "text".into()),
                ("zip".into(), "frozen<list<int>>".into()),
            ],
        );
        let lit = |text: &str, ty: &str| literal(Some(text), ty, &udts);
        assert_eq!(lit(r#"["a","b'"]"#, "list<text>").unwrap(), "['a', 'b''']");
        assert_eq!(lit("[1,2]", "set<int>").unwrap(), "{1, 2}");
        assert_eq!(lit("[]", "set<int>").unwrap(), "{}");
        assert_eq!(
            lit(r#"{"a":[1],"b":[]}"#, "map<text, list<int>>").unwrap(),
            "{'a': [1], 'b': []}"
        );
        assert_eq!(lit(r#"{"7":"x"}"#, "map<int, text>").unwrap(), "{7: 'x'}");
        assert_eq!(
            lit(r#"[1,"x",null]"#, "tuple<int, text, uuid>").unwrap(),
            "(1, 'x', null)"
        );
        assert!(lit("[1]", "tuple<int, text>").is_err());
        assert_eq!(
            lit(r#"{"street":"Main","zip":[1,2]}"#, "address").unwrap(),
            "{street: 'Main', zip: [1, 2]}"
        );
        assert!(lit(r#"{"nope":1}"#, "address").is_err());
        assert!(lit("not json", "list<int>").is_err());
        assert!(lit(r#"["x"]"#, "list<int>").is_err());
        assert_eq!(lit("[0.5,1.5]", "vector<float, 2>").unwrap(), "[0.5, 1.5]");
        assert_eq!(lit("[1]", "frozen<set<int>>").unwrap(), "{1}");
    }

    fn edits<'a>(
        columns: &'a [String],
        types: &'a [String],
        key: &'a [usize],
        udts: &'a Udts,
    ) -> Edits<'a> {
        Edits {
            keyspace: "shop",
            table: "Items",
            columns,
            types,
            key,
            udts,
            updates: Vec::new(),
            inserts: Vec::new(),
            deletes: Vec::new(),
        }
    }

    #[test]
    fn saves_and_undoes_each_kind_of_change() {
        let columns = cols(&["id", "day", "name", "tags"]);
        let types = cols(&["int", "date", "text", "set<text>"]);
        let udts = Udts::new();
        let mut e = edits(&columns, &types, &[0, 1], &udts);
        e.updates.push(Update {
            loaded: vec![some("1"), some("2024-01-02"), some("old"), None],
            set: vec![(2, some("new")), (3, some(r#"["a"]"#))],
        });
        e.inserts.push(vec![
            (0, some("2")),
            (1, some("2024-01-03")),
            (2, None),
            (3, some("[]")),
        ]);
        e.deletes
            .push(vec![some("3"), some("2024-01-04"), some("x"), None]);
        let script = script(&e).unwrap();
        assert_eq!(
            script.commands,
            [
                "DELETE FROM shop.\"Items\" WHERE id = 3 AND day = '2024-01-04'",
                "UPDATE shop.\"Items\" SET name = 'new', tags = {'a'} WHERE id = 1 AND day = '2024-01-02'",
                "INSERT INTO shop.\"Items\" (id, day, tags) VALUES (2, '2024-01-03', {})",
            ]
        );
        assert_eq!(
            script.undo,
            [
                "DELETE FROM shop.\"Items\" WHERE id = 2 AND day = '2024-01-03'",
                "-- an insert over a row that existed replaced its columns: those values are not restored",
                "UPDATE shop.\"Items\" SET name = 'old', tags = null WHERE id = 1 AND day = '2024-01-02'",
                "INSERT INTO shop.\"Items\" (id, day, name) VALUES (3, '2024-01-04', 'x')",
            ]
        );
    }

    #[test]
    fn a_deleted_counter_row_has_only_a_note_for_undo() {
        let columns = cols(&["id", "n", "hits"]);
        let types = cols(&["int", "int", "counter"]);
        let udts = Udts::new();
        let mut e = edits(&columns, &types, &[0], &udts);
        e.deletes.push(vec![some("1"), some("2"), some("5")]);
        let script = script(&e).unwrap();
        assert_eq!(script.commands, ["DELETE FROM shop.\"Items\" WHERE id = 1"]);
        assert_eq!(
            script.undo,
            ["-- the counters of the deleted rows cannot be written back"]
        );
    }

    #[test]
    fn refuses_what_cql_cannot_do() {
        let columns = cols(&["id", "n", "hits"]);
        let types = cols(&["int", "int", "counter"]);
        let udts = Udts::new();
        let mut e = edits(&columns, &types, &[0], &udts);
        e.updates.push(Update {
            loaded: vec![some("1"), some("2"), None],
            set: vec![(0, some("9"))],
        });
        assert!(script(&e).unwrap_err().contains("cannot change"));
        e.updates.clear();
        e.updates.push(Update {
            loaded: vec![some("1"), some("2"), some("5")],
            set: vec![(2, some("6"))],
        });
        assert!(script(&e).unwrap_err().contains("counter"));
        e.updates.clear();
        e.inserts.push(vec![(1, some("2"))]);
        assert!(
            script(&e)
                .unwrap_err()
                .contains("needs a value for the primary key")
        );
        e.inserts.clear();
        e.updates.push(Update {
            loaded: vec![None, some("2"), None],
            set: vec![(1, some("3"))],
        });
        assert!(script(&e).unwrap_err().contains("is empty"));
        e.updates.clear();
        e.inserts.push(vec![(0, some("1")), (1, some("abc"))]);
        assert!(script(&e).unwrap_err().contains("column n"));
        let no_key = edits(&columns, &types, &[], &udts);
        let mut no_key = no_key;
        no_key.deletes.push(vec![some("1"), None, None]);
        assert!(script(&no_key).unwrap_err().contains("no primary key"));
    }

    #[test]
    fn finds_the_columns_that_need_user_types() {
        let types = |names: &[&str]| names.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(!uses_udts(&types(&[
            "int",
            "map<text, list<int>>",
            "tuple<int, text>"
        ])));
        assert!(uses_udts(&types(&["int", "address"])));
        assert!(uses_udts(&types(&["map<text, frozen<address>>"])));
    }

    #[test]
    fn parses_type_names() {
        assert_eq!(
            parse_type("frozen<list<int>>"),
            Ty::List(Box::new(Ty::Native("int".into())))
        );
        assert_eq!(parse_type("ks.\"Addr\""), Ty::Udt("Addr".into()));
        assert_eq!(
            parse_type("vector<float, 3>"),
            Ty::Vector(Box::new(Ty::Native("float".into())))
        );
        assert_eq!(
            parse_type("map<text, frozen<tuple<int, text>>>"),
            Ty::Map(
                Box::new(Ty::Native("text".into())),
                Box::new(Ty::Tuple(vec![
                    Ty::Native("int".into()),
                    Ty::Native("text".into())
                ]))
            )
        );
    }
}
