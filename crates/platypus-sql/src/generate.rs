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
        let sql = format!("SELECT {list}\nFROM {}", self.object(table));
        match (self, limit) {
            (Self::SqlServer, Some(limit)) => format!("{};", self.limited(&sql, limit)),
            (_, Some(limit)) => format!("{sql}\n{};", self.limit_clause(limit)),
            (_, None) => format!("{sql};"),
        }
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
    /// ClickHouse updates through a mutation.
    #[must_use]
    pub fn update(self, table: &ObjectName<'_>, columns: &[&str], keys: &[&str]) -> String {
        let set: Vec<String> = self
            .columns(columns)
            .iter()
            .zip(columns)
            .filter(|(_, c)| !keys.contains(c))
            .map(|(q, _)| format!("{q} = ?"))
            .collect();
        if self == Self::ClickHouse {
            return format!(
                "ALTER TABLE {}
UPDATE {}
WHERE {};",
                self.object(table),
                set.join(
                    ",
    "
                ),
                self.key_filter(columns, keys)
            );
        }
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

    /// Inserts a row or updates it when the key exists. `None` without a key, and in
    /// ClickHouse, whose keys are not unique.
    #[must_use]
    pub fn upsert(self, table: &ObjectName<'_>, columns: &[&str], keys: &[&str]) -> Option<String> {
        if keys.is_empty() || self == Self::ClickHouse {
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
            Self::SqlServer => self.merge(table, columns, keys, &others),
            Self::ClickHouse => return None,
        })
    }

    /// Transact-SQL's upsert: `MERGE` of a one-row source on the key.
    fn merge(
        self,
        table: &ObjectName<'_>,
        columns: &[&str],
        keys: &[&str],
        others: &[String],
    ) -> String {
        let all = self.columns(columns);
        let on: Vec<String> = self
            .columns(keys)
            .iter()
            .map(|k| format!("target.{k} = source.{k}"))
            .collect();
        let matched = if others.is_empty() {
            String::new()
        } else {
            let set: Vec<String> = others.iter().map(|q| format!("{q} = source.{q}")).collect();
            format!("\nWHEN MATCHED THEN\n  UPDATE SET {}", set.join(",\n    "))
        };
        let values: Vec<String> = all.iter().map(|q| format!("source.{q}")).collect();
        format!(
            "MERGE INTO {} AS target\nUSING (VALUES ({})) AS source ({})\nON {}{matched}\n\
             WHEN NOT MATCHED THEN\n  INSERT ({}) VALUES ({});",
            self.object(table),
            vec!["?"; columns.len()].join(", "),
            all.join(", "),
            on.join(" AND "),
            all.join(", "),
            values.join(", ")
        )
    }

    /// The SQL keyword for the kind in `DROP`/`ALTER`, or `None` where the dialect has no
    /// such object.
    const fn keyword(self, kind: ObjectKind) -> Option<&'static str> {
        Some(match (self, kind) {
            (_, ObjectKind::Table) => "TABLE",
            // ClickHouse drops a materialized view with `DROP VIEW`.
            (_, ObjectKind::View) | (Self::ClickHouse, ObjectKind::MaterializedView) => "VIEW",
            // ClickHouse has no triggers.
            (
                Self::Postgres | Self::MySql | Self::Sqlite | Self::DuckDb | Self::SqlServer,
                ObjectKind::Trigger,
            ) => "TRIGGER",
            (Self::Postgres, ObjectKind::MaterializedView) => "MATERIALIZED VIEW",
            (Self::Postgres, ObjectKind::ForeignTable) => "FOREIGN TABLE",
            (Self::Postgres | Self::DuckDb | Self::SqlServer, ObjectKind::Type) => "TYPE",
            (Self::Postgres, ObjectKind::Domain) => "DOMAIN",
            (Self::Postgres, ObjectKind::Extension) => "EXTENSION",
            (
                Self::Postgres | Self::MySql | Self::DuckDb | Self::SqlServer,
                ObjectKind::Sequence,
            ) => "SEQUENCE",
            // DuckDB's user functions are macros.
            (Self::DuckDb, ObjectKind::Function) => "MACRO",
            (Self::Postgres | Self::MySql | Self::SqlServer, ObjectKind::Function) => "FUNCTION",
            (Self::Postgres | Self::MySql | Self::SqlServer, ObjectKind::Procedure) => "PROCEDURE",
            (Self::MySql, ObjectKind::Event) => "EVENT",
            (Self::ClickHouse, ObjectKind::Dictionary) => "DICTIONARY",
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
            // `RENAME TABLE` also moves views; dictionaries have their own statement.
            Self::ClickHouse => match kind {
                ObjectKind::Table | ObjectKind::View | ObjectKind::MaterializedView => {
                    Some(format!(
                        "RENAME TABLE {} TO {};",
                        self.object(o),
                        self.qualified(o.schema, new_name)
                    ))
                }
                ObjectKind::Dictionary => Some(format!(
                    "RENAME DICTIONARY {} TO {};",
                    self.object(o),
                    self.qualified(o.schema, new_name)
                )),
                _ => None,
            },
            // `sp_rename` keeps the schema; it does not rewrite the source of routines.
            Self::SqlServer => matches!(
                kind,
                ObjectKind::Table
                    | ObjectKind::View
                    | ObjectKind::Sequence
                    | ObjectKind::Function
                    | ObjectKind::Procedure
                    | ObjectKind::Trigger
            )
            .then(|| {
                format!(
                    "EXEC sp_rename {}, {};",
                    self.quote_literal(&self.object(o)),
                    self.quote_literal(new_name)
                )
            }),
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
        assert_eq!(
            Dialect::SqlServer.upsert(&ORDERS, &cols, &["id"]).unwrap(),
            "MERGE INTO demo.orders AS target\nUSING (VALUES (?, ?)) AS source (id, total)\n\
             ON target.id = source.id\nWHEN MATCHED THEN\n  UPDATE SET total = source.total\n\
             WHEN NOT MATCHED THEN\n  INSERT (id, total) VALUES (source.id, source.total);"
        );
        assert_eq!(
            Dialect::SqlServer.select(&ORDERS, &[], Some(5)),
            "SELECT TOP (5) *\nFROM demo.orders;"
        );
        assert_eq!(
            Dialect::SqlServer.rename(ObjectKind::Table, &ORDERS, "o2"),
            Some("EXEC sp_rename N'demo.orders', N'o2';".to_owned())
        );
    }

    #[test]
    fn clickhouse_statements() {
        let ch = Dialect::ClickHouse;
        assert_eq!(
            ch.select(&ORDERS, &["id"], Some(5)),
            "SELECT id\nFROM demo.orders\nLIMIT 5;"
        );
        assert_eq!(
            ch.update(&ORDERS, &["id", "total"], &["id"]),
            "ALTER TABLE demo.orders\nUPDATE total = ?\nWHERE id = ?;"
        );
        assert_eq!(
            ch.delete(&ORDERS, &["id"], &["id"]),
            "DELETE FROM demo.orders\nWHERE id = ?;"
        );
        assert_eq!(ch.upsert(&ORDERS, &["id"], &["id"]), None);
        assert_eq!(
            ch.drop(ObjectKind::MaterializedView, &ORDERS, true)
                .as_deref(),
            Some("DROP VIEW demo.orders;")
        );
        assert_eq!(
            ch.drop(ObjectKind::Dictionary, &ORDERS, false).as_deref(),
            Some("DROP DICTIONARY demo.orders;")
        );
        assert_eq!(ch.drop(ObjectKind::Trigger, &ORDERS, false), None);
        assert_eq!(
            ch.rename(ObjectKind::View, &ORDERS, "o2").as_deref(),
            Some("RENAME TABLE demo.orders TO demo.o2;")
        );
        assert_eq!(
            ch.rename(ObjectKind::Dictionary, &ORDERS, "d").as_deref(),
            Some("RENAME DICTIONARY demo.orders TO demo.d;")
        );
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
