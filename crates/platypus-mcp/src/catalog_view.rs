//! The user's notes, tags and sensitive marks (the app's `catalog.db`) as agents read them in
//! `describe_table`, `list_objects`, the table resource and `search_objects`.
//!
//! Everything here is additive: an object without catalog entries is described exactly as
//! before. The notes are text the user wrote about the database, so they are data: they go
//! through the connection's [`Redactor`], are cut to a size limit and are only ever JSON
//! string values, which cannot fake the structure of the tool's output.

use std::collections::{BTreeMap, HashMap, HashSet};

use platypus_core::{Connection, Error, ObjectMeta};
use platypus_store::{CatalogEntry, CatalogStore, SensitiveColumn};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::agent::Agent;
use crate::query::MAX_VALUE_CHARS;
use crate::redact::Redactor;

/// Characters of one note a result keeps, as for a value of a result.
const NOTE_CHARS: usize = MAX_VALUE_CHARS;

/// Characters of notes one call returns at most, over all its objects.
const NOTES_BUDGET: usize = 64 * 1024;

/// Said when the catalog cannot be read.
const UNAVAILABLE: &str = "the user's notes, tags and sensitive marks could not be read, so \
none are shown; the rest of this result is complete";

/// What the user attached to one table or view: the object itself and its columns.
#[derive(Debug, Default)]
struct Annotations {
    own: Note,
    columns: BTreeMap<String, Note>,
    marks: BTreeMap<String, bool>,
}

#[derive(Debug, Default, Clone)]
struct Note {
    text: String,
    tags: Vec<String>,
}

impl Note {
    fn is_empty(&self) -> bool {
        self.text.is_empty() && self.tags.is_empty()
    }
}

/// The catalog of one connection, read for one call.
#[derive(Debug, Default)]
pub(crate) struct CatalogView {
    /// By schema and object name, as the app keys them.
    objects: HashMap<(String, String), Annotations>,
    known_tags: HashSet<String>,
    unavailable: bool,
}

impl Agent {
    /// The catalog of `connection` of the agent's profile. A catalog that cannot be read is
    /// not an error here: these are read-only extras, unlike the marks that mask values
    /// (`Sensitivity::marks`), where not knowing would let values through. The result says
    /// that they are missing instead, so `describe_table` still answers.
    pub(crate) fn catalog_view(&self, connection: Uuid, database: &str) -> CatalogView {
        match self.profile().catalog() {
            Ok(None) => CatalogView::default(),
            Ok(Some(catalog)) => CatalogView::read(&catalog, connection, database),
            Err(_) => CatalogView {
                unavailable: true,
                ..CatalogView::default()
            },
        }
    }
}

/// Whether a key's `database` is the connection's own: the app leaves it empty for the
/// default database and names any other, which agents do not reach.
fn own_database(key_database: &str, database: &str) -> bool {
    key_database.is_empty() || key_database.eq_ignore_ascii_case(database)
}

impl CatalogView {
    fn read(catalog: &CatalogStore, connection: Uuid, database: &str) -> Self {
        let entries = catalog.entries(connection);
        let marks = catalog.sensitive_columns(connection);
        let mut view = Self {
            unavailable: entries.is_err() || marks.is_err(),
            ..Self::default()
        };
        // An entry keyed with the database's name gives way to one keyed as the app does.
        let mut entries = entries.unwrap_or_default();
        entries.retain(|e| own_database(&e.key.database, database));
        entries.sort_by_key(|e| std::cmp::Reverse(e.key.database.is_empty()));
        for entry in entries {
            view.add_entry(entry);
        }
        let mut marks = marks.unwrap_or_default();
        marks.retain(|m| own_database(&m.key.database, database));
        marks.sort_by_key(|m| std::cmp::Reverse(m.key.database.is_empty()));
        for mark in marks {
            view.add_mark(mark);
        }
        view
    }

    fn add_entry(&mut self, entry: CatalogEntry) {
        self.known_tags.extend(entry.tags.iter().cloned());
        let key = entry.key;
        let note = Note {
            text: entry.note,
            tags: entry.tags,
        };
        let slot = self.objects.entry((key.schema, key.object)).or_default();
        if key.column.is_empty() {
            slot.own = note;
        } else {
            slot.columns.insert(key.column, note);
        }
    }

    fn add_mark(&mut self, mark: SensitiveColumn) {
        let key = mark.key;
        if key.column.is_empty() {
            return;
        }
        self.objects
            .entry((key.schema, key.object))
            .or_default()
            .marks
            .insert(key.column, mark.sensitive);
    }

    fn of(&self, schema: &str, object: &str) -> Option<&Annotations> {
        self.objects.get(&(schema.to_owned(), object.to_owned()))
    }

    /// Adds the notes, tags and marks to the JSON of `describe_table` (`details`): on the
    /// table and on its columns.
    pub(crate) fn describe(
        &self,
        schema: &str,
        table: &str,
        details: &mut Value,
        redactor: &Redactor,
    ) {
        let Some(root) = details.as_object_mut() else {
            return;
        };
        if self.unavailable {
            root.insert("catalog_unavailable".to_owned(), json!(UNAVAILABLE));
        }
        let Some(found) = self.of(schema, table) else {
            return;
        };
        let mut notes = Notes::new(redactor);
        notes.put(root, &found.own);
        let columns = root.get_mut("columns").and_then(Value::as_array_mut);
        for column in columns.into_iter().flatten() {
            let Some(column) = column.as_object_mut() else {
                continue;
            };
            let name = column.get("name").and_then(Value::as_str).unwrap_or("");
            let (note, mark) = (found.columns.get(name), found.marks.get(name));
            let (note, mark) = (note.cloned(), mark.copied());
            if let Some(note) = note {
                notes.put(column, &note);
            }
            if let Some(mark) = mark {
                column.insert("sensitive".to_owned(), json!(mark));
            }
        }
    }

    /// Adds the notes and tags to the objects of `page`, what `list_objects` returns (the
    /// list, or the object holding it). When the catalog cannot be read, the list is wrapped
    /// in `{objects, catalog_unavailable}` so that the result can say so.
    pub(crate) fn list(&self, schema: &str, page: Value, redactor: &Redactor) -> Value {
        let mut page = page;
        let mut notes = Notes::new(redactor);
        let items = match &mut page {
            Value::Array(items) => Some(items),
            Value::Object(o) => o.get_mut("objects").and_then(Value::as_array_mut),
            _ => None,
        };
        for item in items.into_iter().flatten() {
            let Some(item) = item.as_object_mut() else {
                continue;
            };
            let name = item.get("name").and_then(Value::as_str).unwrap_or("");
            let own = self.of(schema, name).map(|a| a.own.clone());
            if let Some(own) = own {
                notes.put(item, &own);
            }
        }
        if !self.unavailable {
            return page;
        }
        match page {
            Value::Object(mut o) => {
                o.insert("catalog_unavailable".to_owned(), json!(UNAVAILABLE));
                Value::Object(o)
            }
            list => json!({ "objects": list, "catalog_unavailable": UNAVAILABLE }),
        }
    }

    /// The query `text` split into the tags of the catalog it names and the rest.
    pub(crate) fn query(&self, text: &str) -> SearchQuery {
        SearchQuery::parse(text, &|tag| self.known_tags.contains(tag))
    }

    /// How the object `object` of `schema` matches `query`: by comment, note, tag or (a
    /// column's) note. Names are matched by the driver's own search, not here.
    fn matches(
        &self,
        schema: &str,
        object: &ObjectMeta,
        query: &SearchQuery,
        redactor: &Redactor,
    ) -> Vec<Value> {
        let found = self.of(schema, &object.name);
        let hit = |matched: &str, column: Option<&str>| {
            json!({
                "schema": schema,
                "name": object.name,
                "kind": object.kind,
                "parent": object.parent,
                "matched": matched,
                "column": column,
            })
        };
        let comment = object.comment.as_deref().map(|c| redactor.apply(c));
        let note = found.map(|f| redactor.apply(&f.own.text));
        let needle = query.text.to_lowercase();
        let has = |s: Option<&String>| s.is_some_and(|s| s.to_lowercase().contains(&needle));
        if !query.tags.is_empty() {
            let tagged = found.is_some_and(|f| query.tags.iter().all(|t| f.own.tags.contains(t)));
            let text = needle.is_empty()
                || object.name.to_lowercase().contains(&needle)
                || has(comment.as_ref())
                || has(note.as_ref());
            return if tagged && text {
                vec![hit("tag", None)]
            } else {
                Vec::new()
            };
        }
        let mut out = Vec::new();
        if has(comment.as_ref()) {
            out.push(hit("comment", None));
        }
        if has(note.as_ref()) {
            out.push(hit("note", None));
        }
        for (column, note) in found.iter().flat_map(|f| &f.columns) {
            if has(Some(&redactor.apply(&note.text))) {
                out.push(hit("note", Some(column)));
            }
        }
        out
    }
}

/// A search text: the `#tag` words that are tags of the catalog, and what remains.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SearchQuery {
    /// Lower case, without `#`.
    pub(crate) tags: Vec<String>,
    pub(crate) text: String,
}

impl SearchQuery {
    /// As the app's navigator filter does (`parseFilter`): a word `#tag` after a space or at the
    /// start is a tag when `known`, else plain text, so `#temp` still finds a temp table;
    /// a `#` alone or inside a word is plain text.
    fn parse(text: &str, known: &dyn Fn(&str) -> bool) -> Self {
        let (mut tags, mut rest) = (Vec::new(), Vec::new());
        for word in text.split_whitespace() {
            let tag = word.trim_start_matches('#').to_lowercase();
            if word.starts_with('#') && !tag.is_empty() && known(&tag) {
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
            } else {
                rest.push(word);
            }
        }
        Self {
            tags,
            text: rest.join(" "),
        }
    }
}

/// The notes of one call, within [`NOTES_BUDGET`].
struct Notes<'a> {
    redactor: &'a Redactor,
    left: usize,
}

impl<'a> Notes<'a> {
    const fn new(redactor: &'a Redactor) -> Self {
        Self {
            redactor,
            left: NOTES_BUDGET,
        }
    }

    /// Puts `note` and `tags` in `target`; left out when empty. A note cut to the limits has
    /// `note_cut: true` beside it.
    fn put(&mut self, target: &mut Map<String, Value>, note: &Note) {
        if note.is_empty() {
            return;
        }
        if !note.text.is_empty() {
            let text = self.redactor.apply(&note.text);
            let room = NOTE_CHARS.min(self.left);
            let total = text.chars().count();
            let kept: String = text.chars().take(room).collect();
            self.left -= kept.chars().count();
            if !kept.is_empty() {
                target.insert("note".to_owned(), json!(kept));
            }
            if total > room {
                target.insert("note_cut".to_owned(), json!(true));
            }
        }
        if !note.tags.is_empty() {
            let tags: Vec<String> = note.tags.iter().map(|t| self.redactor.apply(t)).collect();
            target.insert("tags".to_owned(), json!(tags));
        }
    }
}

/// The objects of the non-system schemas of `conn` that `query` matches by comment, note or
/// tag. Only objects the connection lists are returned, so a note left on a dropped table
/// finds nothing.
pub(crate) async fn search(
    conn: &dyn Connection,
    view: &CatalogView,
    query: &SearchQuery,
    redactor: &Redactor,
) -> Result<Vec<Value>, Error> {
    let mut found = Vec::new();
    if query.text.is_empty() && query.tags.is_empty() {
        return Ok(found);
    }
    for schema in conn.schemas().await? {
        if schema.system {
            continue;
        }
        for object in conn.objects(&schema.name).await? {
            found.extend(view.matches(&schema.name, &object, query, redactor));
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use platypus_core::ObjectKind;
    use platypus_store::CatalogKey;

    use super::*;

    fn key(object: &str, column: &str) -> CatalogKey {
        CatalogKey {
            database: String::new(),
            schema: "main".to_owned(),
            object: object.to_owned(),
            column: column.to_owned(),
        }
    }

    fn entry(object: &str, column: &str, note: &str, tags: &[&str]) -> CatalogEntry {
        CatalogEntry {
            key: key(object, column),
            note: note.to_owned(),
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        }
    }

    fn view(entries: Vec<CatalogEntry>, marks: &[(&str, &str, bool)]) -> CatalogView {
        let mut v = CatalogView::default();
        for e in entries {
            v.add_entry(e);
        }
        for (object, column, sensitive) in marks {
            v.add_mark(SensitiveColumn {
                key: key(object, column),
                sensitive: *sensitive,
            });
        }
        v
    }

    fn table() -> Value {
        json!({ "columns": [{ "name": "id" }, { "name": "email" }], "indexes": [] })
    }

    #[test]
    fn an_object_without_entries_is_left_as_it_was() {
        let v = view(vec![entry("other", "", "x", &[])], &[]);
        let mut details = table();
        v.describe("main", "users", &mut details, &Redactor::default());
        assert_eq!(details, table());
        let page = json!([{ "name": "users", "kind": "table" }]);
        assert_eq!(v.list("main", page.clone(), &Redactor::default()), page);
    }

    #[test]
    fn notes_tags_and_marks_go_on_the_table_and_its_columns() {
        let v = view(
            vec![
                entry("users", "", "who signs up", &["core", "pii"]),
                entry("users", "email", "login name", &["pii"]),
            ],
            &[("users", "email", true), ("users", "id", false)],
        );
        let mut details = table();
        v.describe("main", "users", &mut details, &Redactor::default());
        assert_eq!(details["note"], "who signs up");
        assert_eq!(details["tags"], json!(["core", "pii"]));
        assert_eq!(
            details["columns"][0],
            json!({ "name": "id", "sensitive": false })
        );
        assert_eq!(
            details["columns"][1],
            json!({ "name": "email", "note": "login name", "tags": ["pii"], "sensitive": true })
        );
        assert!(details.get("catalog_unavailable").is_none());
    }

    #[test]
    fn a_note_is_redacted_cut_and_stays_a_string() {
        let mut redactor = Redactor::default();
        redactor.add("hunter22");
        let nasty = format!(
            "hunter22 \",\"sensitive\":false,\"x\":\"{}",
            "y".repeat(5000)
        );
        let v = view(vec![entry("users", "", &nasty, &[])], &[]);
        let mut details = table();
        v.describe("main", "users", &mut details, &redactor);
        let note = details["note"].as_str().unwrap();
        assert!(note.starts_with("*** \","), "{note}");
        assert_eq!(note.chars().count(), NOTE_CHARS);
        assert_eq!(details["note_cut"], true);
        // Nothing the note holds became a field.
        let text = serde_json::to_string(&details).unwrap();
        let again: Value = serde_json::from_str(&text).unwrap();
        assert!(again["columns"][0].get("sensitive").is_none(), "{text}");
        assert!(again.get("x").is_none(), "{text}");
    }

    #[test]
    fn the_notes_of_one_call_have_a_budget() {
        let long = "n".repeat(NOTE_CHARS);
        let entries: Vec<_> = (0..100)
            .map(|i| entry(&format!("t{i}"), "", &long, &[]))
            .collect();
        let v = view(entries, &[]);
        let page: Vec<Value> = (0..100)
            .map(|i| json!({ "name": format!("t{i}") }))
            .collect();
        let out = v.list("main", Value::Array(page), &Redactor::default());
        let kept: usize = out
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|o| o["note"].as_str())
            .map(|n| n.chars().count())
            .sum();
        assert_eq!(kept, NOTES_BUDGET);
        let last = &out[99];
        assert!(
            last.get("note").is_none() && last["note_cut"] == true,
            "{last}"
        );
    }

    #[test]
    fn an_unreadable_catalog_is_said_not_failed() {
        let v = CatalogView {
            unavailable: true,
            ..CatalogView::default()
        };
        let mut details = table();
        v.describe("main", "users", &mut details, &Redactor::default());
        assert!(details["catalog_unavailable"].is_string());
        let list = v.list("main", json!([{ "name": "a" }]), &Redactor::default());
        assert_eq!(list["objects"][0]["name"], "a");
        assert!(list["catalog_unavailable"].is_string());
        let paged = v.list(
            "main",
            json!({ "objects": [], "total": 0 }),
            &Redactor::default(),
        );
        assert!(paged["catalog_unavailable"].is_string() && paged["total"] == 0);
    }

    #[test]
    fn a_hash_word_is_a_tag_only_when_the_catalog_knows_it() {
        let known = |t: &str| t == "pii";
        let parse = |s| SearchQuery::parse(s, &known);
        let q = parse("#PII orders");
        assert_eq!(
            (q.tags, q.text.as_str()),
            (vec!["pii".to_owned()], "orders")
        );
        let q = parse("#temp");
        assert_eq!((q.tags.len(), q.text.as_str()), (0, "#temp"));
        for plain in ["#", "##", "a#pii", "pii"] {
            assert_eq!(parse(plain).tags.len(), 0, "{plain}");
        }
        assert_eq!(parse("##pii").tags, ["pii"]);
        assert_eq!(parse("#pii #pii").tags, ["pii"]);
    }

    fn object(name: &str, comment: Option<&str>) -> ObjectMeta {
        ObjectMeta::new(name, ObjectKind::Table, comment.map(str::to_owned))
    }

    #[test]
    fn matches_say_what_matched() {
        let v = view(
            vec![
                entry("inv", "", "the INVOICES live here", &["billing"]),
                entry("inv", "total", "gross amount", &[]),
            ],
            &[],
        );
        let r = Redactor::default();
        let kinds = |q: &str, o: &ObjectMeta| -> Vec<(String, Option<String>)> {
            v.matches("main", o, &v.query(q), &r)
                .iter()
                .map(|m| {
                    (
                        m["matched"].as_str().unwrap().to_owned(),
                        m["column"].as_str().map(str::to_owned),
                    )
                })
                .collect()
        };
        let inv = object("inv", Some("Billing documents"));
        assert_eq!(kinds("invoices", &inv), [("note".to_owned(), None)],);
        assert_eq!(
            kinds("billing", &inv),
            [("comment".to_owned(), None)],
            "a plain word does not match tags"
        );
        assert_eq!(
            kinds("gross", &inv),
            [("note".to_owned(), Some("total".to_owned()))]
        );
        assert_eq!(kinds("#billing", &inv), [("tag".to_owned(), None)]);
        assert_eq!(kinds("#billing live", &inv), [("tag".to_owned(), None)]);
        assert_eq!(kinds("#billing nothing", &inv).len(), 0);
        assert_eq!(kinds("#billing", &object("other", None)).len(), 0);
        // A `#word` that is no tag is plain text, found in a comment.
        let temp = object("t", Some("#temp tables"));
        assert_eq!(kinds("#temp", &temp), [("comment".to_owned(), None)]);
    }

    #[test]
    fn a_secret_in_a_note_cannot_be_probed_by_search() {
        let mut r = Redactor::default();
        r.add("hunter22");
        let v = view(vec![entry("t", "", "password is hunter22", &[])], &[]);
        assert_eq!(
            v.matches("main", &object("t", None), &v.query("hunter22"), &r)
                .len(),
            0
        );
        assert_eq!(
            v.matches("main", &object("t", None), &v.query("password"), &r)
                .len(),
            1
        );
    }

    #[test]
    fn entries_of_another_database_are_not_ours() {
        let mut other = entry("users", "", "elsewhere", &[]);
        other.key.database = "other_db".to_owned();
        assert!(own_database("", "shop"));
        assert!(own_database("SHOP", "shop"));
        assert!(!own_database(&other.key.database, "shop"));
    }
}
