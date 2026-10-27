//! Exports: a query re-run in streaming, or the rows a result has loaded.

use std::path::PathBuf;
use std::sync::Arc;

use std::collections::HashSet;

use platypus_transfer::ExportOptions;
use platypus_transfer::import::select_all;
use serde::Deserialize;
use tauri::State;
use tauri::ipc::Channel;

use super::{AppState, lock};
use crate::error::{CommandError, CommandResult};
use crate::export::{Export, ExportEvent};

/// Re-runs `sql` on a new connection and streams all its rows to `path`. Returns an id for
/// `cancel_export`; progress and the outcome arrive on `on_event`.
#[tauri::command]
pub fn export_query(
    state: State<'_, AppState>,
    session_id: u64,
    sql: String,
    path: PathBuf,
    options: ExportOptions,
    on_event: Channel<ExportEvent>,
) -> CommandResult<u64> {
    let session = state.session(session_id)?;
    let connection = (session.config, session.label, Arc::clone(&state.query_log));
    let export = Export::start(connection, sql, path, options, on_event);
    let id = state.next_id();
    lock(&state.exports).insert(id, export);
    Ok(id)
}

#[tauri::command]
pub fn cancel_export(state: State<'_, AppState>, export_id: u64) {
    if let Some(export) = lock(&state.exports).remove(&export_id) {
        export.cancel();
    }
}

/// Writes the rows a result has loaded so far; returns how many.
#[tauri::command]
pub async fn export_rows(
    state: State<'_, AppState>,
    job_id: u64,
    path: PathBuf,
    options: ExportOptions,
) -> CommandResult<u64> {
    let (schema, batches) = state
        .job(job_id)?
        .snapshot()
        .ok_or_else(|| CommandError::new("not_ready", "the result has no rows"))?;
    tokio::task::spawn_blocking(move || {
        crate::export::write_batches(&path, &schema, &batches, &options)
    })
    .await
    .map_err(|e| CommandError::new("export", e.to_string()))?
    .map_err(Into::into)
}

#[derive(Debug, Deserialize)]
pub struct TableRef {
    /// `None` for the session's current schema.
    schema: Option<String>,
    name: String,
}

/// `name` as a file name: the characters Windows refuses become `_`.
fn file_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if r#"<>:"/\|?*"#.contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// `name` made unique among `taken` (which it joins), ignoring case as Windows and macOS do:
/// `Mix` and `mix`, or `a/b` and `a_b`, would write one file. A suffix `_2`, `_3`… tells
/// the later ones apart.
fn unique_name(name: String, taken: &mut HashSet<String>) -> String {
    let mut candidate = name.clone();
    let mut n = 1;
    while !taken.insert(candidate.to_lowercase()) {
        n += 1;
        candidate = format!("{name}_{n}");
    }
    candidate
}

/// Exports every row of each table to `<dir>/<table>.<extension>`, one after the other on a
/// connection of its own: a MongoDB collection with `find`. Two tables whose file names
/// would be one get one each. Returns an id for `cancel_export`; progress counts the rows of
/// all.
#[tauri::command]
pub fn export_tables(
    state: State<'_, AppState>,
    session_id: u64,
    tables: Vec<TableRef>,
    dir: PathBuf,
    options: ExportOptions,
    on_event: Channel<ExportEvent>,
) -> CommandResult<u64> {
    let session = state.session(session_id)?;
    let driver = session.config.driver;
    let mut taken = HashSet::new();
    let files = tables
        .into_iter()
        .map(|t| {
            let sql = select_all(driver, (t.schema.as_deref(), &t.name), None);
            let path = dir.join(format!(
                "{}.{}{}",
                unique_name(file_name(&t.name), &mut taken),
                options.format.extension(),
                options.compression.suffix()
            ));
            // One signed URL names one file.
            let options = ExportOptions {
                table: t.name,
                driver: Some(session.config.driver),
                upload_url: None,
                ..options.clone()
            };
            (sql, path, options)
        })
        .collect();
    let connection = (session.config, session.label, Arc::clone(&state.query_log));
    let export = Export::start_many(connection, files, on_event);
    let id = state.next_id();
    lock(&state.exports).insert(id, export);
    Ok(id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_names_of_tables() {
        assert_eq!(super::file_name("a/b:c"), "a_b_c");
        assert_eq!(super::file_name("Orders 2024"), "Orders 2024");
        let mut taken = std::collections::HashSet::new();
        let names: Vec<_> = ["Mix", "mix", "a_b", "a/b", "MIX"]
            .into_iter()
            .map(|n| super::unique_name(super::file_name(n), &mut taken))
            .collect();
        assert_eq!(names, ["Mix", "mix_2", "a_b", "a_b_2", "MIX_3"]);
    }
}
