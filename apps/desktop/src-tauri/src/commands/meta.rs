//! Metadata for the navigator and the editor's autocompletion. Navigator sessions are
//! separate from editor sessions, so browsing never waits behind a running query.

use std::collections::BTreeMap;

use platypus_core::{ObjectMeta, SchemaMeta, TableDetails};
use serde::Serialize;
use tauri::State;

use super::AppState;
use crate::error::CommandResult;

/// Tables and columns for editor autocompletion: `schemas[schema][table]` lists the
/// columns in declaration order.
#[derive(Debug, Serialize)]
pub struct CompletionCatalog {
    default_schema: Option<String>,
    schemas: BTreeMap<String, BTreeMap<String, Vec<String>>>,
}

#[tauri::command]
pub async fn completion_catalog(
    state: State<'_, AppState>,
    session_id: u64,
) -> CommandResult<CompletionCatalog> {
    let conn = state.live(session_id).await?;
    let default_schema = conn
        .schemas()
        .await?
        .into_iter()
        .find(|s| s.current)
        .map(|s| s.name);
    let mut schemas: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    for c in conn.catalog_columns().await? {
        schemas
            .entry(c.schema)
            .or_default()
            .entry(c.table)
            .or_default()
            .push(c.column);
    }
    Ok(CompletionCatalog {
        default_schema,
        schemas,
    })
}

#[tauri::command]
pub async fn list_schemas(
    state: State<'_, AppState>,
    session_id: u64,
) -> CommandResult<Vec<SchemaMeta>> {
    let conn = state.live(session_id).await?;
    Ok(conn.schemas().await?)
}

#[tauri::command]
pub async fn list_objects(
    state: State<'_, AppState>,
    session_id: u64,
    schema: String,
) -> CommandResult<Vec<ObjectMeta>> {
    let conn = state.live(session_id).await?;
    Ok(conn.objects(&schema).await?)
}

#[tauri::command]
pub async fn table_details(
    state: State<'_, AppState>,
    session_id: u64,
    schema: String,
    table: String,
) -> CommandResult<TableDetails> {
    let conn = state.live(session_id).await?;
    Ok(conn.table_details(&schema, &table).await?)
}

/// The first value of a small query, as text (`None` for NULL or no rows); used for row
/// counts. Run on the navigator session, which is never held by a paused result.
#[tauri::command]
pub async fn query_value(
    state: State<'_, AppState>,
    session_id: u64,
    sql: String,
) -> CommandResult<Option<String>> {
    let conn = state.live(session_id).await?;
    let rows = platypus_core::query_text(conn.as_ref(), &sql).await?;
    Ok(rows
        .into_iter()
        .next()
        .and_then(|r| r.into_iter().next().flatten()))
}
