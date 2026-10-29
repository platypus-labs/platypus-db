//! What changed between two states of a schema, in words a person reads: the summary the
//! schema history shows beside the script that [`crate::compare`] writes.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::{ColumnDesign, ConstraintDesign, ForeignKeyDesign, IndexDesign, TableDesign};

/// What kind of change a [`SchemaChange`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    TableAdded,
    TableDropped,
    ColumnAdded,
    ColumnDropped,
    ColumnTypeChanged,
    ColumnNullabilityChanged,
    ColumnDefaultChanged,
    /// Anything else about a column: primary key, comment, generated, collation, extras.
    ColumnChanged,
    IndexAdded,
    IndexDropped,
    ForeignKeyAdded,
    ForeignKeyDropped,
    ConstraintAdded,
    ConstraintDropped,
    CommentChanged,
}

/// One change to a table. `item` is the column, index, key or constraint it is about; `from`
/// and `to` hold the old and new value of what changed (a type, `NULL`/`NOT NULL`, a default,
/// a comment, or for an added column its type), where there is one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaChange {
    pub table: String,
    pub kind: ChangeKind,
    pub item: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

impl SchemaChange {
    fn table(table: &str, kind: ChangeKind) -> Self {
        Self {
            table: table.to_owned(),
            kind,
            item: None,
            from: None,
            to: None,
        }
    }

    fn item(table: &str, kind: ChangeKind, item: impl Into<String>) -> Self {
        Self {
            item: Some(item.into()),
            ..Self::table(table, kind)
        }
    }

    fn with(mut self, from: Option<String>, to: Option<String>) -> Self {
        self.from = from;
        self.to = to;
        self
    }
}

/// The changes that turn the tables `old` into `new`, grouped by table (in name order), and
/// within a table in the order columns, indexes, foreign keys, constraints, comment. Tables
/// are matched by name, their parts too: a renamed column is a dropped one and an added one.
/// A part with the same name but another definition is dropped and added again.
#[must_use]
pub fn summary(old: &[TableDesign], new: &[TableDesign]) -> Vec<SchemaChange> {
    let names: BTreeSet<&str> = old.iter().chain(new).map(|t| t.name.as_str()).collect();
    let mut out = Vec::new();
    for name in names {
        match (find(old, name), find(new, name)) {
            (None, Some(_)) => out.push(SchemaChange::table(name, ChangeKind::TableAdded)),
            (Some(_), None) => out.push(SchemaChange::table(name, ChangeKind::TableDropped)),
            (Some(o), Some(n)) => table_changes(o, n, &mut out),
            (None, None) => {}
        }
    }
    out
}

fn find<'a>(tables: &'a [TableDesign], name: &str) -> Option<&'a TableDesign> {
    tables.iter().find(|t| t.name == name)
}

fn table_changes(old: &TableDesign, new: &TableDesign, out: &mut Vec<SchemaChange>) {
    let t = new.name.as_str();
    columns(t, &old.columns, &new.columns, out);
    parts(
        t,
        (ChangeKind::IndexAdded, ChangeKind::IndexDropped),
        &old.indexes,
        &new.indexes,
        index_key,
        out,
    );
    parts(
        t,
        (ChangeKind::ForeignKeyAdded, ChangeKind::ForeignKeyDropped),
        &old.foreign_keys,
        &new.foreign_keys,
        foreign_key_key,
        out,
    );
    parts(
        t,
        (ChangeKind::ConstraintAdded, ChangeKind::ConstraintDropped),
        &old.constraints,
        &new.constraints,
        constraint_key,
        out,
    );
    if old.comment != new.comment {
        out.push(
            SchemaChange::table(t, ChangeKind::CommentChanged)
                .with(old.comment.clone(), new.comment.clone()),
        );
    }
}

fn columns(table: &str, old: &[ColumnDesign], new: &[ColumnDesign], out: &mut Vec<SchemaChange>) {
    for o in old.iter().filter(|o| !new.iter().any(|n| n.name == o.name)) {
        out.push(SchemaChange::item(
            table,
            ChangeKind::ColumnDropped,
            &o.name,
        ));
    }
    for n in new {
        match old.iter().find(|o| o.name == n.name) {
            None => out.push(
                SchemaChange::item(table, ChangeKind::ColumnAdded, &n.name)
                    .with(None, Some(n.data_type.clone())),
            ),
            Some(o) => column_changes(table, o, n, out),
        }
    }
}

fn nullability(nullable: bool) -> String {
    (if nullable { "NULL" } else { "NOT NULL" }).to_owned()
}

fn column_changes(table: &str, o: &ColumnDesign, n: &ColumnDesign, out: &mut Vec<SchemaChange>) {
    let at = |kind| SchemaChange::item(table, kind, &n.name);
    if o.data_type != n.data_type {
        out.push(
            at(ChangeKind::ColumnTypeChanged)
                .with(Some(o.data_type.clone()), Some(n.data_type.clone())),
        );
    }
    if o.nullable != n.nullable {
        out.push(
            at(ChangeKind::ColumnNullabilityChanged)
                .with(Some(nullability(o.nullable)), Some(nullability(n.nullable))),
        );
    }
    if o.default != n.default {
        out.push(at(ChangeKind::ColumnDefaultChanged).with(o.default.clone(), n.default.clone()));
    }
    let other: Vec<&str> = [
        (o.primary_key != n.primary_key, "primary key"),
        (o.comment != n.comment, "comment"),
        (o.extra != n.extra, "extra"),
        (o.generated != n.generated, "generated"),
        (o.collation != n.collation, "collation"),
    ]
    .into_iter()
    .filter_map(|(changed, what)| changed.then_some(what))
    .collect();
    if !other.is_empty() {
        out.push(at(ChangeKind::ColumnChanged).with(None, Some(other.join(", "))));
    }
}

/// The name of an index and what it is, to tell an index of the same name that changed.
fn index_key(i: &IndexDesign) -> (String, String) {
    let unique = if i.unique { "unique " } else { "" };
    (i.name.clone(), format!("{unique}{}", i.columns.join(",")))
}

fn foreign_key_key(f: &ForeignKeyDesign) -> (String, String) {
    let definition = format!(
        "{} -> {}.{}({}) {:?} {:?}",
        f.columns.join(","),
        f.ref_schema,
        f.ref_table,
        f.ref_columns.join(","),
        f.on_delete,
        f.on_update
    );
    (f.name.clone(), definition)
}

fn constraint_key(c: &ConstraintDesign) -> (String, String) {
    let definition = format!("{:?} {} {}", c.kind, c.columns.join(","), c.check);
    (c.name.clone(), definition)
}

/// Adds the parts of `new` that `old` lacks and drops those `new` lacks. `key` gives a part's
/// name and its definition: parts match when both are equal; an unnamed part is shown by its
/// definition.
fn parts<T>(
    table: &str,
    (added, dropped): (ChangeKind, ChangeKind),
    old: &[T],
    new: &[T],
    key: fn(&T) -> (String, String),
    out: &mut Vec<SchemaChange>,
) {
    let old: Vec<_> = old.iter().map(key).collect();
    let new: Vec<_> = new.iter().map(key).collect();
    let shown = |(name, definition): &(String, String)| {
        if name.is_empty() {
            definition.clone()
        } else {
            name.clone()
        }
    };
    for k in old.iter().filter(|k| !new.contains(k)) {
        out.push(SchemaChange::item(table, dropped, shown(k)));
    }
    for k in new.iter().filter(|k| !old.contains(k)) {
        out.push(SchemaChange::item(table, added, shown(k)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConstraintType;

    fn column(name: &str, data_type: &str) -> ColumnDesign {
        ColumnDesign {
            original: Some(name.to_owned()),
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            nullable: true,
            ..ColumnDesign::default()
        }
    }

    fn table(name: &str, columns: Vec<ColumnDesign>) -> TableDesign {
        TableDesign {
            name: name.to_owned(),
            columns,
            ..TableDesign::default()
        }
    }

    fn kinds(changes: &[SchemaChange]) -> Vec<(&str, ChangeKind, Option<&str>)> {
        changes
            .iter()
            .map(|c| (c.table.as_str(), c.kind, c.item.as_deref()))
            .collect()
    }

    #[test]
    fn identical_schemas_have_no_changes() {
        let t = vec![table("a", vec![column("id", "integer")])];
        assert_eq!(summary(&t, &t), Vec::new());
    }

    #[test]
    fn tables_added_and_dropped() {
        let old = vec![table("gone", vec![]), table("kept", vec![])];
        let new = vec![table("kept", vec![]), table("fresh", vec![])];
        assert_eq!(
            kinds(&summary(&old, &new)),
            [
                ("fresh", ChangeKind::TableAdded, None),
                ("gone", ChangeKind::TableDropped, None)
            ]
        );
    }

    #[test]
    fn column_changes_are_told_apart() {
        let old = vec![table(
            "t",
            vec![
                column("id", "integer"),
                column("name", "text"),
                column("old", "text"),
                column("n", "integer"),
            ],
        )];
        let mut n = column("n", "integer");
        n.nullable = false;
        n.default = Some("0".to_owned());
        n.comment = Some("count".to_owned());
        let new = vec![table(
            "t",
            vec![
                column("id", "bigint"),
                column("name", "text"),
                column("added", "date"),
                n,
            ],
        )];
        let changes = summary(&old, &new);
        assert_eq!(
            kinds(&changes),
            [
                ("t", ChangeKind::ColumnDropped, Some("old")),
                ("t", ChangeKind::ColumnTypeChanged, Some("id")),
                ("t", ChangeKind::ColumnAdded, Some("added")),
                ("t", ChangeKind::ColumnNullabilityChanged, Some("n")),
                ("t", ChangeKind::ColumnDefaultChanged, Some("n")),
                ("t", ChangeKind::ColumnChanged, Some("n")),
            ]
        );
        assert_eq!(changes[1].from.as_deref(), Some("integer"));
        assert_eq!(changes[1].to.as_deref(), Some("bigint"));
        assert_eq!(changes[2].to.as_deref(), Some("date"));
        assert_eq!(changes[3].to.as_deref(), Some("NOT NULL"));
        assert_eq!(changes[4].from, None);
        assert_eq!(changes[4].to.as_deref(), Some("0"));
        assert_eq!(changes[5].to.as_deref(), Some("comment"));
    }

    #[test]
    fn indexes_keys_and_constraints() {
        let mut old = table("t", vec![]);
        old.indexes = vec![
            IndexDesign {
                name: "by_a".into(),
                columns: vec!["a".into()],
                ..IndexDesign::default()
            },
            IndexDesign {
                name: "by_b".into(),
                columns: vec!["b".into()],
                ..IndexDesign::default()
            },
        ];
        old.foreign_keys = vec![ForeignKeyDesign {
            name: "fk_a".into(),
            columns: vec!["a".into()],
            ref_table: "p".into(),
            ..ForeignKeyDesign::default()
        }];
        let mut new = table("t", vec![]);
        // `by_b` keeps its name but covers another column, so it is dropped and added.
        new.indexes = vec![
            old.indexes[0].clone(),
            IndexDesign {
                name: "by_b".into(),
                columns: vec!["b".into(), "c".into()],
                ..IndexDesign::default()
            },
        ];
        new.constraints = vec![ConstraintDesign {
            name: "pos".into(),
            kind: ConstraintType::Check,
            check: "a > 0".into(),
            ..ConstraintDesign::default()
        }];
        assert_eq!(
            kinds(&summary(&[old], &[new])),
            [
                ("t", ChangeKind::IndexDropped, Some("by_b")),
                ("t", ChangeKind::IndexAdded, Some("by_b")),
                ("t", ChangeKind::ForeignKeyDropped, Some("fk_a")),
                ("t", ChangeKind::ConstraintAdded, Some("pos")),
            ]
        );
    }

    #[test]
    fn unnamed_parts_are_shown_by_their_definition() {
        let mut old = table("t", vec![]);
        old.foreign_keys = vec![ForeignKeyDesign {
            columns: vec!["a".into()],
            ref_table: "p".into(),
            ..ForeignKeyDesign::default()
        }];
        let changes = summary(&[old], &[table("t", vec![])]);
        assert_eq!(changes.len(), 1);
        assert!(changes[0].item.as_deref().unwrap().contains("a -> "));
    }

    #[test]
    fn comment_changes() {
        let mut new = table("t", vec![]);
        new.comment = Some("orders".into());
        let changes = summary(&[table("t", vec![])], &[new]);
        assert_eq!(kinds(&changes), [("t", ChangeKind::CommentChanged, None)]);
        assert_eq!(changes[0].from, None);
        assert_eq!(changes[0].to.as_deref(), Some("orders"));
    }
}
