//! The statements that save the grid's changes to the documents of a collection, and those
//! that undo them. Each is a mongosh statement ([`crate::shell`]) that changes one document,
//! run one by one by `execute_all`:
//!
//! - an edited document is `updateOne({ _id: … }, { $set: { … } })`. A NULL cell sets the
//!   field to `null`;
//! - a new document is `insertOne({ … })`. Cells left NULL are not sent. Without an `_id`
//!   the app makes an `ObjectId`, so that the undo can name the document;
//! - a deleted document is `deleteOne({ _id: … })`.
//!
//! A document is found by its `_id`, which MongoDB never changes: an edit of it is refused
//! (delete the document and insert it again). Values are written by the BSON type of their
//! column ([`META_DB_TYPE`]), from the text the grid shows. Columns the grid cannot write back
//! exactly (`mixed`, `binData`, regular expressions, timestamps…) are refused, and so are
//! field names that `$set` would read as a path or an operator (`a.b`, `$a`).
//!
//! [`META_DB_TYPE`]: platypus_core::META_DB_TYPE

use std::str::FromStr;

use mongodb::bson::{Bson, Decimal128, Document, doc, oid::ObjectId};

use crate::generate::collection_ref;
use crate::literal::{Reader, quote, resolve_extended, to_shell};
use crate::shell::{self, CollectionOp, Command};

/// A loaded row changed: its values as loaded, and the new values of some columns (`None`
/// for NULL).
#[derive(Debug, Clone)]
pub struct Update {
    pub loaded: Vec<Option<String>>,
    pub set: Vec<(usize, Option<String>)>,
}

/// The changes to save to the documents of one collection.
#[derive(Debug, Clone)]
pub struct Edits<'a> {
    /// The database of the collection; `None` for the session's current one.
    pub db: Option<&'a str>,
    pub collection: &'a str,
    /// The result's column names, by index.
    pub columns: &'a [String],
    /// Their BSON types (`long`, `string`, `document`…), by index.
    pub types: &'a [String],
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
    /// Statements, and `//` comments for what cannot be undone exactly.
    pub undo: Vec<String>,
}

/// The collection a `find` reads, when its documents can be edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The database the statement names; `None` for the session's current one.
    pub db: Option<String>,
    pub collection: String,
}

/// Whether `value` is the `1` or `true` of an inclusion projection.
fn includes(value: &Bson) -> bool {
    match value {
        Bson::Boolean(b) => *b,
        Bson::Int32(n) => *n != 0,
        Bson::Int64(n) => *n != 0,
        Bson::Double(n) => *n != 0.0,
        _ => false,
    }
}

/// Whether a projection lists fields to include, the `_id` among the results, with no
/// computed values or paths into documents: the fields it returns are whole.
fn keeps_whole_fields(projection: &Document) -> bool {
    projection
        .iter()
        .all(|(name, value)| !name.contains('.') && includes(value))
}

/// The collection a statement reads when its result is that collection's documents: a plain
/// `find`, with a filter, `sort`, `skip` and `limit`, and a projection that only lists fields to
/// include (`_id` too). Aggregations, `findOne`, counts and the like are not.
#[must_use]
pub fn source(src: &str) -> Option<Source> {
    let Ok(Command::Collection {
        db,
        collection,
        op: CollectionOp::Find(find),
    }) = shell::parse(src.trim().trim_end_matches(';'))
    else {
        return None;
    };
    let whole = find
        .projection
        .as_ref()
        .is_none_or(|p| p.is_empty() || keeps_whole_fields(p));
    (!find.one && whole).then_some(Source { db, collection })
}

/// A field name `$set` and `insertOne` take as it is.
fn field_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.contains('.') || name.starts_with('$') {
        return Err(format!(
            "the field `{name}` has a `.` in its name or starts with `$`, which an update would \
             read as a path or an operator: change that document with a script"
        ));
    }
    Ok(())
}

/// The types of the grid's columns it can write back exactly.
const WRITABLE: &[&str] = &[
    "bool", "int", "long", "double", "decimal", "string", "objectId", "date", "document", "array",
];

fn parse_with<T>(text: &str, what: &str, parse: impl Fn(&str) -> Option<T>) -> Result<T, String> {
    parse(text).ok_or_else(|| format!("invalid {what} `{text}`"))
}

/// A double from its text: the grid shows the special values as Arrow prints them.
fn double(text: &str) -> Option<f64> {
    match text {
        "NaN" | "nan" => Some(f64::NAN),
        "Infinity" | "inf" | "+inf" => Some(f64::INFINITY),
        "-Infinity" | "-inf" => Some(f64::NEG_INFINITY),
        _ => text.parse::<f64>().ok().filter(|d| d.is_finite()),
    }
}

/// A document or an array from the relaxed Extended JSON the grid shows.
fn structured(text: &str, db_type: &str) -> Result<Bson, String> {
    let mut reader = Reader::new(text);
    let parsed = reader
        .value()
        .ok()
        .filter(|_| reader.at_end())
        .map(resolve_extended);
    match (db_type, parsed) {
        ("document", Some(value @ Bson::Document(_))) | ("array", Some(value @ Bson::Array(_))) => {
            Ok(value)
        }
        ("document", _) => Err(format!("`{text}` is not a document: write it as {{ … }}")),
        _ => Err(format!("`{text}` is not an array: write it as [ … ]")),
    }
}

/// The date a cell's text stands for: the grid shows RFC 3339 with a space for the `T`, and
/// `ISODate` reads that and the shorter forms (a day alone, no seconds, no zone).
fn date(text: &str) -> Result<Bson, String> {
    let call = format!("ISODate({})", quote(text));
    Reader::new(&call)
        .value()
        .map_err(|_| format!("invalid date `{text}`: write it like 2024-01-02 10:30:00"))
}

/// The BSON value of a cell's text, by the type of its column.
fn value(text: &str, db_type: &str) -> Result<Bson, String> {
    let t = text.trim();
    match db_type {
        "string" => Ok(Bson::String(text.to_owned())),
        "bool" => match t.to_ascii_lowercase().as_str() {
            "true" => Ok(Bson::Boolean(true)),
            "false" => Ok(Bson::Boolean(false)),
            _ => Err(format!("invalid boolean `{t}`: true or false")),
        },
        "int" => match t.parse::<i32>() {
            Ok(n) => Ok(Bson::Int32(n)),
            Err(_) if t.parse::<i64>().is_ok() => Err(format!(
                "`{t}` does not fit a 32-bit int (-2147483648 to 2147483647)"
            )),
            Err(_) => Err(format!("invalid integer `{t}`")),
        },
        "long" => parse_with(t, "integer", |s| s.parse().ok()).map(Bson::Int64),
        "double" => parse_with(t, "number", double).map(Bson::Double),
        "decimal" => {
            parse_with(t, "decimal number", |s| Decimal128::from_str(s).ok()).map(Bson::Decimal128)
        }
        "objectId" => parse_with(t, "ObjectId (24 hexadecimal characters)", |s| {
            ObjectId::parse_str(s).ok()
        })
        .map(Bson::ObjectId),
        "date" => date(t),
        "document" | "array" => structured(t, db_type),
        other => Err(unwritable(other)),
    }
}

/// Why the grid cannot write a column of type `db_type`.
fn unwritable(db_type: &str) -> String {
    match db_type {
        "mixed" => "its values have several types, which their text cannot tell apart (a string from a number): change them with a script"
            .to_owned(),
        "" => "its type is not known: change it with a script".to_owned(),
        other => format!(
            "the grid cannot write {other} values back exactly: change them with a script"
        ),
    }
}

/// The statements for a collection's edits.
struct Builder<'a> {
    edits: &'a Edits<'a>,
    /// `db.name`, as the statements address the collection.
    target: String,
}

impl Builder<'_> {
    fn name(&self, column: usize) -> &str {
        &self.edits.columns[column]
    }

    fn db_type(&self, column: usize) -> &str {
        self.edits.types.get(column).map_or("", String::as_str)
    }

    /// A cell as a BSON value (NULL as `null`), refusing the columns it cannot be written to.
    fn cell(&self, column: usize, text: Option<&str>) -> Result<Bson, String> {
        let name = self.name(column);
        let ty = self.db_type(column);
        let written = field_name(name).and_then(|()| {
            if !WRITABLE.contains(&ty) {
                return Err(unwritable(ty));
            }
            text.map_or(Ok(Bson::Null), |text| value(text, ty))
        });
        written.map_err(|e| format!("column {name}: {e}"))
    }

    /// The `_id` of a loaded row.
    fn id(&self, loaded: &[Option<String>]) -> Result<Bson, String> {
        let column = self
            .edits
            .columns
            .iter()
            .position(|c| c == "_id")
            .ok_or("the result has no _id column: a document is found by its _id")?;
        let text = loaded
            .get(column)
            .and_then(Option::as_deref)
            .ok_or("the document has no _id")?;
        self.id_value(column, text)
    }

    fn id_value(&self, column: usize, text: &str) -> Result<Bson, String> {
        value(text, self.db_type(column)).map_err(|e| format!("column _id: {e}"))
    }

    fn statement(&self, method: &str, arguments: &[Document]) -> String {
        let arguments: Vec<String> = arguments
            .iter()
            .map(|d| to_shell(&Bson::Document(d.clone())))
            .collect();
        format!("{}.{method}({})", self.target, arguments.join(", "))
    }

    /// `_id` as it is shown in a comment.
    fn shown(id: &Bson) -> String {
        to_shell(id)
    }

    /// The `$set` of an update, and the one that puts the loaded values back, with a comment
    /// for each field that was NULL: it may have been missing.
    fn update(
        &self,
        update: &Update,
        script: &mut Script,
        undo: &mut Vec<String>,
    ) -> Result<(), String> {
        let id = self.id(&update.loaded)?;
        let (mut new, mut old) = (Document::new(), Document::new());
        let mut nulls = Vec::new();
        for (column, text) in &update.set {
            let name = self.name(*column);
            if name == "_id" {
                if text.as_deref() != update.loaded.get(*column).and_then(Option::as_deref) {
                    return Err(
                        "the _id of a document cannot be changed: delete the document and \
                                insert it again"
                            .to_owned(),
                    );
                }
                continue;
            }
            let before = update.loaded.get(*column).and_then(Option::as_deref);
            new.insert(name, self.cell(*column, text.as_deref())?);
            old.insert(name, self.cell(*column, before)?);
            if before.is_none() {
                nulls.push(name);
            }
        }
        if new.is_empty() {
            return Ok(());
        }
        let key = doc! { "_id": id.clone() };
        script
            .commands
            .push(self.statement("updateOne", &[key.clone(), doc! { "$set": new }]));
        if !nulls.is_empty() {
            undo.push(format!(
                "// _id {}: {} was NULL, empty or missing, and comes back as null",
                Self::shown(&id),
                nulls.join(", ")
            ));
        }
        undo.push(self.statement("updateOne", &[key, doc! { "$set": old }]));
        Ok(())
    }

    /// The `insertOne` of a new row and the `deleteOne` that removes it.
    fn insert(
        &self,
        set: &[(usize, Option<String>)],
        script: &mut Script,
        undo: &mut Vec<String>,
    ) -> Result<(), String> {
        let mut set: Vec<_> = set.iter().collect();
        set.sort_by_key(|(column, _)| *column);
        let given = set.iter().find_map(|(column, text)| {
            (self.name(*column) == "_id")
                .then(|| text.as_deref().map(|t| self.id_value(*column, t)))
                .flatten()
        });
        let id = match given {
            Some(id) => id?,
            None => Bson::ObjectId(ObjectId::new()),
        };
        let mut document = doc! { "_id": id.clone() };
        for (column, text) in set {
            if self.name(*column) == "_id" {
                continue;
            }
            if let Some(text) = text {
                document.insert(self.name(*column), self.cell(*column, Some(text))?);
            }
        }
        script
            .commands
            .push(self.statement("insertOne", &[document]));
        undo.push(self.statement("deleteOne", &[doc! { "_id": id }]));
        Ok(())
    }

    /// The `deleteOne` of a loaded row and the `insertOne` that puts it back, with what the
    /// grid cannot restore noted before it.
    fn delete(
        &self,
        loaded: &[Option<String>],
        script: &mut Script,
        undo: &mut Vec<String>,
    ) -> Result<(), String> {
        let id = self.id(loaded)?;
        let mut document = doc! { "_id": id.clone() };
        let mut lost = Vec::new();
        for (column, text) in loaded.iter().enumerate() {
            let name = self.name(column);
            let Some(text) = text else { continue };
            if name == "_id" {
                continue;
            }
            match self.cell(column, Some(text)) {
                Ok(v) => {
                    document.insert(name, v);
                }
                Err(_) => lost.push(name),
            }
        }
        script
            .commands
            .push(self.statement("deleteOne", &[doc! { "_id": id.clone() }]));
        if !lost.is_empty() {
            undo.push(format!(
                "// _id {}: {} cannot be put back exactly, and is left out",
                Self::shown(&id),
                lost.join(", ")
            ));
        }
        undo.push(self.statement("insertOne", &[document]));
        Ok(())
    }
}

/// The statements that save `edits`, and the ones that undo them. Deletes come first, then
/// updates, then inserts; the undo does the opposite, in the opposite order.
///
/// # Errors
/// The message for a change that cannot be written: an edit of `_id`, a column or value the
/// grid cannot write, a result without `_id`.
pub fn script(edits: &Edits<'_>) -> Result<Script, String> {
    if edits.types.len() != edits.columns.len() {
        return Err("every column needs its type".to_owned());
    }
    let builder = Builder {
        edits,
        target: collection_ref(edits.db, edits.collection),
    };
    let mut script = Script {
        commands: Vec::new(),
        undo: Vec::new(),
    };
    let (mut undo_deletes, mut undo_updates, mut undo_inserts) =
        (Vec::new(), Vec::new(), Vec::new());
    for loaded in &edits.deletes {
        builder.delete(loaded, &mut script, &mut undo_deletes)?;
    }
    for update in &edits.updates {
        builder.update(update, &mut script, &mut undo_updates)?;
    }
    for set in &edits.inserts {
        builder.insert(set, &mut script, &mut undo_inserts)?;
    }
    if !edits.deletes.is_empty() {
        undo_deletes.insert(
            0,
            "// Deleted documents come back with the fields the result showed, minus the NULL ones"
                .to_owned(),
        );
    }
    script.undo.extend(undo_inserts);
    script.undo.extend(undo_updates);
    script.undo.extend(undo_deletes);
    Ok(script)
}

#[cfg(test)]
mod tests;
