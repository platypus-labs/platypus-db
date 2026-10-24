//! Transact-SQL definitions rebuilt from the catalog: SQL Server has no `SHOW CREATE TABLE`,
//! and keeps the source text only of views, routines and triggers.

use std::fmt::Write as _;

use platypus_sql::Dialect;

const D: Dialect = Dialect::SqlServer;

/// A column type as declared, from `sys.columns` and `sys.types`: `nvarchar(50)`,
/// `decimal(12,2)`, `varbinary(max)`. `max_length` is in bytes, `None` for `max` types;
/// alias and CLR types keep their own name.
#[must_use]
pub fn declared_type(
    name: &str,
    max_length: Option<u64>,
    precision: u8,
    scale: u8,
    user_defined: bool,
) -> String {
    let length = |chars: bool| match max_length {
        Some(bytes) if chars => (bytes / 2).to_string(),
        Some(bytes) => bytes.to_string(),
        None => "max".to_owned(),
    };
    match name {
        _ if user_defined => name.to_owned(),
        "varchar" | "char" | "varbinary" | "binary" => format!("{name}({})", length(false)),
        "nvarchar" | "nchar" => format!("{name}({})", length(true)),
        "decimal" | "numeric" => format!("{name}({precision},{scale})"),
        "datetime2" | "time" | "datetimeoffset" => format!("{name}({scale})"),
        _ => name.to_owned(),
    }
}

/// A table column, with what its definition needs beyond `ColumnMeta`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    /// The default's expression as stored, e.g. `((0))`.
    pub default: Option<String>,
    pub comment: Option<String>,
    /// `(seed, increment)` of an identity column.
    pub identity: Option<(String, String)>,
    /// The expression of a computed column, and whether it is stored.
    pub computed: Option<(String, bool)>,
}

/// An index, including those behind primary key and unique constraints.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Index {
    pub name: String,
    pub unique: bool,
    pub primary: bool,
    /// Made by a `UNIQUE` constraint rather than `CREATE UNIQUE INDEX`.
    pub unique_constraint: bool,
    /// `CLUSTERED`, `NONCLUSTERED`, `HEAP`, `XML`, `SPATIAL`, ...
    pub kind: String,
    /// Key columns with `true` for descending ones.
    pub columns: Vec<(String, bool)>,
    pub included: Vec<String>,
    pub filter: Option<String>,
    /// Bytes on disk.
    pub size: Option<u64>,
}

impl Index {
    /// Backs a primary key or unique constraint.
    #[must_use]
    pub fn is_constraint(&self) -> bool {
        self.primary || self.unique_constraint
    }

    fn key_list(&self) -> String {
        self.columns
            .iter()
            .map(|(name, desc)| {
                let name = D.quote_ident(name);
                if *desc {
                    format!("{name} DESC")
                } else {
                    name.into_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    /// The column of a column-level check.
    pub column: Option<String>,
    /// As stored, with its parentheses: `([qty]>=(0))`.
    pub definition: String,
}

/// A foreign key; one of a table's own or one that points at it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForeignKey {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub columns: Vec<String>,
    pub ref_schema: String,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    /// `NO_ACTION`, `CASCADE`, `SET_NULL` or `SET_DEFAULT`.
    pub on_delete: String,
    pub on_update: String,
}

/// The quoted `columns`, comma-separated.
#[must_use]
pub fn names(columns: &[String]) -> String {
    columns
        .iter()
        .map(|c| D.quote_ident(c).into_owned())
        .collect::<Vec<_>>()
        .join(", ")
}

fn column_definition(c: &Column) -> String {
    let mut out = D.quote_ident(&c.name).into_owned();
    if let Some((expression, persisted)) = &c.computed {
        let _ = write!(out, " AS {expression}");
        if *persisted {
            out.push_str(" PERSISTED");
            // Only stored computed columns can be declared NOT NULL.
            if !c.nullable {
                out.push_str(" NOT NULL");
            }
        }
        return out;
    }
    let _ = write!(out, " {}", c.data_type);
    if let Some((seed, increment)) = &c.identity {
        let _ = write!(out, " IDENTITY({seed},{increment})");
    }
    out.push_str(if c.nullable { " NULL" } else { " NOT NULL" });
    if let Some(default) = &c.default {
        let _ = write!(out, " DEFAULT {default}");
    }
    out
}

fn action(keyword: &str, action: &str) -> String {
    if action.is_empty() || action == "NO_ACTION" {
        String::new()
    } else {
        format!(" {keyword} {}", action.replace('_', " "))
    }
}

/// `CREATE TABLE` with its columns and constraints, then `CREATE INDEX` for the indexes
/// that back no constraint. Only relational indexes are rebuilt: XML, spatial and
/// columnstore ones are left out.
#[must_use]
pub fn table(
    qualified: &str,
    columns: &[Column],
    indexes: &[Index],
    checks: &[Check],
    foreign_keys: &[ForeignKey],
) -> String {
    let mut lines: Vec<String> = columns.iter().map(column_definition).collect();
    for i in indexes.iter().filter(|i| i.is_constraint()) {
        let keyword = if i.primary { "PRIMARY KEY" } else { "UNIQUE" };
        lines.push(format!(
            "CONSTRAINT {} {keyword} {} ({})",
            D.quote_ident(&i.name),
            i.kind,
            i.key_list()
        ));
    }
    for fk in foreign_keys {
        lines.push(format!(
            "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}){}{}",
            D.quote_ident(&fk.name),
            names(&fk.columns),
            D.qualified(Some(&fk.ref_schema), &fk.ref_table),
            names(&fk.ref_columns),
            action("ON DELETE", &fk.on_delete),
            action("ON UPDATE", &fk.on_update),
        ));
    }
    for c in checks {
        lines.push(format!(
            "CONSTRAINT {} CHECK {}",
            D.quote_ident(&c.name),
            c.definition
        ));
    }
    let mut out = format!(
        "CREATE TABLE {qualified} (\n    {}\n);",
        lines.join(",\n    ")
    );
    let relational = |i: &&Index| matches!(i.kind.as_str(), "CLUSTERED" | "NONCLUSTERED");
    for i in indexes
        .iter()
        .filter(|i| !i.is_constraint())
        .filter(relational)
    {
        let unique = if i.unique { "UNIQUE " } else { "" };
        let _ = write!(
            out,
            "\nCREATE {unique}{} INDEX {} ON {qualified} ({})",
            i.kind,
            D.quote_ident(&i.name),
            i.key_list()
        );
        if !i.included.is_empty() {
            let _ = write!(out, " INCLUDE ({})", names(&i.included));
        }
        if let Some(filter) = &i.filter {
            let _ = write!(out, " WHERE {filter}");
        }
        out.push(';');
    }
    out
}

/// A sequence, from `sys.sequences`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sequence {
    pub data_type: String,
    pub start: String,
    pub increment: String,
    pub min: String,
    pub max: String,
    pub cycle: bool,
}

#[must_use]
pub fn sequence(qualified: &str, s: &Sequence) -> String {
    format!(
        "CREATE SEQUENCE {qualified} AS {}\n    START WITH {}\n    INCREMENT BY {}\n    \
         MINVALUE {}\n    MAXVALUE {}\n    {}CYCLE;",
        s.data_type,
        s.start,
        s.increment,
        s.min,
        s.max,
        if s.cycle { "" } else { "NO " }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declares_types() {
        assert_eq!(
            declared_type("nvarchar", Some(100), 0, 0, false),
            "nvarchar(50)"
        );
        assert_eq!(declared_type("varchar", None, 0, 0, false), "varchar(max)");
        assert_eq!(declared_type("binary", Some(16), 0, 0, false), "binary(16)");
        assert_eq!(
            declared_type("decimal", Some(9), 12, 2, false),
            "decimal(12,2)"
        );
        assert_eq!(
            declared_type("datetime2", Some(8), 27, 7, false),
            "datetime2(7)"
        );
        assert_eq!(declared_type("int", Some(4), 10, 0, false), "int");
        assert_eq!(declared_type("varchar", Some(11), 0, 0, true), "varchar");
        assert_eq!(declared_type("ssn", Some(11), 0, 0, true), "ssn");
    }

    fn column(name: &str, data_type: &str, nullable: bool) -> Column {
        Column {
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            nullable,
            ..Column::default()
        }
    }

    #[test]
    fn rebuilds_tables() {
        let columns = [
            Column {
                identity: Some(("1".to_owned(), "1".to_owned())),
                ..column("id", "int", false)
            },
            Column {
                default: Some("((0))".to_owned()),
                ..column("qty", "int", true)
            },
            column("order", "nvarchar(10)", true),
            Column {
                computed: Some(("([qty]*(2))".to_owned(), true)),
                ..column("double", "int", true)
            },
        ];
        let indexes = [
            Index {
                name: "pk_t".to_owned(),
                unique: true,
                primary: true,
                kind: "CLUSTERED".to_owned(),
                columns: vec![("id".to_owned(), false)],
                ..Index::default()
            },
            Index {
                name: "t_qty".to_owned(),
                kind: "NONCLUSTERED".to_owned(),
                columns: vec![("qty".to_owned(), true)],
                included: vec!["order".to_owned()],
                filter: Some("([qty]>(0))".to_owned()),
                ..Index::default()
            },
            Index {
                name: "t_xml".to_owned(),
                kind: "XML".to_owned(),
                columns: vec![("order".to_owned(), false)],
                ..Index::default()
            },
        ];
        let checks = [Check {
            name: "ck_qty".to_owned(),
            column: Some("qty".to_owned()),
            definition: "([qty]>=(0))".to_owned(),
        }];
        let fks = [ForeignKey {
            name: "fk_c".to_owned(),
            columns: vec!["qty".to_owned()],
            ref_schema: "dbo".to_owned(),
            ref_table: "my table".to_owned(),
            ref_columns: vec!["id".to_owned()],
            on_delete: "SET_NULL".to_owned(),
            on_update: "NO_ACTION".to_owned(),
            ..ForeignKey::default()
        }];
        assert_eq!(
            table("demo.t", &columns, &indexes, &checks, &fks),
            "CREATE TABLE demo.t (\n    \
             id int IDENTITY(1,1) NOT NULL,\n    \
             qty int NULL DEFAULT ((0)),\n    \
             [order] nvarchar(10) NULL,\n    \
             [double] AS ([qty]*(2)) PERSISTED,\n    \
             CONSTRAINT pk_t PRIMARY KEY CLUSTERED (id),\n    \
             CONSTRAINT fk_c FOREIGN KEY (qty) REFERENCES dbo.[my table] (id) ON DELETE SET NULL,\n    \
             CONSTRAINT ck_qty CHECK ([qty]>=(0))\n);\n\
             CREATE NONCLUSTERED INDEX t_qty ON demo.t (qty DESC) INCLUDE ([order]) WHERE ([qty]>(0));"
        );
    }

    #[test]
    fn rebuilds_sequences() {
        let s = Sequence {
            data_type: "bigint".to_owned(),
            start: "1".to_owned(),
            increment: "5".to_owned(),
            min: "1".to_owned(),
            max: "100".to_owned(),
            cycle: true,
        };
        assert_eq!(
            sequence("demo.s", &s),
            "CREATE SEQUENCE demo.s AS bigint\n    START WITH 1\n    INCREMENT BY 5\n    \
             MINVALUE 1\n    MAXVALUE 100\n    CYCLE;"
        );
    }
}
