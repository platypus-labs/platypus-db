//! Database objects for the navigator and the object tab: DDL, properties, generated SQL
//! and the statements that change objects (drop, rename, truncate).

use futures::TryStreamExt;
use platypus_core::{DriverKind, ExecOptions, ExecResult, ObjectKind, ObjectRef, Property};
use platypus_plugins::QueryStatus;
use platypus_sql::{Dialect, ObjectName};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::{AppState, one_off, plugins};
use crate::error::CommandResult;

/// An object as the UI identifies it.
#[derive(Debug, Deserialize)]
pub struct ObjectTarget {
    schema: String,
    name: String,
    kind: ObjectKind,
    /// The table of a trigger.
    parent: Option<String>,
}

impl ObjectTarget {
    fn as_ref(&self) -> ObjectRef<'_> {
        ObjectRef {
            schema: &self.schema,
            name: &self.name,
            kind: self.kind,
            parent: self.parent.as_deref(),
        }
    }
}

#[tauri::command]
pub async fn object_ddl(
    state: State<'_, AppState>,
    session_id: u64,
    object: ObjectTarget,
) -> CommandResult<String> {
    let conn = state.live(session_id).await?;
    Ok(conn.object_ddl(&object.as_ref()).await?)
}

/// The DDL of one object of `objects_ddl`, or why it could not be read.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectDdl {
    Ddl(String),
    Error(String),
}

/// The DDL of several objects, each with its own result: the driver reads them with fewer
/// queries than one call a object.
#[tauri::command]
pub async fn objects_ddl(
    state: State<'_, AppState>,
    session_id: u64,
    objects: Vec<ObjectTarget>,
) -> CommandResult<Vec<ObjectDdl>> {
    let conn = state.live(session_id).await?;
    let refs: Vec<ObjectRef<'_>> = objects.iter().map(ObjectTarget::as_ref).collect();
    Ok(conn
        .objects_ddl(&refs)
        .await
        .into_iter()
        .map(|r| r.map_or_else(|e| ObjectDdl::Error(e.to_string()), ObjectDdl::Ddl))
        .collect())
}

#[tauri::command]
pub async fn object_properties(
    state: State<'_, AppState>,
    session_id: u64,
    object: ObjectTarget,
) -> CommandResult<Vec<Property>> {
    let conn = state.live(session_id).await?;
    Ok(conn.object_properties(&object.as_ref()).await?)
}

/// Other databases of the server, for connections whose driver supports switching.
#[tauri::command]
pub async fn list_databases(
    state: State<'_, AppState>,
    session_id: u64,
) -> CommandResult<Vec<String>> {
    let conn = state.live(session_id).await?;
    Ok(conn.databases().await?)
}

/// Runs a statement that returns nothing worth showing (DDL from the navigator), reading
/// and discarding any rows, and commits it (see [`one_off`]). Returns the affected row count
/// when there is one.
#[tauri::command]
pub async fn execute_statement(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    session_id: u64,
    sql: String,
) -> CommandResult<Option<u64>> {
    state.may_run(session_id, &sql)?;
    let after = if plugins::query_hooked() {
        plugins::before_query(&app, &state, session_id, &sql).await?
    } else {
        None
    };
    let conn = state.live(session_id).await?;
    let started = std::time::Instant::now();
    let result = one_off(&conn, async {
        match conn.execute(&sql, &ExecOptions::default()).await? {
            ExecResult::Affected { rows } => Ok(Some(rows)),
            ExecResult::Rows(stream) => {
                stream.try_for_each(|_| async { Ok(()) }).await?;
                Ok(None)
            }
        }
    })
    .await;
    if let Some(after) = after {
        let (status, affected) = match &result {
            Ok(rows) => (QueryStatus::Ok, *rows),
            Err(_) => (QueryStatus::Error, None),
        };
        after.fire(status, started.elapsed(), affected);
    }
    result
}

/// A table or other object to generate SQL for. Columns and keys apply to DML.
#[derive(Debug, Deserialize)]
pub struct GenerateTarget {
    /// `None` for the session's current schema.
    schema: Option<String>,
    name: String,
    kind: ObjectKind,
    parent: Option<String>,
    #[serde(default)]
    columns: Vec<String>,
    /// Primary key columns.
    #[serde(default)]
    keys: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "statement", rename_all = "snake_case")]
pub enum GenerateRequest {
    Select { limit: Option<u64> },
    Insert,
    Update,
    Delete,
    Upsert,
    Drop { cascade: bool },
    Truncate,
    Rename { new_name: String },
}

/// The statement for `request`, or `None` when the dialect has no such statement for the
/// object (e.g. renaming a MySQL function, an upsert without a key).
#[tauri::command]
pub fn generate_sql(
    driver: DriverKind,
    target: GenerateTarget,
    request: GenerateRequest,
) -> Option<String> {
    if driver == DriverKind::Mongo {
        return mongo_statement(&target, request);
    }
    // A key type's virtual table is only read; its keys change in the grid or the editor.
    if driver == DriverKind::Redis {
        return match request {
            GenerateRequest::Select { limit } => Some(platypus_transfer::import::select_all(
                driver,
                (target.schema.as_deref(), &target.name),
                limit,
            )),
            _ => None,
        };
    }
    // An index is read with a search and deleted with a request; its documents change in
    // the grid or the editor.
    if driver == DriverKind::Elasticsearch {
        use platypus_driver_search::{command, meta};
        return match request {
            GenerateRequest::Select { limit } => Some(command::browse_limited(&target.name, limit)),
            GenerateRequest::Drop { .. } => {
                Some(format!("DELETE /{}", meta::segment(&target.name)))
            }
            _ => None,
        };
    }
    let d = Dialect::from(driver);
    let name = ObjectName {
        schema: target.schema.as_deref(),
        name: &target.name,
        parent: target.parent.as_deref(),
    };
    let columns: Vec<&str> = target.columns.iter().map(String::as_str).collect();
    let keys: Vec<&str> = target.keys.iter().map(String::as_str).collect();
    if driver == DriverKind::Cassandra {
        return cql_statement(&target, &request, (&name, &columns, &keys));
    }
    match request {
        GenerateRequest::Select { limit } => Some(d.select(&name, &columns, limit)),
        GenerateRequest::Insert => Some(d.insert(&name, &columns)),
        GenerateRequest::Update => Some(d.update(&name, &columns, &keys)),
        GenerateRequest::Delete => Some(d.delete(&name, &columns, &keys)),
        GenerateRequest::Upsert => d.upsert(&name, &columns, &keys),
        GenerateRequest::Drop { cascade } => d.drop(target.kind, &name, cascade),
        GenerateRequest::Truncate => Some(d.truncate(&name)),
        GenerateRequest::Rename { new_name } => d.rename(target.kind, &name, &new_name),
    }
}

/// [`generate_sql`] in CQL: the standard statements for reading and writing rows, with CQL's
/// `LIMIT`, and the `DROP` and `TRUNCATE` it has. A row is written by its whole primary key, so
/// an `UPDATE` or a `DELETE` without one has no statement.
fn cql_statement(
    target: &GenerateTarget,
    request: &GenerateRequest,
    (name, columns, keys): (&ObjectName<'_>, &[&str], &[&str]),
) -> Option<String> {
    let d = Dialect::Ansi;
    let table = match target.schema.as_deref() {
        Some(schema) => platypus_driver_cassandra::cql::qualified(schema, &target.name),
        None => platypus_driver_cassandra::cql::quote_ident(&target.name),
    };
    match request {
        GenerateRequest::Select { limit } => {
            let sql = d.select(name, columns, None);
            Some(match limit {
                Some(limit) => format!("{}\nLIMIT {limit};", sql.trim_end_matches(';')),
                None => sql,
            })
        }
        GenerateRequest::Insert | GenerateRequest::Upsert => Some(d.insert(name, columns)),
        GenerateRequest::Update if !keys.is_empty() => Some(d.update(name, columns, keys)),
        GenerateRequest::Delete if !keys.is_empty() => Some(d.delete(name, columns, keys)),
        GenerateRequest::Drop { .. } => match target.kind {
            ObjectKind::Table => Some(format!("DROP TABLE {table};")),
            ObjectKind::MaterializedView => Some(format!("DROP MATERIALIZED VIEW {table};")),
            ObjectKind::Type => Some(format!("DROP TYPE {table};")),
            _ => None,
        },
        GenerateRequest::Truncate if target.kind == ObjectKind::Table => {
            Some(format!("TRUNCATE {table};"))
        }
        _ => None,
    }
}

/// [`generate_sql`] in mongosh.
fn mongo_statement(target: &GenerateTarget, request: GenerateRequest) -> Option<String> {
    use platypus_driver_mongo::generate::{Statement, statement};
    let columns: Vec<&str> = target.columns.iter().map(String::as_str).collect();
    let request = match &request {
        GenerateRequest::Select { limit } => Statement::Select { limit: *limit },
        GenerateRequest::Insert => Statement::Insert,
        GenerateRequest::Update => Statement::Update,
        GenerateRequest::Delete => Statement::Delete,
        GenerateRequest::Upsert => Statement::Upsert,
        GenerateRequest::Drop { .. } => Statement::Drop,
        GenerateRequest::Truncate => Statement::Truncate,
        GenerateRequest::Rename { new_name } => Statement::Rename { new_name },
    };
    statement(
        target.schema.as_deref(),
        &target.name,
        target.kind,
        &columns,
        &request,
    )
}

/// What the object actions can do in a dialect, so the UI shows only those that apply.
#[derive(Debug, Serialize)]
pub struct DialectSupport {
    cascade: bool,
    droppable: Vec<ObjectKind>,
    renamable: Vec<ObjectKind>,
}

const ALL_KINDS: [ObjectKind; 15] = [
    ObjectKind::Table,
    ObjectKind::View,
    ObjectKind::MaterializedView,
    ObjectKind::ForeignTable,
    ObjectKind::Sequence,
    ObjectKind::Function,
    ObjectKind::Procedure,
    ObjectKind::Trigger,
    ObjectKind::Type,
    ObjectKind::Domain,
    ObjectKind::Extension,
    ObjectKind::Event,
    ObjectKind::Dictionary,
    ObjectKind::Synonym,
    ObjectKind::Package,
];

#[tauri::command]
pub fn dialect_support(driver: DriverKind) -> DialectSupport {
    if driver == DriverKind::Mongo {
        return DialectSupport {
            cascade: false,
            droppable: vec![ObjectKind::Table, ObjectKind::View],
            renamable: vec![ObjectKind::Table],
        };
    }
    if driver == DriverKind::Elasticsearch {
        return DialectSupport {
            cascade: false,
            droppable: vec![ObjectKind::Table],
            renamable: Vec::new(),
        };
    }
    if driver == DriverKind::Redis {
        return DialectSupport {
            cascade: false,
            droppable: Vec::new(),
            renamable: Vec::new(),
        };
    }
    // CQL renames nothing (a column of a table's key apart), and drops by kind.
    if driver == DriverKind::Cassandra {
        return DialectSupport {
            cascade: false,
            droppable: vec![
                ObjectKind::Table,
                ObjectKind::MaterializedView,
                ObjectKind::Type,
            ],
            renamable: Vec::new(),
        };
    }
    let d = Dialect::from(driver);
    // A probe name with a parent and arguments, so every kind can be tried.
    let probe = ObjectName {
        schema: Some("s"),
        name: "o()",
        parent: Some("t"),
    };
    DialectSupport {
        cascade: d.supports_cascade(),
        droppable: ALL_KINDS
            .into_iter()
            .filter(|&k| d.drop(k, &probe, false).is_some())
            .collect(),
        renamable: ALL_KINDS
            .into_iter()
            .filter(|&k| d.rename(k, &probe, "n").is_some())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_dialect_support() {
        let pg = dialect_support(DriverKind::Postgres);
        assert!(pg.cascade);
        assert!(pg.droppable.contains(&ObjectKind::Extension));
        assert!(!pg.renamable.contains(&ObjectKind::Extension));
        let sqlite = dialect_support(DriverKind::Sqlite);
        assert_eq!(sqlite.renamable, [ObjectKind::Table]);
        assert_eq!(
            sqlite.droppable,
            [ObjectKind::Table, ObjectKind::View, ObjectKind::Trigger]
        );
        let mssql = dialect_support(DriverKind::SqlServer);
        assert!(!mssql.cascade);
        assert!(mssql.droppable.contains(&ObjectKind::Type));
        assert!(mssql.renamable.contains(&ObjectKind::Procedure));
        assert!(!mssql.renamable.contains(&ObjectKind::Type));
        let ch = dialect_support(DriverKind::ClickHouse);
        assert!(ch.droppable.contains(&ObjectKind::Dictionary));
        assert!(ch.renamable.contains(&ObjectKind::MaterializedView));
        assert!(!ch.droppable.contains(&ObjectKind::Trigger));
        let cql = dialect_support(DriverKind::Cassandra);
        assert!(cql.renamable.is_empty() && !cql.cascade);
        assert_eq!(cql.droppable.len(), 3);
    }

    #[test]
    fn generates_cql() {
        let target = |kind, keys: &[&str]| GenerateTarget {
            schema: Some("shop".to_owned()),
            name: "Orders".to_owned(),
            kind,
            parent: None,
            columns: vec!["id".to_owned(), "total".to_owned()],
            keys: keys.iter().map(|k| (*k).to_owned()).collect(),
        };
        let gen_ = |kind, keys: &[&str], request| {
            generate_sql(DriverKind::Cassandra, target(kind, keys), request)
        };
        let select = gen_(
            ObjectKind::Table,
            &[],
            GenerateRequest::Select { limit: Some(50) },
        );
        assert_eq!(
            select.as_deref(),
            Some("SELECT \"id\",\n       \"total\"\nFROM \"shop\".\"Orders\"\nLIMIT 50;")
        );
        assert!(
            gen_(ObjectKind::Table, &["id"], GenerateRequest::Update)
                .unwrap()
                .starts_with("UPDATE")
        );
        assert!(gen_(ObjectKind::Table, &[], GenerateRequest::Update).is_none());
        assert!(gen_(ObjectKind::Table, &[], GenerateRequest::Delete).is_none());
        assert_eq!(
            gen_(
                ObjectKind::Table,
                &[],
                GenerateRequest::Drop { cascade: true }
            )
            .as_deref(),
            Some("DROP TABLE shop.\"Orders\";")
        );
        assert_eq!(
            gen_(
                ObjectKind::MaterializedView,
                &[],
                GenerateRequest::Drop { cascade: false }
            )
            .as_deref(),
            Some("DROP MATERIALIZED VIEW shop.\"Orders\";")
        );
        assert_eq!(
            gen_(ObjectKind::Table, &[], GenerateRequest::Truncate).as_deref(),
            Some("TRUNCATE shop.\"Orders\";")
        );
        assert!(
            gen_(
                ObjectKind::Table,
                &[],
                GenerateRequest::Rename {
                    new_name: "x".into()
                }
            )
            .is_none()
        );
        assert!(gen_(ObjectKind::Type, &[], GenerateRequest::Truncate).is_none());
    }
}
