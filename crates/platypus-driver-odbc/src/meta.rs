//! The rows of ODBC's catalog functions (`SQLTables`, `SQLColumns`, `SQLPrimaryKeys`,
//! `SQLForeignKeys`) as plain structs, and the app's metadata made from them.

use platypus_core::{
    CatalogColumn, ColumnMeta, DriverKind, ForeignKeyMeta, IndexMeta, ObjectKind, ObjectMeta,
    ReferencingKey, SchemaMeta, TableDetails,
};

/// A row of `SQLTables`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableRow {
    pub schema: String,
    pub name: String,
    /// `TABLE`, `VIEW`, `SYSTEM TABLE`, `SYNONYM`…, as the driver names it.
    pub kind: String,
    pub remarks: Option<String>,
}

/// A row of `SQLColumns`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnRow {
    pub schema: String,
    pub table: String,
    pub name: String,
    /// The SQL type code (`SQL_VARCHAR` is 12…).
    pub sql_type: i16,
    pub type_name: String,
    pub size: Option<i32>,
    pub digits: Option<i16>,
    pub nullable: bool,
    pub default: Option<String>,
    pub remarks: Option<String>,
}

/// A row of `SQLPrimaryKeys`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyRow {
    pub column: String,
    pub seq: i16,
    pub name: Option<String>,
}

/// A row of `SQLForeignKeys`: one column of a key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForeignKeyRow {
    pub pk_schema: String,
    pub pk_table: String,
    pub pk_column: String,
    pub fk_schema: String,
    pub fk_table: String,
    pub fk_column: String,
    pub seq: i16,
    pub name: Option<String>,
}

/// Schemas the database itself keeps: Db2's catalog, the standard `INFORMATION_SCHEMA` and
/// SQL Server's `sys`.
fn is_system(schema: &str, kind: DriverKind) -> bool {
    let upper = schema.to_ascii_uppercase();
    match kind {
        DriverKind::Db2 => upper.starts_with("SYS") || matches!(upper.as_str(), "NULLID" | "SQLJ"),
        _ => matches!(upper.as_str(), "INFORMATION_SCHEMA" | "SYS" | "PG_CATALOG"),
    }
}

/// The schemas named, by name, the `current` one marked.
#[must_use]
pub fn schemas(names: Vec<String>, current: Option<&str>, kind: DriverKind) -> Vec<SchemaMeta> {
    let mut names = names;
    names.sort();
    names.dedup();
    names
        .into_iter()
        .map(|name| SchemaMeta {
            system: is_system(&name, kind),
            current: current.is_some_and(|c| c == name),
            name,
        })
        .collect()
}

/// The object kind of a `SQLTables` type; `None` for what the navigator does not list
/// (synonyms, aliases, system tables).
fn kind(table_type: &str) -> Option<ObjectKind> {
    match table_type.to_ascii_uppercase().as_str() {
        "TABLE" | "GLOBAL TEMPORARY" | "LOCAL TEMPORARY" | "BASE TABLE" => Some(ObjectKind::Table),
        "VIEW" => Some(ObjectKind::View),
        "MATERIALIZED VIEW" | "MATERIALIZED QUERY TABLE" => Some(ObjectKind::MaterializedView),
        _ => None,
    }
}

/// The tables and views of a schema, by name.
#[must_use]
pub fn objects(rows: Vec<TableRow>) -> Vec<ObjectMeta> {
    let mut out: Vec<ObjectMeta> = rows
        .into_iter()
        .filter_map(|t| Some(ObjectMeta::new(t.name, kind(&t.kind)?, t.remarks)))
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name);
    out
}

/// The type as the database prints it: `VARCHAR2(50)`, `NUMBER(10,2)`.
fn type_text(c: &ColumnRow) -> String {
    // SQL_CHAR, SQL_VARCHAR, SQL_WCHAR, SQL_WVARCHAR, SQL_BINARY, SQL_VARBINARY.
    const SIZED: &[i16] = &[1, 12, -8, -9, -2, -3];
    // SQL_NUMERIC, SQL_DECIMAL.
    const EXACT: &[i16] = &[2, 3];
    if c.type_name.contains('(') {
        return c.type_name.clone();
    }
    match (c.size, c.digits) {
        (Some(size), _) if SIZED.contains(&c.sql_type) => format!("{}({size})", c.type_name),
        (Some(p), Some(s)) if EXACT.contains(&c.sql_type) && s > 0 => {
            format!("{}({p},{s})", c.type_name)
        }
        (Some(p), _) if EXACT.contains(&c.sql_type) => format!("{}({p})", c.type_name),
        _ => c.type_name.clone(),
    }
}

/// Groups foreign key column rows into keys, in column order.
fn group_keys(mut rows: Vec<ForeignKeyRow>) -> Vec<Vec<ForeignKeyRow>> {
    let key = |r: &ForeignKeyRow| {
        (
            r.fk_schema.clone(),
            r.fk_table.clone(),
            r.name.clone(),
            r.pk_schema.clone(),
            r.pk_table.clone(),
        )
    };
    rows.sort_by(|a, b| key(a).cmp(&key(b)).then(a.seq.cmp(&b.seq)));
    let mut groups: Vec<Vec<ForeignKeyRow>> = Vec::new();
    for row in rows {
        match groups.last_mut() {
            // A key without a name starts again at column 1.
            Some(group) if key(&group[0]) == key(&row) && row.seq > group[group.len() - 1].seq => {
                group.push(row);
            }
            _ => groups.push(vec![row]),
        }
    }
    groups
}

/// The columns, primary key and foreign keys of a table, and the keys pointing at it.
#[must_use]
pub fn table_details(
    columns: Vec<ColumnRow>,
    mut primary: Vec<KeyRow>,
    foreign: Vec<ForeignKeyRow>,
    referencing: Vec<ForeignKeyRow>,
) -> TableDetails {
    primary.sort_by_key(|k| k.seq);
    let in_key = |name: &str| primary.iter().any(|k| k.column == name);
    let columns = columns
        .into_iter()
        .map(|c| ColumnMeta {
            data_type: type_text(&c),
            primary_key: in_key(&c.name),
            name: c.name,
            nullable: c.nullable,
            default: c.default,
            comment: c.remarks,
            enum_values: Vec::new(),
            fields: Vec::new(),
        })
        .collect();
    let indexes = primary
        .first()
        .map(|first| IndexMeta {
            name: first
                .name
                .clone()
                .unwrap_or_else(|| "PRIMARY KEY".to_owned()),
            columns: primary.iter().map(|k| k.column.clone()).collect(),
            unique: true,
            primary: true,
            size: None,
        })
        .into_iter()
        .collect();
    let foreign_keys = group_keys(foreign)
        .into_iter()
        .map(|key| ForeignKeyMeta {
            name: key[0].name.clone().unwrap_or_default(),
            columns: key.iter().map(|r| r.fk_column.clone()).collect(),
            ref_schema: key[0].pk_schema.clone(),
            ref_table: key[0].pk_table.clone(),
            ref_columns: key.iter().map(|r| r.pk_column.clone()).collect(),
        })
        .collect();
    TableDetails {
        columns,
        indexes,
        foreign_keys,
        referenced_by: referencing_keys(referencing),
        ..TableDetails::default()
    }
}

/// The foreign keys of other tables that point at a table.
#[must_use]
pub fn referencing_keys(rows: Vec<ForeignKeyRow>) -> Vec<ReferencingKey> {
    group_keys(rows)
        .into_iter()
        .map(|key| ReferencingKey {
            schema: key[0].fk_schema.clone(),
            table: key[0].fk_table.clone(),
            name: key[0].name.clone().unwrap_or_default(),
            columns: key.iter().map(|r| r.fk_column.clone()).collect(),
            ref_columns: key.iter().map(|r| r.pk_column.clone()).collect(),
        })
        .collect()
}

/// The columns of the user schemas, for autocompletion; at most `limit`.
#[must_use]
pub fn catalog_columns(rows: Vec<ColumnRow>, kind: DriverKind, limit: usize) -> Vec<CatalogColumn> {
    rows.into_iter()
        .filter(|c| !is_system(&c.schema, kind))
        .take(limit)
        .map(|c| CatalogColumn {
            schema: c.schema,
            table: c.table,
            column: c.name,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fk(fk_table: &str, fk_column: &str, pk_column: &str, seq: i16, name: &str) -> ForeignKeyRow {
        ForeignKeyRow {
            pk_schema: "HR".to_owned(),
            pk_table: "DEPT".to_owned(),
            pk_column: pk_column.to_owned(),
            fk_schema: "HR".to_owned(),
            fk_table: fk_table.to_owned(),
            fk_column: fk_column.to_owned(),
            seq,
            name: Some(name.to_owned()),
        }
    }

    #[test]
    fn schemas_and_objects() {
        let s = schemas(
            vec!["SYSCAT".into(), "HR".into(), "SYSIBM".into(), "HR".into()],
            Some("HR"),
            DriverKind::Db2,
        );
        let names: Vec<_> = s
            .iter()
            .map(|s| (s.name.as_str(), s.system, s.current))
            .collect();
        assert_eq!(
            names,
            [
                ("HR", false, true),
                ("SYSCAT", true, false),
                ("SYSIBM", true, false)
            ]
        );
        assert!(schemas(vec!["dbo".into(), "sys".into()], None, DriverKind::Odbc)[1].system);
        let objects = objects(vec![
            TableRow {
                name: "EMP".into(),
                kind: "TABLE".into(),
                ..TableRow::default()
            },
            TableRow {
                name: "EMP_V".into(),
                kind: "VIEW".into(),
                remarks: Some("staff".into()),
                ..TableRow::default()
            },
            TableRow {
                name: "E".into(),
                kind: "SYNONYM".into(),
                ..TableRow::default()
            },
        ]);
        assert_eq!(objects.len(), 2);
        assert_eq!(objects[1].kind, ObjectKind::View);
        assert_eq!(objects[1].comment.as_deref(), Some("staff"));
    }

    #[test]
    fn details_of_a_table() {
        let column = |name: &str, sql_type, type_name: &str, size, digits| ColumnRow {
            name: name.to_owned(),
            sql_type,
            type_name: type_name.to_owned(),
            size,
            digits,
            nullable: name != "ID",
            ..ColumnRow::default()
        };
        let details = table_details(
            vec![
                column("ID", 3, "NUMBER", Some(10), Some(0)),
                column("NAME", 12, "VARCHAR2", Some(50), None),
                column("PAY", 3, "NUMBER", Some(10), Some(2)),
                column("AT", 93, "DATE", Some(19), Some(0)),
            ],
            vec![KeyRow {
                column: "ID".into(),
                seq: 1,
                name: Some("EMP_PK".into()),
            }],
            vec![fk("EMP", "DEPT_ID", "ID", 1, "EMP_DEPT_FK")],
            vec![
                fk("PROJ", "DEPT_B", "B", 2, "PROJ_FK"),
                fk("PROJ", "DEPT_A", "A", 1, "PROJ_FK"),
            ],
        );
        let types: Vec<_> = details
            .columns
            .iter()
            .map(|c| c.data_type.as_str())
            .collect();
        assert_eq!(
            types,
            ["NUMBER(10)", "VARCHAR2(50)", "NUMBER(10,2)", "DATE"]
        );
        assert!(details.columns[0].primary_key && !details.columns[0].nullable);
        assert_eq!(details.indexes[0].name, "EMP_PK");
        assert_eq!(details.foreign_keys[0].ref_table, "DEPT");
        assert_eq!(details.foreign_keys[0].columns, ["DEPT_ID"]);
        assert_eq!(details.referenced_by.len(), 1);
        assert_eq!(details.referenced_by[0].columns, ["DEPT_A", "DEPT_B"]);
        assert_eq!(details.referenced_by[0].ref_columns, ["A", "B"]);
    }

    #[test]
    fn unnamed_keys_split_on_their_first_column() {
        let mut a = fk("T", "X", "A", 1, "");
        let mut b = fk("T", "Y", "B", 1, "");
        a.name = None;
        b.name = None;
        assert_eq!(referencing_keys(vec![a, b]).len(), 2);
    }
}
