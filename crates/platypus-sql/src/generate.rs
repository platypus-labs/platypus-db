//! Statements generated for the navigator's "Generate SQL" and object actions. DML
//! templates use `?` placeholders for the values to fill in.

use platypus_core::ObjectKind;

use crate::Dialect;

/// An object as the navigator names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectName<'a> {
    /// `None` for the session's current schema.
    pub schema: Option<&'a str>,
    /// Routines carry their argument types in PostgreSQL: `add(integer, integer)`.
    pub name: &'a str,
    /// The table of a trigger.
    pub parent: Option<&'a str>,
}

impl<'a> ObjectName<'a> {
    #[must_use]
    pub const fn new(schema: Option<&'a str>, name: &'a str) -> Self {
        Self {
            schema,
            name,
            parent: None,
        }
    }
}

impl Dialect {
    /// The qualified name; a PostgreSQL routine keeps its argument list unquoted.
    #[must_use]
    pub fn object(self, o: &ObjectName<'_>) -> String {
        match (self, o.name.split_once('(')) {
            (Self::Postgres, Some((base, args))) => {
                format!("{}({args}", self.qualified(o.schema, base))
            }
            _ => self.qualified(o.schema, o.name),
        }
    }

    fn columns(self, columns: &[&str]) -> Vec<String> {
        columns
            .iter()
            .map(|c| self.quote_ident(c).into_owned())
            .collect()
    }

    /// `a = ? AND b = ?` over the key columns, or every column without a key.
    fn key_filter(self, columns: &[&str], keys: &[&str]) -> String {
        let keys = if keys.is_empty() { columns } else { keys };
        self.columns(keys)
            .iter()
            .map(|k| format!("{k} = ?"))
            .collect::<Vec<_>>()
            .join("\n  AND ")
    }

    #[must_use]
    pub fn select(self, table: &ObjectName<'_>, columns: &[&str], limit: Option<u64>) -> String {
        let list = if columns.is_empty() {
            "*".to_owned()
        } else {
            self.columns(columns).join(",\n       ")
        };
        let mut sql = format!("SELECT {list}\nFROM {}", self.object(table));
        if let Some(limit) = limit {
            sql.push('\n');
            sql.push_str(&self.limit_clause(limit));
        }
        sql.push(';');
        sql
    }

    #[must_use]
    pub fn insert(self, table: &ObjectName<'_>, columns: &[&str]) -> String {
        let placeholders = vec!["?"; columns.len()].join(", ");
        format!(
            "INSERT INTO {} ({})\nVALUES ({placeholders});",
            self.object(table),
            self.columns(columns).join(", ")
        )
    }

    /// Sets the non-key columns where the key matches. Without a key every column is in
    /// both lists, so the statement still names a single row when the values are exact.
    #[must_use]
    pub fn update(self, table: &ObjectName<'_>, columns: &[&str], keys: &[&str]) -> String {
        let set: Vec<String> = self
            .columns(columns)
            .iter()
            .zip(columns)
            .filter(|(_, c)| !keys.contains(c))
            .map(|(q, _)| format!("{q} = ?"))
            .collect();
        format!(
            "UPDATE {}\nSET {}\nWHERE {};",
            self.object(table),
            set.join(",\n    "),
            self.key_filter(columns, keys)
        )
    }

    #[must_use]
    pub fn delete(self, table: &ObjectName<'_>, columns: &[&str], keys: &[&str]) -> String {
        format!(
            "DELETE FROM {}\nWHERE {};",
            self.object(table),
            self.key_filter(columns, keys)
        )
    }

    /// Inserts a row or updates it when the key exists. `None` without a key.
    #[must_use]
    pub fn upsert(self, table: &ObjectName<'_>, columns: &[&str], keys: &[&str]) -> Option<String> {
        if keys.is_empty() {
            return None;
        }
        let insert = self.insert(table, columns);
        let insert = insert.trim_end_matches(';');
        let others: Vec<String> = self
            .columns(columns)
            .into_iter()
            .zip(columns)
            .filter(|(_, c)| !keys.contains(c))
            .map(|(q, _)| q)
            .collect();
        Some(match self {
            Self::Postgres | Self::Sqlite | Self::DuckDb => {
                let action = if others.is_empty() {
                    "DO NOTHING".to_owned()
                } else {
                    let set: Vec<_> = others
                        .iter()
                        .map(|q| format!("{q} = EXCLUDED.{q}"))
                        .collect();
                    format!("DO UPDATE SET {}", set.join(",\n    "))
                };
                format!(
                    "{insert}\nON CONFLICT ({}) {action};",
                    self.columns(keys).join(", ")
                )
            }
            // `VALUES()` is deprecated in MySQL 8.0.20 but the only form MariaDB knows.
            Self::MySql => {
                let targets = if others.is_empty() {
                    self.columns(keys)
                } else {
                    others
                };
                let set: Vec<_> = targets
                    .iter()
                    .map(|q| format!("{q} = VALUES({q})"))
                    .collect();
                format!("{insert}\nON DUPLICATE KEY UPDATE {};", set.join(",\n    "))
            }
        })
    }

    /// The SQL keyword for the kind in `DROP`/`ALTER`, or `None` where the dialect has no
    /// such object.
    const fn keyword(self, kind: ObjectKind) -> Option<&'static str> {
        Some(match (self, kind) {
            (_, ObjectKind::Table) => "TABLE",
            (_, ObjectKind::View) => "VIEW",
            (_, ObjectKind::Trigger) => "TRIGGER",
            (Self::Postgres, ObjectKind::MaterializedView) => "MATERIALIZED VIEW",
            (Self::Postgres, ObjectKind::ForeignTable) => "FOREIGN TABLE",
            (Self::Postgres | Self::DuckDb, ObjectKind::Type) => "TYPE",
            (Self::Postgres, ObjectKind::Domain) => "DOMAIN",
            (Self::Postgres, ObjectKind::Extension) => "EXTENSION",
            (Self::Postgres | Self::MySql | Self::DuckDb, ObjectKind::Sequence) => "SEQUENCE",
            // DuckDB's user functions are macros.
            (Self::DuckDb, ObjectKind::Function) => "MACRO",
            (Self::Postgres | Self::MySql, ObjectKind::Function) => "FUNCTION",
            (Self::Postgres | Self::MySql, ObjectKind::Procedure) => "PROCEDURE",
            (Self::MySql, ObjectKind::Event) => "EVENT",
            _ => return None,
        })
    }

    /// Whether `DROP ... CASCADE` also drops dependent objects (PostgreSQL, DuckDB).
    #[must_use]
    pub const fn supports_cascade(self) -> bool {
        matches!(self, Self::Postgres | Self::DuckDb)
    }

    #[must_use]
    pub fn drop(self, kind: ObjectKind, o: &ObjectName<'_>, cascade: bool) -> Option<String> {
        let keyword = self.keyword(kind)?;
        let target = match (self, kind) {
            // PostgreSQL triggers are named per table; extensions are not in a schema.
            (Self::Postgres, ObjectKind::Trigger) => format!(
                "{} ON {}",
                self.quote_ident(o.name),
                self.qualified(o.schema, o.parent?)
            ),
            (Self::Postgres, ObjectKind::Extension) => self.quote_ident(o.name).into_owned(),
            _ => self.object(o),
        };
        let cascade = if cascade && self.supports_cascade() {
            " CASCADE"
        } else {
            ""
        };
        Some(format!("DROP {keyword} {target}{cascade};"))
    }

    /// Removes every row: SQLite has no `TRUNCATE`.
    #[must_use]
    pub fn truncate(self, table: &ObjectName<'_>) -> String {
        match self {
            Self::Sqlite => format!("DELETE FROM {};", self.object(table)),
            _ => format!("TRUNCATE TABLE {};", self.object(table)),
        }
    }

    /// Renames an object, or `None` where the dialect cannot rename that kind.
    #[must_use]
    pub fn rename(self, kind: ObjectKind, o: &ObjectName<'_>, new_name: &str) -> Option<String> {
        let to = self.quote_ident(new_name);
        match self {
            Self::Postgres => match kind {
                ObjectKind::Trigger => Some(format!(
                    "ALTER TRIGGER {} ON {} RENAME TO {to};",
                    self.quote_ident(o.name),
                    self.qualified(o.schema, o.parent?)
                )),
                ObjectKind::Extension => None,
                _ => Some(format!(
                    "ALTER {} {} RENAME TO {to};",
                    self.keyword(kind)?,
                    self.object(o)
                )),
            },
            // RENAME TABLE moves views too; the new name stays in the same schema.
            Self::MySql => match kind {
                ObjectKind::Table | ObjectKind::View | ObjectKind::Sequence => Some(format!(
                    "RENAME TABLE {} TO {};",
                    self.object(o),
                    self.qualified(o.schema, new_name)
                )),
                ObjectKind::Event => Some(format!(
                    "ALTER EVENT {} RENAME TO {};",
                    self.object(o),
                    self.qualified(o.schema, new_name)
                )),
                _ => None,
            },
            Self::Sqlite => (kind == ObjectKind::Table)
                .then(|| format!("ALTER TABLE {} RENAME TO {to};", self.object(o))),
            Self::DuckDb => match kind {
                ObjectKind::Table | ObjectKind::View => Some(format!(
                    "ALTER {} {} RENAME TO {to};",
                    self.keyword(kind)?,
                    self.object(o)
                )),
                _ => None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDERS: ObjectName<'static> = ObjectName::new(Some("demo"), "orders");

    #[test]
    fn generates_dml() {
        let pg = Dialect::Postgres;
        assert_eq!(
            pg.select(&ORDERS, &["id", "Total"], Some(200)),
            "SELECT id,\n       \"Total\"\nFROM demo.orders\nLIMIT 200;"
        );
        assert_eq!(pg.select(&ORDERS, &[], None), "SELECT *\nFROM demo.orders;");
        assert_eq!(
            pg.insert(&ORDERS, &["id", "total"]),
            "INSERT INTO demo.orders (id, total)\nVALUES (?, ?);"
        );
        assert_eq!(
            pg.update(&ORDERS, &["id", "total", "note"], &["id"]),
            "UPDATE demo.orders\nSET total = ?,\n    note = ?\nWHERE id = ?;"
        );
        assert_eq!(
            Dialect::MySql.delete(&ObjectName::new(None, "t"), &["a", "b"], &[]),
            "DELETE FROM t\nWHERE a = ?\n  AND b = ?;"
        );
    }

    #[test]
    fn generates_upserts_per_dialect() {
        let cols = ["id", "total"];
        assert_eq!(
            Dialect::Postgres.upsert(&ORDERS, &cols, &["id"]).unwrap(),
            "INSERT INTO demo.orders (id, total)\nVALUES (?, ?)\nON CONFLICT (id) DO UPDATE SET total = EXCLUDED.total;"
        );
        assert_eq!(
            Dialect::MySql.upsert(&ORDERS, &cols, &["id"]).unwrap(),
            "INSERT INTO demo.orders (id, total)\nVALUES (?, ?)\nON DUPLICATE KEY UPDATE total = VALUES(total);"
        );
        assert_eq!(
            Dialect::Sqlite.upsert(&ORDERS, &["id"], &["id"]).unwrap(),
            "INSERT INTO demo.orders (id)\nVALUES (?)\nON CONFLICT (id) DO NOTHING;"
        );
        assert_eq!(Dialect::Postgres.upsert(&ORDERS, &cols, &[]), None);
    }

    #[test]
    fn drops_renames_and_truncates() {
        let pg = Dialect::Postgres;
        let f = ObjectName::new(Some("demo"), "Add(integer, integer)");
        assert_eq!(
            pg.drop(ObjectKind::Function, &f, true).unwrap(),
            "DROP FUNCTION demo.\"Add\"(integer, integer) CASCADE;"
        );
        let trg = ObjectName {
            parent: Some("orders"),
            ..ObjectName::new(Some("demo"), "audit")
        };
        assert_eq!(
            pg.drop(ObjectKind::Trigger, &trg, false).unwrap(),
            "DROP TRIGGER audit ON demo.orders;"
        );
        assert_eq!(
            pg.rename(ObjectKind::Trigger, &trg, "audit2").unwrap(),
            "ALTER TRIGGER audit ON demo.orders RENAME TO audit2;"
        );
        assert_eq!(
            Dialect::MySql
                .drop(ObjectKind::Table, &ORDERS, true)
                .unwrap(),
            "DROP TABLE demo.orders;",
            "no CASCADE outside PostgreSQL"
        );
        assert_eq!(
            Dialect::MySql
                .rename(ObjectKind::View, &ORDERS, "o2")
                .unwrap(),
            "RENAME TABLE demo.orders TO demo.o2;"
        );
        assert_eq!(
            Dialect::MySql.rename(ObjectKind::Function, &ORDERS, "x"),
            None
        );
        assert_eq!(Dialect::Sqlite.rename(ObjectKind::View, &ORDERS, "x"), None);
        let duck = Dialect::DuckDb;
        assert_eq!(
            duck.rename(ObjectKind::View, &ORDERS, "x").as_deref(),
            Some("ALTER VIEW demo.orders RENAME TO x;")
        );
        assert_eq!(
            duck.drop(ObjectKind::Function, &ORDERS, true).as_deref(),
            Some("DROP MACRO demo.orders CASCADE;")
        );
        assert_eq!(duck.truncate(&ORDERS), "TRUNCATE TABLE demo.orders;");
        assert_eq!(
            Dialect::Sqlite.drop(ObjectKind::Function, &ORDERS, false),
            None
        );
        assert_eq!(
            Dialect::Sqlite.truncate(&ORDERS),
            "DELETE FROM demo.orders;"
        );
        assert_eq!(
            pg.rename(ObjectKind::MaterializedView, &ORDERS, "Big")
                .unwrap(),
            "ALTER MATERIALIZED VIEW demo.orders RENAME TO \"Big\";"
        );
    }
}
