//! Metadata for the navigator and the editor's autocompletion. Navigator sessions are
//! separate from editor sessions, so browsing never waits behind a running query.

use std::collections::BTreeMap;

use platypus_core::{
    CatalogForeignKey, DriverKind, ObjectMeta, ReferencingKey, SchemaMeta, TableDetails,
};
use platypus_sql::Dialect;
use platypus_sql::generated::{self, Generated};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::AppState;
use crate::error::CommandResult;

/// Tables and columns for editor autocompletion: `schemas[schema][table]` lists the
/// columns in declaration order. `foreign_keys` complete JOIN conditions.
#[derive(Debug, Serialize)]
pub struct CompletionCatalog {
    default_schema: Option<String>,
    schemas: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    foreign_keys: Vec<CatalogForeignKey>,
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
    // Completion works without JOIN conditions when the keys cannot be read.
    let foreign_keys = conn.catalog_foreign_keys().await.unwrap_or_default();
    Ok(CompletionCatalog {
        default_schema,
        schemas,
        foreign_keys,
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

/// A column the database fills by itself, for the data editor.
#[derive(Debug, Serialize)]
pub struct GeneratedColumn {
    name: String,
    /// `numbered`, `identity` or `computed` (see [`Generated`]).
    kind: &'static str,
}

/// The columns of `schema.table` the database fills by itself: the data editor leaves them
/// out of what it writes. Empty when the catalog cannot tell, e.g. on an old server.
#[tauri::command]
pub async fn generated_columns(
    state: State<'_, AppState>,
    session_id: u64,
    schema: String,
    table: String,
) -> CommandResult<Vec<GeneratedColumn>> {
    let dialect = Dialect::from(state.session(session_id)?.config.driver);
    let conn = state.live(session_id).await?;
    let columns = generated::generated_columns(conn.as_ref(), dialect, &schema, &table)
        .await
        .unwrap_or_default();
    Ok(columns
        .into_iter()
        .map(|c| GeneratedColumn {
            name: c.name,
            kind: match c.generated {
                Generated::Numbered => "numbered",
                Generated::Identity => "identity",
                Generated::Computed => "computed",
            },
        })
        .collect())
}

/// The foreign keys of other tables that point at `schema.table`, for the References panel.
#[tauri::command]
pub async fn referencing_keys(
    state: State<'_, AppState>,
    session_id: u64,
    schema: String,
    table: String,
) -> CommandResult<Vec<ReferencingKey>> {
    let conn = state.live(session_id).await?;
    Ok(conn.referencing_keys(&schema, &table).await?)
}

/// About how many rows a table has, from the database's statistics; `None` when unknown.
/// Shown when a statement is about to change all of them.
#[tauri::command]
pub async fn estimate_rows(
    state: State<'_, AppState>,
    session_id: u64,
    schema: Option<String>,
    table: String,
) -> CommandResult<Option<u64>> {
    let driver = state.session(session_id)?.config.driver;
    let conn = state.live(session_id).await?;
    let sql = if driver == DriverKind::Mongo {
        let collection = platypus_driver_mongo::generate::collection_ref(schema.as_deref(), &table);
        format!("{collection}.estimatedDocumentCount()")
    } else {
        Dialect::from(driver).row_estimate_sql(schema.as_deref(), &table)
    };
    let rows = platypus_core::query_text(conn.as_ref(), &sql).await?;
    Ok(rows
        .into_iter()
        .next()
        .and_then(|r| r.into_iter().next().flatten())
        .and_then(|n| n.parse().ok()))
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

/// Rows read at most by [`lookup_values`].
const LOOKUP_LIMIT: usize = 5_000;

/// The table a foreign key points to: its key column and the column that describes a row.
#[derive(Debug, Deserialize)]
pub struct Lookup {
    schema: String,
    table: String,
    key: String,
    label: String,
}

/// `(key, label)` pairs of the table a foreign key points to, for the data editor's
/// dictionary: the labels of `values`, or with `None` the first rows ordered by label.
#[tauri::command]
pub async fn lookup_values(
    state: State<'_, AppState>,
    session_id: u64,
    lookup: Lookup,
    values: Option<Vec<String>>,
    limit: usize,
) -> CommandResult<Vec<(Option<String>, Option<String>)>> {
    let dialect = Dialect::from(state.session(session_id)?.config.driver);
    let conn = state.live(session_id).await?;
    let key_name = dialect.quote_ident(&lookup.key);
    let label_name = dialect.quote_ident(&lookup.label);
    let mut sql = format!(
        "SELECT {key_name}, {label_name} FROM {}",
        dialect.qualified(Some(&lookup.schema), &lookup.table)
    );
    let limit = limit.min(LOOKUP_LIMIT);
    match values {
        Some(values) if values.is_empty() => return Ok(Vec::new()),
        // Text literals compare with any key type: the databases convert them.
        Some(values) => {
            let list: Vec<String> = values
                .iter()
                .take(limit)
                .map(|v| dialect.quote_literal(v))
                .collect();
            sql = format!("{sql} WHERE {key_name} IN ({})", list.join(", "));
        }
        None => {
            sql = format!(
                "{sql} ORDER BY {label_name} {}",
                dialect.limit_clause(limit as u64)
            );
        }
    }
    let rows = platypus_core::query_text(conn.as_ref(), &sql).await?;
    Ok(rows
        .into_iter()
        .map(|mut r| {
            let label = r.pop().flatten();
            (r.pop().flatten(), label)
        })
        .collect())
}
