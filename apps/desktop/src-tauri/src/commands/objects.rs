//! Database objects for the navigator and the object tab: DDL, properties, generated SQL
//! and the statements that change objects (drop, rename, truncate).

use futures::TryStreamExt;
use platypus_core::{DriverKind, ExecOptions, ExecResult, ObjectKind, ObjectRef, Property};
use platypus_sql::{Dialect, ObjectName};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::AppState;
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
/// and discarding any rows. Returns the affected row count when there is one.
#[tauri::command]
pub async fn execute_statement(
    state: State<'_, AppState>,
    session_id: u64,
    sql: String,
) -> CommandResult<Option<u64>> {
    state.may_run(session_id, &sql)?;
    let conn = state.live(session_id).await?;
    match conn.execute(&sql, &ExecOptions::default()).await? {
        ExecResult::Affected { rows } => Ok(Some(rows)),
        ExecResult::Rows(stream) => {
            stream.try_for_each(|_| async { Ok(()) }).await?;
            Ok(None)
        }
    }
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
    let d = Dialect::from(driver);
    let name = ObjectName {
        schema: target.schema.as_deref(),
        name: &target.name,
        parent: target.parent.as_deref(),
    };
    let columns: Vec<&str> = target.columns.iter().map(String::as_str).collect();
    let keys: Vec<&str> = target.keys.iter().map(String::as_str).collect();
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

const ALL_KINDS: [ObjectKind; 14] = [
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
    }
}
