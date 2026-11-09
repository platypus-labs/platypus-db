//! The statements that save the grid's changes to the documents of a collection, and those
//! that undo them. Each is a mongosh statement ([`crate::shell`]) that changes one document,
//! run one by one by `execute_all`:
//!
//! - an edited document is `updateOne({ _id: … }, { $set: { … } })`. A NULL cell sets the
//!   field to `null`. A changed number keeps the numeric type the document stores when that
//!   type holds it ([`number`]): the update is then a pipeline, `[{ $set: { n: { $switch: … },
//!   other: { $literal: … } } }]`, whose other values are literals;
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

mod number;
mod proposed;

use mongodb::bson::{Bson, Document, doc, oid::ObjectId};

use crate::generate::collection_ref;
use crate::literal::{Reader, quote, resolve_extended, to_shell};
use crate::shell::{self, CollectionOp, Command};

use self::number::{Exact, NUMERIC};
pub use self::proposed::{ProposedFind, check_proposed_cell, find_proposed};

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
    // `returnKey` reads the index keys instead of the documents, `showRecordId` adds a field.
    let documents = ["returnKey", "showRecordId"]
        .iter()
        .all(|option| !find.options.get(option).is_some_and(includes));
    (!find.one && whole && documents).then_some(Source { db, collection })
}

/// Whether the documents of collection `collection` of database `db` can be saved in a
/// transaction: MongoDB runs none on the admin, config and local databases and on system
/// collections.
#[must_use]
pub fn transactional(db: &str, collection: &str) -> bool {
    crate::transaction::reachable(db, collection)
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

/// The number of a cell's text, in the numeric type of its column when that holds it, else in
/// the first that does: `3.5` in an `int` column is a double.
fn number(text: &str, db_type: &str) -> Result<Bson, String> {
    parse_with(text, "number", |t| Exact::of(t)?.by_type(db_type))
}

/// A document or an array from the relaxed Extended JSON the grid shows, whose integers are
/// ints and longs (a long beyond 2^53 as a double would lose digits).
fn structured(text: &str, db_type: &str) -> Result<Bson, String> {
    let mut reader = Reader::json(text);
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
        "int" | "long" | "double" | "decimal" => number(t, db_type),
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

/// A cell of field `name`, of type `db_type`, as a BSON value (NULL as `null`), refusing the
/// fields the grid cannot write.
fn cell(name: &str, db_type: &str, text: Option<&str>) -> Result<Bson, String> {
    let written = field_name(name).and_then(|()| {
        if !WRITABLE.contains(&db_type) {
            return Err(unwritable(db_type));
        }
        text.map_or(Ok(Bson::Null), |text| value(text, db_type))
    });
    written.map_err(|e| format!("column {name}: {e}"))
}

/// An `_id` of type `db_type`, in that type only: it finds the document.
fn id_value(db_type: &str, text: &str) -> Result<Bson, String> {
    let found = if NUMERIC.contains(&db_type) {
        parse_with(text.trim(), db_type, |t| Exact::of(t)?.as_type(db_type))
    } else {
        value(text, db_type)
    };
    found.map_err(|e| format!("column _id: {e}"))
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
        cell(self.name(column), self.db_type(column), text)
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

    /// An `_id`, in the type of its column only: it finds the document.
    fn id_value(&self, column: usize, text: &str) -> Result<Bson, String> {
        id_value(self.db_type(column), text)
    }

    /// What `$set` writes to column `column` for a cell: in a pipeline, a number in the type the
    /// document stores ([`Exact::kept`]) and any other value as a literal; `None` when the cell
    /// is not a number, which the plain `$set` writes as well. After [`Self::cell`], which
    /// refuses what cannot be written.
    fn kept(&self, column: usize, text: Option<&str>) -> Option<Bson> {
        let ty = self.db_type(column);
        let text = text.filter(|_| NUMERIC.contains(&ty))?;
        let kept = Exact::of(text.trim())?.kept(self.name(column), ty)?;
        Some(Bson::Document(kept))
    }

    /// The `$set` of `fields` (each with its pipeline form, if a number): the plain update when
    /// no number is among them, else the pipeline.
    fn set(fields: Vec<(&str, Bson, Option<Bson>)>) -> Bson {
        if fields.iter().all(|(_, _, kept)| kept.is_none()) {
            let set: Document = fields
                .into_iter()
                .map(|(n, v, _)| (n.to_owned(), v))
                .collect();
            return Bson::Document(doc! { "$set": set });
        }
        let set: Document = fields
            .into_iter()
            .map(|(name, value, kept)| {
                let value = kept.unwrap_or_else(|| Bson::Document(doc! { "$literal": value }));
                (name.to_owned(), value)
            })
            .collect();
        Bson::Array(vec![Bson::Document(doc! { "$set": set })])
    }

    fn statement(&self, method: &str, arguments: &[Document]) -> String {
        let arguments: Vec<Bson> = arguments.iter().cloned().map(Bson::Document).collect();
        self.call(method, &arguments)
    }

    fn call(&self, method: &str, arguments: &[Bson]) -> String {
        let arguments: Vec<String> = arguments.iter().map(to_shell).collect();
        format!("{}.{method}({})", self.target, arguments.join(", "))
    }

    /// `_id` as it is shown in a comment.
    fn shown(id: &Bson) -> String {
        to_shell(id)
    }

    /// The `$set` of an update, and the one that puts the loaded values back, with a comment
    /// for each field that was NULL (it may have been missing) and for each number whose type
    /// the edit may have changed (the undo puts the number back in the type it has then).
    fn update(
        &self,
        update: &Update,
        script: &mut Script,
        undo: &mut Vec<String>,
    ) -> Result<(), String> {
        let id = self.id(&update.loaded)?;
        let (mut new, mut old) = (Vec::new(), Vec::new());
        let (mut nulls, mut retyped) = (Vec::new(), Vec::new());
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
            let text = text.as_deref();
            new.push((name, self.cell(*column, text)?, self.kept(*column, text)));
            old.push((
                name,
                self.cell(*column, before)?,
                self.kept(*column, before),
            ));
            if before.is_none() {
                nulls.push(name);
            }
            let missing = text
                .filter(|_| NUMERIC.contains(&self.db_type(*column)))
                .and_then(|t| Exact::of(t.trim()))
                .map(|n| n.missing())
                .unwrap_or_default();
            if !missing.is_empty() {
                retyped.push(format!("{name} (not an exact {})", missing.join(" or ")));
            }
        }
        if new.is_empty() {
            return Ok(());
        }
        let key = Bson::Document(doc! { "_id": id.clone() });
        script
            .commands
            .push(self.call("updateOne", &[key.clone(), Self::set(new)]));
        let shown = Self::shown(&id);
        if !nulls.is_empty() {
            undo.push(format!(
                "// _id {shown}: {} was NULL, empty or missing, and comes back as null",
                nulls.join(", ")
            ));
        }
        if !retyped.is_empty() {
            undo.push(format!(
                "// _id {shown}: {} may have changed type, and comes back as a number of the type \
                 it has now",
                retyped.join(", ")
            ));
        }
        undo.push(self.call("updateOne", &[key, Self::set(old)]));
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
