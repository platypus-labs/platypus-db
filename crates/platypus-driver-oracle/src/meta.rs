//! The navigator's metadata from Oracle's `ALL_*` views: what the session's user can see.

use platypus_core::{
    CatalogColumn, CatalogForeignKey, ColumnMeta, ConstraintKind, ConstraintMeta, ForeignKeyMeta,
    IndexMeta, ObjectKind, ObjectMeta, ObjectRef, ReferencingKey, SchemaMeta, TextRow, TextRowExt,
    group_sorted,
};
use platypus_sql::Dialect;

fn lit(value: &str) -> String {
    Dialect::Oracle.quote_literal(value)
}

/// Oracle's own users on servers before 12.1, which do not mark them (`ORACLE_MAINTAINED`).
const LEGACY_SYSTEM: &str = "'ANONYMOUS', 'APEX_PUBLIC_USER', 'APPQOSSYS', 'CTXSYS', 'DBSNMP', \
     'DIP', 'DVSYS', 'EXFSYS', 'FLOWS_FILES', 'LBACSYS', 'MDDATA', 'MDSYS', 'MGMT_VIEW', 'OLAPSYS', \
     'ORACLE_OCM', 'ORDDATA', 'ORDPLUGINS', 'ORDSYS', 'OUTLN', 'OWBSYS', 'OWBSYS_AUDIT', \
     'SI_INFORMTN_SCHEMA', 'SPATIAL_CSW_ADMIN_USR', 'SPATIAL_WFS_ADMIN_USR', 'SYS', 'SYSMAN', \
     'SYSTEM', 'WMSYS', 'XDB', 'XS$NULL'";

/// What the server's dictionary offers: before 12.1 users are not marked as Oracle's own,
/// and there is no `FETCH FIRST`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dictionary {
    pub legacy: bool,
}

impl Dictionary {
    /// From the server's version, e.g. `23.26.0.0.0`.
    #[must_use]
    pub fn of(version: &str) -> Self {
        let major: u32 = version
            .split('.')
            .next()
            .and_then(|m| m.parse().ok())
            .unwrap_or(99);
        Self { legacy: major < 12 }
    }

    /// `'Y'` for Oracle's own user in `column`, `'N'` otherwise.
    fn system(self, column: &str) -> String {
        if self.legacy {
            format!("CASE WHEN {column} IN ({LEGACY_SYSTEM}) THEN 'Y' ELSE 'N' END")
        } else {
            "ORACLE_MAINTAINED".to_owned()
        }
    }

    /// A condition true when `owner` is a user's schema, not Oracle's own.
    fn user_owner(self, owner: &str) -> String {
        if self.legacy {
            format!("{owner} NOT IN ({LEGACY_SYSTEM})")
        } else {
            format!("{owner} IN (SELECT USERNAME FROM ALL_USERS WHERE ORACLE_MAINTAINED = 'N')")
        }
    }

    /// `query` cut to `limit` rows.
    fn limited(self, query: &str, limit: usize) -> String {
        if self.legacy {
            format!("SELECT * FROM ({query}) WHERE ROWNUM <= {limit}")
        } else {
            format!("{query} FETCH FIRST {limit} ROWS ONLY")
        }
    }

    /// The schemas (users), Oracle's own marked as system ones.
    #[must_use]
    pub fn schemas_sql(self) -> String {
        format!(
            "SELECT USERNAME, {}, \
             CASE WHEN USERNAME = SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') THEN 'Y' ELSE 'N' END \
             FROM ALL_USERS ORDER BY USERNAME",
            self.system("USERNAME")
        )
    }

    /// The columns of the tables and views in the users' schemas, for autocompletion.
    #[must_use]
    pub fn catalog_columns_sql(self, limit: usize) -> String {
        self.limited(
            &format!(
                "SELECT c.OWNER, c.TABLE_NAME, c.COLUMN_NAME FROM ALL_TAB_COLUMNS c \
                 WHERE {} AND c.TABLE_NAME NOT LIKE 'BIN$%' \
                 ORDER BY c.OWNER, c.TABLE_NAME, c.COLUMN_ID",
                self.user_owner("c.OWNER")
            ),
            limit,
        )
    }

    /// The foreign keys between the users' tables, one row per column.
    #[must_use]
    pub fn catalog_foreign_keys_sql(self) -> String {
        format!(
            "SELECT c.OWNER, c.TABLE_NAME, c.CONSTRAINT_NAME, cc.COLUMN_NAME, r.OWNER, \
             r.TABLE_NAME, rc.COLUMN_NAME \
             FROM ALL_CONSTRAINTS c \
             JOIN ALL_CONSTRAINTS r ON r.OWNER = c.R_OWNER \
               AND r.CONSTRAINT_NAME = c.R_CONSTRAINT_NAME \
             JOIN ALL_CONS_COLUMNS cc ON cc.OWNER = c.OWNER \
               AND cc.CONSTRAINT_NAME = c.CONSTRAINT_NAME \
             JOIN ALL_CONS_COLUMNS rc ON rc.OWNER = r.OWNER \
               AND rc.CONSTRAINT_NAME = r.CONSTRAINT_NAME AND rc.POSITION = cc.POSITION \
             WHERE c.CONSTRAINT_TYPE = 'R' AND {} \
             ORDER BY c.OWNER, c.TABLE_NAME, c.CONSTRAINT_NAME, cc.POSITION",
            self.user_owner("c.OWNER")
        )
    }
}

#[must_use]
pub fn schemas(rows: &[TextRow]) -> Vec<SchemaMeta> {
    rows.iter()
        .map(|r| SchemaMeta {
            name: r.text(0),
            system: r.text(1) == "Y",
            current: r.text(2) == "Y",
        })
        .collect()
}

/// The object types the navigator lists, as `ALL_OBJECTS` names them.
const OBJECT_TYPES: &str = "'TABLE', 'VIEW', 'MATERIALIZED VIEW', 'SEQUENCE', 'FUNCTION', \
     'PROCEDURE', 'TRIGGER', 'TYPE', 'PACKAGE', 'SYNONYM'";

/// Tables, views, sequences, routines, triggers, types, packages and synonyms of `schema`,
/// with the comments and row estimates of tables and the table of each trigger. A
/// materialized view is also a table in `ALL_OBJECTS`: listed once; tables in the recycle
/// bin are left out, and so are the sequences of identity columns (`ISEQ$$_…`, generated).
#[must_use]
pub fn objects_sql(schema: &str) -> String {
    let s = lit(schema);
    format!(
        "SELECT o.OBJECT_NAME, o.OBJECT_TYPE, c.COMMENTS, t.TABLE_NAME, s.NUM_ROWS \
         FROM ALL_OBJECTS o \
         LEFT JOIN ALL_TAB_COMMENTS c ON c.OWNER = o.OWNER AND c.TABLE_NAME = o.OBJECT_NAME \
           AND o.OBJECT_TYPE IN ('TABLE', 'VIEW') \
         LEFT JOIN ALL_TRIGGERS t ON t.OWNER = o.OWNER AND t.TRIGGER_NAME = o.OBJECT_NAME \
           AND o.OBJECT_TYPE = 'TRIGGER' \
         LEFT JOIN ALL_TABLES s ON s.OWNER = o.OWNER AND s.TABLE_NAME = o.OBJECT_NAME \
           AND o.OBJECT_TYPE = 'TABLE' \
         WHERE o.OWNER = {s} AND o.OBJECT_TYPE IN ({OBJECT_TYPES}) \
           AND o.OBJECT_NAME NOT LIKE 'BIN$%' \
           AND NOT (o.OBJECT_TYPE = 'SEQUENCE' AND (o.GENERATED = 'Y' \
             OR o.OBJECT_NAME LIKE 'ISEQ$$\\_%' ESCAPE '\\')) \
           AND NOT (o.OBJECT_TYPE = 'TABLE' AND EXISTS (SELECT 1 FROM ALL_MVIEWS m \
             WHERE m.OWNER = o.OWNER AND m.MVIEW_NAME = o.OBJECT_NAME)) \
         ORDER BY o.OBJECT_NAME, o.OBJECT_TYPE"
    )
}

fn object_kind(oracle_type: &str) -> Option<ObjectKind> {
    Some(match oracle_type {
        "TABLE" => ObjectKind::Table,
        "VIEW" => ObjectKind::View,
        "MATERIALIZED VIEW" => ObjectKind::MaterializedView,
        "SEQUENCE" => ObjectKind::Sequence,
        "FUNCTION" => ObjectKind::Function,
        "PROCEDURE" => ObjectKind::Procedure,
        "TRIGGER" => ObjectKind::Trigger,
        "TYPE" => ObjectKind::Type,
        "PACKAGE" => ObjectKind::Package,
        "SYNONYM" => ObjectKind::Synonym,
        _ => return None,
    })
}

#[must_use]
pub fn objects(rows: &[TextRow]) -> Vec<ObjectMeta> {
    rows.iter()
        .filter_map(|r| {
            let mut object = ObjectMeta::new(r.text(0), object_kind(&r.text(1))?, r.opt(2));
            object.parent = r.opt(3);
            object.rows = r.number(4);
            Some(object)
        })
        .collect()
}

/// The columns of a table or view, in order, with their comments.
#[must_use]
pub fn columns_sql(schema: &str, table: &str) -> String {
    format!(
        "SELECT c.COLUMN_NAME, c.DATA_TYPE, c.DATA_PRECISION, c.DATA_SCALE, c.CHAR_LENGTH, \
         c.CHAR_USED, c.NULLABLE, c.DATA_DEFAULT, m.COMMENTS \
         FROM ALL_TAB_COLUMNS c \
         LEFT JOIN ALL_COL_COMMENTS m ON m.OWNER = c.OWNER AND m.TABLE_NAME = c.TABLE_NAME \
           AND m.COLUMN_NAME = c.COLUMN_NAME \
         WHERE c.OWNER = {} AND c.TABLE_NAME = {} ORDER BY c.COLUMN_ID",
        lit(schema),
        lit(table)
    )
}

/// The type as Oracle prints it: `VARCHAR2(20 CHAR)`, `NUMBER(10,2)`, `RAW(16)`.
fn type_text(r: &TextRow) -> String {
    let name = r.text(1);
    let (precision, scale) = (r.opt(2), r.opt(3));
    match name.as_str() {
        "VARCHAR2" | "NVARCHAR2" | "CHAR" | "NCHAR" => {
            let unit = if r.text(5) == "C" && !name.starts_with('N') {
                " CHAR"
            } else {
                ""
            };
            format!("{name}({}{unit})", r.text(4))
        }
        "NUMBER" => match (precision, scale.as_deref()) {
            (None, None) => name,
            (None, Some(s)) => format!("NUMBER(*,{s})"),
            (Some(p), None | Some("0")) => format!("NUMBER({p})"),
            (Some(p), Some(s)) => format!("NUMBER({p},{s})"),
        },
        "FLOAT" => precision.map_or(name.clone(), |p| format!("FLOAT({p})")),
        "RAW" => format!("RAW({})", r.text(4)),
        _ => name,
    }
}

fn column(r: &TextRow) -> ColumnMeta {
    ColumnMeta {
        name: r.text(0),
        data_type: type_text(r),
        nullable: r.text(6) != "N",
        // A `LONG`: kept as typed, often with a trailing newline.
        default: r
            .opt(7)
            .map(|d| d.trim().to_owned())
            .filter(|d| !d.is_empty()),
        primary_key: false,
        comment: r.opt(8),
        enum_values: Vec::new(),
        fields: Vec::new(),
        hidden: false,
    }
}

/// The constraints of a table, one row per column: name, type (`P`, `U`, `C`, `R`), column,
/// check condition (a `LONG` in every version), for a foreign key the referenced schema,
/// table and column, and whether Oracle named the constraint.
#[must_use]
pub fn constraints_sql(schema: &str, table: &str) -> String {
    format!(
        "SELECT c.CONSTRAINT_NAME, c.CONSTRAINT_TYPE, cc.COLUMN_NAME, c.SEARCH_CONDITION, \
         r.OWNER, r.TABLE_NAME, rc.COLUMN_NAME, c.GENERATED \
         FROM ALL_CONSTRAINTS c \
         LEFT JOIN ALL_CONS_COLUMNS cc ON cc.OWNER = c.OWNER \
           AND cc.CONSTRAINT_NAME = c.CONSTRAINT_NAME \
         LEFT JOIN ALL_CONSTRAINTS r ON r.OWNER = c.R_OWNER \
           AND r.CONSTRAINT_NAME = c.R_CONSTRAINT_NAME \
         LEFT JOIN ALL_CONS_COLUMNS rc ON rc.OWNER = r.OWNER \
           AND rc.CONSTRAINT_NAME = r.CONSTRAINT_NAME AND rc.POSITION = cc.POSITION \
         WHERE c.OWNER = {} AND c.TABLE_NAME = {} AND c.CONSTRAINT_TYPE IN ('P', 'U', 'C', 'R') \
         ORDER BY c.CONSTRAINT_NAME, cc.POSITION",
        lit(schema),
        lit(table)
    )
}

/// The indexed column or expression of a row of [`indexes_sql`]: a function-based index (and
/// a descending column, which Oracle also stores as an expression) names a hidden column
/// `SYS_NC…$`, its expression is in `ALL_IND_EXPRESSIONS`.
fn index_column(r: &TextRow) -> String {
    let expression = r
        .opt(4)
        .map(|e| e.trim().to_owned())
        .filter(|e| !e.is_empty());
    let column = match expression {
        // A plain column: the name without its quotes, as other columns show.
        Some(e) => match e.strip_prefix('"').and_then(|e| e.strip_suffix('"')) {
            Some(name) if !name.contains('"') && name == name.to_ascii_uppercase() => {
                name.to_owned()
            }
            _ => e,
        },
        None => r.text(2),
    };
    if r.text(3) == "DESC" {
        format!("{column} DESC")
    } else {
        column
    }
}

/// The indexes of a table, one row per column: name, uniqueness, column, `ASC` or `DESC` and
/// the expression of a function-based index; LOB indexes are the database's own.
#[must_use]
pub fn indexes_sql(schema: &str, table: &str) -> String {
    format!(
        "SELECT i.INDEX_NAME, i.UNIQUENESS, ic.COLUMN_NAME, ic.DESCEND, e.COLUMN_EXPRESSION \
         FROM ALL_INDEXES i \
         JOIN ALL_IND_COLUMNS ic ON ic.INDEX_OWNER = i.OWNER AND ic.INDEX_NAME = i.INDEX_NAME \
         LEFT JOIN ALL_IND_EXPRESSIONS e ON e.INDEX_OWNER = ic.INDEX_OWNER \
           AND e.INDEX_NAME = ic.INDEX_NAME AND e.COLUMN_POSITION = ic.COLUMN_POSITION \
         WHERE i.TABLE_OWNER = {} AND i.TABLE_NAME = {} AND i.INDEX_TYPE <> 'LOB' \
         ORDER BY i.INDEX_NAME, ic.COLUMN_POSITION",
        lit(schema),
        lit(table)
    )
}

/// The foreign keys of other tables pointing at a table, one row per column: their schema,
/// table and name, the column and the referenced column.
#[must_use]
pub fn referencing_sql(schema: &str, table: &str) -> String {
    format!(
        "SELECT c.OWNER, c.TABLE_NAME, c.CONSTRAINT_NAME, cc.COLUMN_NAME, rc.COLUMN_NAME \
         FROM ALL_CONSTRAINTS c \
         JOIN ALL_CONSTRAINTS r ON r.OWNER = c.R_OWNER AND r.CONSTRAINT_NAME = c.R_CONSTRAINT_NAME \
         JOIN ALL_CONS_COLUMNS cc ON cc.OWNER = c.OWNER AND cc.CONSTRAINT_NAME = c.CONSTRAINT_NAME \
         JOIN ALL_CONS_COLUMNS rc ON rc.OWNER = r.OWNER AND rc.CONSTRAINT_NAME = r.CONSTRAINT_NAME \
           AND rc.POSITION = cc.POSITION \
         WHERE c.CONSTRAINT_TYPE = 'R' AND r.OWNER = {} AND r.TABLE_NAME = {} \
         ORDER BY c.OWNER, c.TABLE_NAME, c.CONSTRAINT_NAME, cc.POSITION",
        lit(schema),
        lit(table)
    )
}

#[must_use]
pub fn referencing_keys(rows: &[TextRow]) -> Vec<ReferencingKey> {
    let keyed = rows.iter().map(|r| {
        (
            format!("{}.{}.{}", r.text(0), r.text(1), r.text(2)),
            ReferencingKey {
                schema: r.text(0),
                table: r.text(1),
                name: r.text(2),
                columns: vec![r.text(3)],
                ref_columns: vec![r.text(4)],
            },
        )
    });
    group_sorted(keyed, |key, more| {
        key.columns.extend(more.columns);
        key.ref_columns.extend(more.ref_columns);
    })
}

/// A table's details from the rows of [`columns_sql`], [`constraints_sql`],
/// [`indexes_sql`] and [`referencing_sql`].
#[must_use]
pub fn table_details(
    columns: &[TextRow],
    constraints: &[TextRow],
    indexes: &[TextRow],
    referencing: &[TextRow],
) -> platypus_core::TableDetails {
    // The `NOT NULL` checks Oracle names itself are left out: the columns show them.
    let not_null = |r: &TextRow| {
        r.text(1) == "C"
            && r.text(7) == "GENERATED NAME"
            && r.text(3).trim().ends_with("IS NOT NULL")
    };
    let grouped = group_sorted(
        constraints
            .iter()
            .filter(|r| !not_null(r))
            .map(|r| (r.text(0), vec![r.clone()])),
        Extend::extend,
    );
    let mut columns: Vec<ColumnMeta> = columns.iter().map(column).collect();
    let mut foreign_keys = Vec::new();
    let mut checks = Vec::new();
    let mut primary_name = None;
    for rows in &grouped {
        let first = &rows[0];
        let names: Vec<String> = rows.iter().filter_map(|r| r.opt(2)).collect();
        let (name, kind) = (first.text(0), first.text(1));
        match kind.as_str() {
            "P" | "U" => {
                let (label, constraint) = if kind == "P" {
                    for c in columns.iter_mut().filter(|c| names.contains(&c.name)) {
                        c.primary_key = true;
                    }
                    primary_name = Some(name.clone());
                    ("PRIMARY KEY", ConstraintKind::PrimaryKey)
                } else {
                    ("UNIQUE", ConstraintKind::Unique)
                };
                checks.push(ConstraintMeta {
                    definition: format!("{label} ({})", names.join(", ")),
                    name,
                    kind: constraint,
                    columns: names,
                });
            }
            "C" => checks.push(ConstraintMeta {
                definition: format!("CHECK ({})", first.text(3)),
                name,
                kind: ConstraintKind::Check,
                columns: Vec::new(),
            }),
            _ => foreign_keys.push(ForeignKeyMeta {
                name,
                columns: names,
                ref_schema: first.text(4),
                ref_table: first.text(5),
                ref_columns: rows.iter().map(|r| r.text(6)).collect(),
                detail: None,
            }),
        }
    }
    let indexes = group_sorted(
        indexes.iter().map(|r| {
            let name = r.text(0);
            let index = IndexMeta {
                primary: primary_name.as_ref() == Some(&name),
                partial: false,
                name: name.clone(),
                columns: vec![index_column(r)],
                unique: r.text(1) == "UNIQUE",
                size: None,
                detail: None,
            };
            (name, index)
        }),
        |index, more| index.columns.extend(more.columns),
    );
    platypus_core::TableDetails {
        columns,
        indexes,
        foreign_keys,
        constraints: checks,
        referenced_by: referencing_keys(referencing),
    }
}

#[must_use]
pub fn catalog_columns(rows: &[TextRow]) -> Vec<CatalogColumn> {
    rows.iter()
        .map(|r| CatalogColumn {
            schema: r.text(0),
            table: r.text(1),
            column: r.text(2),
        })
        .collect()
}

#[must_use]
pub fn catalog_foreign_keys(rows: &[TextRow]) -> Vec<CatalogForeignKey> {
    let keyed = rows.iter().map(|r| {
        (
            format!("{}.{}.{}", r.text(0), r.text(1), r.text(2)),
            CatalogForeignKey {
                schema: r.text(0),
                table: r.text(1),
                columns: vec![r.text(3)],
                ref_schema: r.text(4),
                ref_table: r.text(5),
                ref_columns: vec![r.text(6)],
            },
        )
    });
    group_sorted(keyed, |key, more| {
        key.columns.extend(more.columns);
        key.ref_columns.extend(more.ref_columns);
    })
}

/// The `CREATE` statement of an object, from `DBMS_METADATA`; `None` for kinds it does not
/// describe here. The DDL of PL/SQL (routines, triggers, types, packages) can hold several
/// statements (a trigger's `ALTER TRIGGER … ENABLE`, a body after its specification): it
/// comes with its terminators, a `/` line after each block, so that it runs back as a
/// script. Terminators are a session setting, set and reset by a function of the query
/// (Oracle 12c and later; `legacy` dictionaries go without).
#[must_use]
pub fn ddl_sql(object: &ObjectRef<'_>, legacy: bool) -> Option<String> {
    let (kind, plsql) = match object.kind {
        ObjectKind::Table => ("TABLE", false),
        ObjectKind::View => ("VIEW", false),
        ObjectKind::MaterializedView => ("MATERIALIZED_VIEW", false),
        ObjectKind::Sequence => ("SEQUENCE", false),
        ObjectKind::Synonym => ("SYNONYM", false),
        ObjectKind::Function => ("FUNCTION", true),
        ObjectKind::Procedure => ("PROCEDURE", true),
        ObjectKind::Trigger => ("TRIGGER", true),
        ObjectKind::Type => ("TYPE", true),
        ObjectKind::Package => ("PACKAGE", true),
        _ => return None,
    };
    let get = format!(
        "DBMS_METADATA.GET_DDL('{kind}', {}, {})",
        lit(object.name),
        lit(object.schema)
    );
    if !plsql || legacy {
        return Some(format!("SELECT {get} FROM DUAL"));
    }
    Some(format!(
        "WITH FUNCTION platypus_ddl RETURN CLOB IS d CLOB; BEGIN \
         DBMS_METADATA.SET_TRANSFORM_PARAM(DBMS_METADATA.SESSION_TRANSFORM, 'SQLTERMINATOR', TRUE); \
         d := {get}; \
         DBMS_METADATA.SET_TRANSFORM_PARAM(DBMS_METADATA.SESSION_TRANSFORM, 'DEFAULT'); \
         RETURN d; END;\nSELECT platypus_ddl FROM DUAL"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[Option<&str>]) -> TextRow {
        cells.iter().map(|c| c.map(str::to_owned)).collect()
    }

    #[test]
    fn reads_schemas_and_objects() {
        let s = schemas(&[
            row(&[Some("HR"), Some("N"), Some("Y")]),
            row(&[Some("SYS"), Some("Y"), Some("N")]),
        ]);
        assert!(s[0].current && !s[0].system);
        assert!(s[1].system && !s[1].current);
        let o = objects(&[
            row(&[Some("EMP"), Some("TABLE"), Some("staff"), None, Some("14")]),
            row(&[Some("EMP_TRG"), Some("TRIGGER"), None, Some("EMP"), None]),
            row(&[Some("PKG"), Some("PACKAGE"), None, None, None]),
            row(&[Some("J"), Some("JAVA CLASS"), None, None, None]),
        ]);
        assert_eq!(o.len(), 3);
        assert_eq!(o[2].kind, ObjectKind::Package);
        assert_eq!(
            (o[0].kind, o[0].rows, o[0].comment.as_deref()),
            (ObjectKind::Table, Some(14), Some("staff"))
        );
        assert_eq!(o[1].parent.as_deref(), Some("EMP"));
        assert!(objects_sql("O'X").contains("o.OWNER = 'O''X'"));
    }

    /// The constraints of `EMP`, one row per column, as `constraints_sql` reads them.
    fn emp_constraints() -> Vec<TextRow> {
        vec![
            row(&[
                Some("EMP_CHK"),
                Some("C"),
                None,
                Some("PAY > 0"),
                None,
                None,
                None,
            ]),
            row(&[
                Some("EMP_DEPT_FK"),
                Some("R"),
                Some("DEPT_ID"),
                None,
                Some("HR"),
                Some("DEPT"),
                Some("ID"),
            ]),
            row(&[
                Some("EMP_PK"),
                Some("P"),
                Some("ID"),
                None,
                None,
                None,
                None,
            ]),
            // A `NOT NULL` Oracle named: left to the column.
            row(&[
                Some("SYS_C008"),
                Some("C"),
                Some("NAME"),
                Some("\"NAME\" IS NOT NULL"),
                None,
                None,
                None,
                Some("GENERATED NAME"),
            ]),
        ]
    }

    /// An `EMP` table with a check, a foreign key, a primary key, an index of two columns
    /// and a key pointing at it.
    fn emp_details() -> platypus_core::TableDetails {
        let columns = [
            row(&[
                Some("ID"),
                Some("NUMBER"),
                Some("10"),
                Some("0"),
                Some("0"),
                None,
                Some("N"),
                None,
                None,
            ]),
            row(&[
                Some("NAME"),
                Some("VARCHAR2"),
                None,
                None,
                Some("20"),
                Some("C"),
                Some("Y"),
                Some("'x' \n"),
                Some("full name"),
            ]),
            row(&[
                Some("PAY"),
                Some("NUMBER"),
                Some("8"),
                Some("2"),
                Some("0"),
                None,
                Some("Y"),
                None,
                None,
            ]),
            row(&[
                Some("N"),
                Some("NUMBER"),
                None,
                None,
                Some("0"),
                None,
                Some("Y"),
                None,
                None,
            ]),
            row(&[
                Some("AT"),
                Some("TIMESTAMP(6)"),
                None,
                Some("6"),
                Some("0"),
                None,
                Some("Y"),
                None,
                None,
            ]),
        ];
        let constraints = emp_constraints();
        let indexes = [
            row(&[Some("EMP_NAME_IX"), Some("NONUNIQUE"), Some("NAME")]),
            row(&[Some("EMP_NAME_IX"), Some("NONUNIQUE"), Some("PAY")]),
            row(&[Some("EMP_PK"), Some("UNIQUE"), Some("ID")]),
        ];
        let referencing = [row(&[
            Some("HR"),
            Some("PROJ"),
            Some("PROJ_FK"),
            Some("EMP_A"),
            Some("ID"),
        ])];
        table_details(&columns, &constraints, &indexes, &referencing)
    }

    #[test]
    fn builds_table_details() {
        let d = emp_details();
        let types: Vec<_> = d.columns.iter().map(|c| c.data_type.as_str()).collect();
        assert_eq!(
            types,
            [
                "NUMBER(10)",
                "VARCHAR2(20 CHAR)",
                "NUMBER(8,2)",
                "NUMBER",
                "TIMESTAMP(6)"
            ]
        );
        assert!(d.columns[0].primary_key && !d.columns[0].nullable);
        assert_eq!(d.columns[1].default.as_deref(), Some("'x'"));
        assert_eq!(d.columns[1].comment.as_deref(), Some("full name"));
        assert_eq!(d.foreign_keys[0].ref_table, "DEPT");
        assert_eq!(d.foreign_keys[0].ref_columns, ["ID"]);
        assert_eq!(d.constraints.len(), 2);
        assert_eq!(d.constraints[0].definition, "CHECK (PAY > 0)");
        assert_eq!(d.constraints[1].definition, "PRIMARY KEY (ID)");
        assert_eq!(d.indexes[0].columns, ["NAME", "PAY"]);
        assert!(d.indexes[1].primary && d.indexes[1].unique);
        assert_eq!(d.referenced_by[0].table, "PROJ");
    }

    #[test]
    fn older_dictionaries() {
        let old = Dictionary::of("11.2.0.4.0");
        assert!(old.legacy && !Dictionary::of("23.26.0.0.0").legacy);
        assert!(old.schemas_sql().contains("IN ('ANONYMOUS'"));
        let columns = old.catalog_columns_sql(10);
        assert!(columns.ends_with("WHERE ROWNUM <= 10"), "{columns}");
        assert!(!columns.contains("ORACLE_MAINTAINED"));
        let modern = Dictionary::of("19.3.0.0.0").catalog_columns_sql(10);
        assert!(modern.ends_with("FETCH FIRST 10 ROWS ONLY"), "{modern}");
    }

    #[test]
    fn groups_keys_by_name() {
        let keys = catalog_foreign_keys(&[
            row(&[
                Some("HR"),
                Some("E"),
                Some("FK"),
                Some("A"),
                Some("HR"),
                Some("D"),
                Some("X"),
            ]),
            row(&[
                Some("HR"),
                Some("E"),
                Some("FK"),
                Some("B"),
                Some("HR"),
                Some("D"),
                Some("Y"),
            ]),
        ]);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].columns, ["A", "B"]);
        assert_eq!(keys[0].ref_columns, ["X", "Y"]);
    }

    #[test]
    fn ddl_of_objects() {
        let object = ObjectRef {
            schema: "HR",
            name: "EMP",
            kind: ObjectKind::Table,
            parent: None,
        };
        assert_eq!(
            ddl_sql(&object, false).unwrap(),
            "SELECT DBMS_METADATA.GET_DDL('TABLE', 'EMP', 'HR') FROM DUAL"
        );
        let package = ObjectRef {
            kind: ObjectKind::Package,
            ..object
        };
        let sql = ddl_sql(&package, false).unwrap();
        assert!(sql.starts_with("WITH FUNCTION platypus_ddl"), "{sql}");
        assert!(sql.contains("GET_DDL('PACKAGE', 'EMP', 'HR')"), "{sql}");
        assert!(
            ddl_sql(&package, true)
                .unwrap()
                .starts_with("SELECT DBMS_METADATA")
        );
        let object = ObjectRef {
            kind: ObjectKind::Extension,
            ..object
        };
        assert!(ddl_sql(&object, false).is_none());
    }
}
