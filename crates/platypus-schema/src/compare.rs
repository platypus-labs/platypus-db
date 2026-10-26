//! The script that makes one schema like another: tables created, altered with [`script`] and
//! dropped, in an order the foreign keys allow.

use platypus_sql::Dialect;

use crate::{ConstraintDesign, Script, TableDesign, script};

/// The script that turns the tables `current` of `schema` into `wanted`, read from
/// `wanted_schema` (possibly of another database). Tables are matched by name, and their
/// parts too: a renamed column is a dropped column and a new one.
pub fn compare(
    dialect: Dialect,
    schema: &str,
    wanted_schema: &str,
    wanted: &[TableDesign],
    current: &[TableDesign],
) -> Result<Script, String> {
    fn find<'a>(tables: &'a [TableDesign], name: &str) -> Option<&'a TableDesign> {
        tables.iter().find(|t| t.name == name)
    }
    let mut out = Script {
        transactional: true,
        ..Script::default()
    };
    let new: Vec<TableDesign> = wanted
        .iter()
        .filter(|w| find(current, &w.name).is_none())
        .map(|w| matched(w, None, schema, wanted_schema))
        .collect();
    for table in by_references(&new, false) {
        add(&mut out, script(dialect, schema, None, table)?);
    }
    for old in current {
        if let Some(w) = find(wanted, &old.name) {
            add(
                &mut out,
                script(
                    dialect,
                    schema,
                    Some(old),
                    &matched(w, Some(old), schema, wanted_schema),
                )?,
            );
        }
    }
    let gone: Vec<TableDesign> = current
        .iter()
        .filter(|c| find(wanted, &c.name).is_none())
        .cloned()
        .collect();
    for table in by_references(&gone, true) {
        out.statements.push(format!(
            "DROP TABLE {}",
            dialect.qualified(Some(schema), &table.name)
        ));
        out.warnings.push(format!(
            "Drops the table {}: its rows are lost.",
            table.name
        ));
    }
    out.transactional &= !matches!(dialect, Dialect::MySql | Dialect::ClickHouse);
    Ok(out)
}

/// `wanted` as a design of the table `old` of `schema`: its parts named like those of `old`
/// keep them, and its foreign keys within `wanted_schema` point into `schema`.
fn matched(
    wanted: &TableDesign,
    old: Option<&TableDesign>,
    schema: &str,
    wanted_schema: &str,
) -> TableDesign {
    /// The name in the database of the part of `old` that `is` accepts: the same part.
    fn kept<T>(
        old: Option<&[T]>,
        is: impl Fn(&T) -> bool,
        original: fn(&T) -> Option<&String>,
    ) -> Option<String> {
        old?.iter().find(|p| is(p)).and_then(original).cloned()
    }
    let mut t = wanted.clone();
    for c in &mut t.columns {
        c.original = kept(
            old.map(|o| &o.columns[..]),
            |o| o.name == c.name,
            |o| o.original.as_ref(),
        );
    }
    for i in &mut t.indexes {
        i.original = kept(
            old.map(|o| &o.indexes[..]),
            |o| o.name == i.name,
            |o| o.original.as_ref(),
        );
    }
    for f in &mut t.foreign_keys {
        f.original = kept(
            old.map(|o| &o.foreign_keys[..]),
            |o| o.name == f.name,
            |o| o.original.as_ref(),
        );
        if f.ref_schema == wanted_schema {
            schema.clone_into(&mut f.ref_schema);
        }
    }
    // SQLite's constraints have no name: those are the same when their columns are.
    for c in &mut t.constraints {
        let is = |o: &ConstraintDesign| {
            o.name == c.name && (!c.name.is_empty() || (o.kind, &o.columns) == (c.kind, &c.columns))
        };
        c.original = kept(old.map(|o| &o.constraints[..]), is, |o| o.original.as_ref());
    }
    t.primary_key_name = old.and_then(|o| o.primary_key_name.clone());
    t
}

/// `tables` with each one after the tables it references (`reverse`: before them), so they
/// can be created (dropped) in that order. Tables in a cycle keep their order, at the end.
fn by_references(tables: &[TableDesign], reverse: bool) -> Vec<&TableDesign> {
    let mut left: Vec<&TableDesign> = tables.iter().collect();
    let mut sorted = Vec::with_capacity(left.len());
    loop {
        let ready = left.iter().position(|t| {
            left.iter().all(|other| {
                let (from, to) = if reverse { (*other, *t) } else { (*t, *other) };
                from.name == to.name || !from.foreign_keys.iter().any(|f| f.ref_table == to.name)
            })
        });
        match ready {
            Some(i) => sorted.push(left.remove(i)),
            None => break,
        }
    }
    sorted.extend(left);
    sorted
}

/// Appends the statements of `part` to `out`, and the parts that each table's script repeats
/// (before, after, warnings) only once.
fn add(out: &mut Script, part: Script) {
    fn once(to: &mut Vec<String>, items: Vec<String>) {
        for s in items {
            if !to.contains(&s) {
                to.push(s);
            }
        }
    }
    once(&mut out.before, part.before);
    once(&mut out.after, part.after);
    out.statements.extend(part.statements);
    once(&mut out.warnings, part.warnings);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColumnDesign, ForeignKeyDesign};

    fn column(name: &str, data_type: &str) -> ColumnDesign {
        ColumnDesign {
            original: Some(name.to_owned()),
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            nullable: true,
            ..ColumnDesign::default()
        }
    }

    fn table(name: &str, columns: Vec<ColumnDesign>, refs: &[&str], schema: &str) -> TableDesign {
        TableDesign {
            name: name.to_owned(),
            columns,
            foreign_keys: refs
                .iter()
                .map(|r| ForeignKeyDesign {
                    original: Some(format!("{name}_{r}")),
                    name: format!("{name}_{r}"),
                    columns: vec![format!("{r}_id")],
                    ref_schema: schema.to_owned(),
                    ref_table: (*r).to_owned(),
                    ref_columns: vec!["id".to_owned()],
                    ..ForeignKeyDesign::default()
                })
                .collect(),
            ..TableDesign::default()
        }
    }

    #[test]
    fn same_schemas_need_nothing() {
        let t = [table("a", vec![column("id", "integer")], &[], "public")];
        let s = compare(Dialect::Postgres, "public", "public", &t, &t).unwrap();
        assert!(s.statements.is_empty(), "{:?}", s.statements);
    }

    #[test]
    fn unnamed_constraints_are_matched_by_their_columns() {
        use crate::ConstraintDesign;
        let mut t = table(
            "a",
            vec![column("id", "integer"), column("code", "text")],
            &[],
            "main",
        );
        t.constraints = vec![ConstraintDesign {
            original: Some("sqlite_autoindex_a_1".to_owned()),
            columns: vec!["code".to_owned()],
            ..ConstraintDesign::default()
        }];
        let tables = [t];
        let s = compare(Dialect::Sqlite, "main", "main", &tables, &tables).unwrap();
        assert_eq!(s.statements, Vec::<String>::new());
    }

    #[test]
    fn creates_alters_and_drops_in_reference_order() {
        let wanted = [
            table(
                "orders",
                vec![column("id", "integer"), column("customers_id", "integer")],
                &["customers"],
                "dev",
            ),
            table("customers", vec![column("id", "integer")], &[], "dev"),
            table(
                "items",
                vec![column("id", "bigint"), column("note", "text")],
                &[],
                "dev",
            ),
        ];
        let current = [
            table("items", vec![column("id", "integer")], &[], "prod"),
            table(
                "old_lines",
                vec![column("old_id", "integer")],
                &["old"],
                "prod",
            ),
            table("old", vec![column("id", "integer")], &[], "prod"),
        ];
        let s = compare(Dialect::Postgres, "prod", "dev", &wanted, &current).unwrap();
        let first_words: Vec<String> = s
            .statements
            .iter()
            .map(|st| st.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
            .collect();
        assert_eq!(
            first_words,
            [
                "CREATE TABLE prod.customers",
                "CREATE TABLE prod.orders",
                "ALTER TABLE prod.items",
                "ALTER TABLE prod.items",
                "DROP TABLE prod.old_lines",
                "DROP TABLE prod.old",
            ]
        );
        assert!(
            s.statements[1].contains("REFERENCES prod.customers"),
            "{}",
            s.statements[1]
        );
        assert_eq!(s.warnings.len(), 2);
    }
}
