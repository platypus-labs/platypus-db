//! The schema linter: what in the tables of a schema is likely a mistake, each finding with
//! the statement that fixes it where there is one.

use platypus_core::{ConstraintKind, IndexMeta, TableDetails};
use platypus_sql::Dialect;
use serde::Serialize;

use crate::IndexDesign;
use crate::ddl::{create_index, drop_index};
use crate::model::SchemaTable;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LintKind {
    NoPrimaryKey,
    /// No index starts with the columns of a foreign key: deleting a referenced row scans the
    /// whole table.
    UnindexedForeignKey,
    /// Two indexes on the same columns in the same order.
    DuplicateIndex,
    /// An index whose columns start another index: that one serves its queries too.
    RedundantIndex,
    /// A foreign key column whose type differs from the column it references.
    ForeignKeyTypeMismatch,
    /// The table could not be read: nothing else is checked in it.
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub kind: LintKind,
    pub table: String,
    /// The index or foreign key concerned; `None` for the table itself.
    pub object: Option<String>,
    pub message: String,
    /// The statement that fixes it, to review before running.
    pub fix: Option<String>,
}

/// Every finding in the tables of `schema`, table by table.
#[must_use]
pub fn lint(dialect: Dialect, schema: &str, tables: &[SchemaTable]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for table in tables {
        let d = &table.details;
        let finding = |kind, object: Option<&str>, message: String, fix: Option<String>| Finding {
            kind,
            table: table.name.clone(),
            object: object.map(str::to_owned),
            message,
            fix,
        };
        if let Some(error) = &table.error {
            findings.push(finding(LintKind::Unreadable, None, error.clone(), None));
            continue;
        }
        if !d.columns.iter().any(|c| c.primary_key) {
            findings.push(finding(
                LintKind::NoPrimaryKey,
                None,
                "The table has no primary key.".to_owned(),
                None,
            ));
        }
        for fk in &d.foreign_keys {
            if !d
                .indexes
                .iter()
                .any(|i| starts_with_set(&i.columns, &fk.columns))
            {
                let index = IndexDesign {
                    original: None,
                    name: free_name(d, &format!("{}_{}_idx", table.name, fk.columns.join("_"))),
                    columns: fk.columns.clone(),
                    unique: false,
                };
                findings.push(finding(
                    LintKind::UnindexedForeignKey,
                    Some(&fk.name),
                    format!(
                        "No index on the foreign key columns ({}).",
                        fk.columns.join(", ")
                    ),
                    Some(create_index(dialect, schema, &table.name, &index)),
                ));
            }
            let target = (fk.ref_schema.is_empty() || fk.ref_schema == schema)
                .then(|| tables.iter().find(|t| t.name == fk.ref_table))
                .flatten();
            for (column, referenced) in fk.columns.iter().zip(&fk.ref_columns) {
                let (Some(from), Some(to)) = (
                    type_of(d, column),
                    target.and_then(|t| type_of(&t.details, referenced)),
                ) else {
                    continue;
                };
                if !same_type(from, to) {
                    findings.push(finding(
                        LintKind::ForeignKeyTypeMismatch,
                        Some(&fk.name),
                        format!(
                            "{column} is {from} but {}.{referenced} is {to}.",
                            fk.ref_table
                        ),
                        None,
                    ));
                }
            }
        }
        for (kind, index, other) in overlapping_indexes(&d.indexes) {
            let message = match kind {
                LintKind::DuplicateIndex => format!("Same columns as index {}.", other.name),
                _ => format!("Index {} starts with the same columns.", other.name),
            };
            // The catalog gives no WHERE of a partial index nor INCLUDE columns.
            let message =
                format!("{message} Check first that neither is partial or has included columns.");
            let fix =
                droppable(d, index).then(|| drop_index(dialect, schema, &table.name, &index.name));
            findings.push(finding(kind, Some(&index.name), message, fix));
        }
    }
    findings
}

/// `name`, or `name` with a number after it, so that no index of the table has it.
fn free_name(details: &TableDetails, name: &str) -> String {
    let taken = |n: &str| details.indexes.iter().any(|i| i.name == n);
    (1..)
        .map(|i| {
            if i == 1 {
                name.to_owned()
            } else {
                format!("{name}{i}")
            }
        })
        .find(|n| !taken(n))
        .unwrap_or_default()
}

fn type_of<'a>(details: &'a TableDetails, column: &str) -> Option<&'a str> {
    details
        .columns
        .iter()
        .find(|c| c.name == column)
        .map(|c| c.data_type.as_str())
}

/// Types that differ only in case or spaces are the same.
fn same_type(a: &str, b: &str) -> bool {
    let norm = |t: &str| {
        t.split_whitespace()
            .collect::<String>()
            .to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

/// Whether the first columns of `index` are `columns`, in any order.
fn starts_with_set(index: &[String], columns: &[String]) -> bool {
    index.len() >= columns.len() && columns.iter().all(|c| index[..columns.len()].contains(c))
}

/// Indexes that another one makes useless, with that other one: a duplicate keeps the first
/// of the two (or the one behind a key), a redundant one is non-unique and starts the other.
fn overlapping_indexes(indexes: &[IndexMeta]) -> Vec<(LintKind, &IndexMeta, &IndexMeta)> {
    let mut found = Vec::new();
    for (i, index) in indexes.iter().enumerate() {
        let duplicate = indexes.iter().enumerate().find(|&(j, other)| {
            j != i
                && other.columns == index.columns
                && (other.unique, other.primary) >= (index.unique, index.primary)
                && ((other.unique, other.primary) != (index.unique, index.primary) || j < i)
        });
        if let Some((_, other)) = duplicate {
            found.push((LintKind::DuplicateIndex, index, other));
            continue;
        }
        let wider = indexes.iter().find(|other| {
            !index.unique
                && other.columns.len() > index.columns.len()
                && other.columns.starts_with(&index.columns)
        });
        if let Some(other) = wider {
            found.push((LintKind::RedundantIndex, index, other));
        }
    }
    found
}

/// Whether `index` can be dropped by itself: it is not the primary key or the index of a
/// constraint.
fn droppable(details: &TableDetails, index: &IndexMeta) -> bool {
    !index.primary
        && !details.constraints.iter().any(|c| {
            c.name == index.name
                && matches!(c.kind, ConstraintKind::PrimaryKey | ConstraintKind::Unique)
        })
}

#[cfg(test)]
mod tests {
    use platypus_core::{ColumnMeta, ForeignKeyMeta};

    use super::*;

    fn column(name: &str, data_type: &str, primary_key: bool) -> ColumnMeta {
        ColumnMeta {
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            nullable: !primary_key,
            default: None,
            primary_key,
            comment: None,
            enum_values: Vec::new(),
            fields: Vec::new(),
        }
    }

    fn index(name: &str, columns: &[&str], unique: bool, primary: bool) -> IndexMeta {
        IndexMeta {
            name: name.to_owned(),
            columns: columns.iter().map(|&c| c.to_owned()).collect(),
            unique,
            primary,
            size: None,
        }
    }

    fn table(name: &str, details: TableDetails) -> SchemaTable {
        SchemaTable {
            name: name.to_owned(),
            comment: None,
            details,
            error: None,
        }
    }

    fn shop() -> Vec<SchemaTable> {
        let customers = TableDetails {
            columns: vec![column("id", "bigint", true)],
            indexes: vec![index("customers_pkey", &["id"], true, true)],
            ..TableDetails::default()
        };
        let orders = TableDetails {
            columns: vec![
                column("id", "integer", true),
                column("customer_id", "integer", false),
                column("placed", "date", false),
            ],
            indexes: vec![
                index("orders_pkey", &["id"], true, true),
                index("orders_placed", &["placed"], false, false),
                index("orders_placed_again", &["placed"], false, false),
                index("orders_placed_id", &["placed", "id"], false, false),
            ],
            foreign_keys: vec![ForeignKeyMeta {
                name: "orders_customer".to_owned(),
                columns: vec!["customer_id".to_owned()],
                ref_schema: "public".to_owned(),
                ref_table: "customers".to_owned(),
                ref_columns: vec!["id".to_owned()],
            }],
            ..TableDetails::default()
        };
        let log = TableDetails {
            columns: vec![column("line", "TEXT", false)],
            ..TableDetails::default()
        };
        vec![
            table("customers", customers),
            table("orders", orders),
            table("log", log),
        ]
    }

    fn kinds(findings: &[Finding]) -> Vec<(LintKind, &str, Option<&str>)> {
        findings
            .iter()
            .map(|f| (f.kind, f.table.as_str(), f.object.as_deref()))
            .collect()
    }

    #[test]
    fn finds_every_kind_of_problem() {
        let findings = lint(Dialect::Postgres, "public", &shop());
        assert_eq!(
            kinds(&findings),
            [
                (
                    LintKind::UnindexedForeignKey,
                    "orders",
                    Some("orders_customer")
                ),
                (
                    LintKind::ForeignKeyTypeMismatch,
                    "orders",
                    Some("orders_customer")
                ),
                (LintKind::RedundantIndex, "orders", Some("orders_placed")),
                (
                    LintKind::DuplicateIndex,
                    "orders",
                    Some("orders_placed_again")
                ),
                (LintKind::NoPrimaryKey, "log", None),
            ]
        );
        assert_eq!(
            findings[0].fix.as_deref(),
            Some("CREATE INDEX orders_customer_id_idx ON public.orders (customer_id)")
        );
        assert_eq!(
            findings[1].message,
            "customer_id is integer but customers.id is bigint."
        );
        assert_eq!(
            findings[3].fix.as_deref(),
            Some("DROP INDEX public.orders_placed_again")
        );
    }

    #[test]
    fn unreadable_tables_are_reported_and_not_checked() {
        let mut tables = shop();
        tables[2].error = Some("permission denied".to_owned());
        let findings = lint(Dialect::Postgres, "public", &tables[2..]);
        assert_eq!(kinds(&findings), [(LintKind::Unreadable, "log", None)]);
        assert_eq!(findings[0].message, "permission denied");
    }

    #[test]
    fn suggested_indexes_get_a_free_name() {
        let details = TableDetails {
            indexes: vec![
                index("t_a_idx", &["x"], false, false),
                index("t_a_idx2", &["y"], false, false),
            ],
            ..TableDetails::default()
        };
        assert_eq!(free_name(&details, "t_a_idx"), "t_a_idx3");
        assert_eq!(free_name(&details, "t_b_idx"), "t_b_idx");
    }

    #[test]
    fn a_clean_schema_has_no_findings() {
        let mut tables = shop();
        tables.truncate(1);
        assert_eq!(lint(Dialect::Postgres, "public", &tables), []);
    }

    #[test]
    fn a_unique_index_is_kept_over_a_plain_duplicate() {
        let indexes = [
            index("plain", &["a"], false, false),
            index("uniq", &["a"], true, false),
        ];
        let found = overlapping_indexes(&indexes);
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].1.name.as_str(), found[0].2.name.as_str()),
            ("plain", "uniq")
        );
    }

    #[test]
    fn foreign_keys_are_covered_by_any_index_starting_with_their_columns() {
        let cols = ["b".to_owned(), "a".to_owned()];
        assert!(starts_with_set(
            &["a".to_owned(), "b".to_owned(), "c".to_owned()],
            &cols
        ));
        assert!(!starts_with_set(
            &["a".to_owned(), "c".to_owned(), "b".to_owned()],
            &cols
        ));
        assert!(same_type("double precision", "DOUBLE  PRECISION"));
    }
}
