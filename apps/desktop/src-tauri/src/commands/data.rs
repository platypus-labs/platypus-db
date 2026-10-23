//! Editing table data in the data viewer. The UI keeps the pending changes; these commands
//! turn them into UPDATE, INSERT and DELETE statements for a preview, or run them all or
//! none on the session of the result being edited.

use platypus_sql::Dialect;
use platypus_sql::dml::{self, ApplyError, Column, Table, Value, ValueKind};
use serde::Deserialize;
use tauri::State;

use super::AppState;
use super::query::stop_jobs;
use crate::display;
use crate::error::{CommandError, CommandResult};
use crate::jobs::Job;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CellInput {
    Value { text: String },
    Null,
    Default,
}

impl CellInput {
    fn value(&self) -> Value<'_> {
        match self {
            Self::Value { text } => Value::Text(text),
            Self::Null => Value::Null,
            Self::Default => Value::Default,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ColumnValue {
    /// Index of the result column.
    column: usize,
    value: CellInput,
}

#[derive(Debug, Deserialize)]
pub struct RowUpdate {
    /// Index of the result row.
    row: usize,
    values: Vec<ColumnValue>,
}

#[derive(Debug, Deserialize)]
pub struct TableRef {
    schema: String,
    name: String,
}

/// The pending changes to a data viewer result, whose columns are the table's.
#[derive(Debug, Deserialize)]
pub struct Changes {
    table: TableRef,
    /// Result columns that identify a row: the primary key or a non-null unique key.
    key: Vec<usize>,
    /// Default expressions by result column, for dialects without a `DEFAULT` keyword.
    defaults: Vec<Option<String>>,
    updates: Vec<RowUpdate>,
    /// New rows, as the values the user set; other columns take their default.
    inserts: Vec<Vec<ColumnValue>>,
    /// Result rows to delete.
    deletes: Vec<usize>,
}

fn pairs(values: &[ColumnValue]) -> Vec<(usize, Value<'_>)> {
    values.iter().map(|v| (v.column, v.value())).collect()
}

fn borrowed(key: &[(usize, Option<String>)]) -> Vec<(usize, Option<&str>)> {
    key.iter().map(|(c, v)| (*c, v.as_deref())).collect()
}

/// The full text of a buffered result row.
fn row_text(job: &Job, row: usize) -> CommandResult<Vec<Option<String>>> {
    let (_, batches) = job
        .window(row, 1)
        .ok_or_else(|| CommandError::new("not_ready", "query has no result set yet"))?;
    let batch = batches
        .first()
        .ok_or_else(|| CommandError::not_found("row", row + 1))?;
    Ok(display::row_text(batch, 0)?)
}

/// The statements for `changes`: deletes first, which frees unique values for the updates
/// and inserts that follow.
fn statements(state: &AppState, job: &Job, changes: &Changes) -> CommandResult<Vec<String>> {
    let dialect = Dialect::from(state.session(job.session_id)?.config.driver);
    let (schema, _) = job
        .window(0, 0)
        .ok_or_else(|| CommandError::new("not_ready", "query has no result set yet"))?;
    let columns: Vec<Column<'_>> = schema
        .fields()
        .iter()
        .enumerate()
        .map(|(i, f)| Column {
            name: f.name(),
            kind: ValueKind::of(f.data_type()),
            default: changes.defaults.get(i).and_then(Option::as_deref),
        })
        .collect();
    let table = Table {
        schema: Some(&changes.table.schema),
        name: &changes.table.name,
    };
    let invalid = |e: dml::InvalidValue| CommandError::new("invalid_value", e.to_string());
    let key_of = |row: usize| -> CommandResult<Vec<(usize, Option<String>)>> {
        let text = row_text(job, row)?;
        changes
            .key
            .iter()
            .map(|&c| {
                let value = text
                    .get(c)
                    .ok_or_else(|| CommandError::not_found("column", c + 1))?;
                Ok((c, value.clone()))
            })
            .collect()
    };

    let mut out =
        Vec::with_capacity(changes.deletes.len() + changes.updates.len() + changes.inserts.len());
    for &row in &changes.deletes {
        let key = key_of(row)?;
        out.push(dml::delete(dialect, table, &columns, &borrowed(&key)).map_err(invalid)?);
    }
    for update in &changes.updates {
        let key = key_of(update.row)?;
        let set = pairs(&update.values);
        out.push(dml::update(dialect, table, &columns, &set, &borrowed(&key)).map_err(invalid)?);
    }
    for values in &changes.inserts {
        out.push(dml::insert(dialect, table, &columns, &pairs(values)).map_err(invalid)?);
    }
    Ok(out)
}

/// The SQL that saving `changes` would run.
#[tauri::command]
pub fn preview_changes(
    state: State<'_, AppState>,
    job_id: u64,
    changes: Changes,
) -> CommandResult<Vec<String>> {
    let job = state.job(job_id)?;
    statements(&state, &job, &changes)
}

/// Runs the statements for `changes` on the result's session, all or none (see
/// [`dml::apply`]), and returns how many ran. Fetching of the session's unfinished results
/// stops first: they hold the session. The UI reloads the data afterwards.
#[tauri::command]
pub async fn save_changes(
    state: State<'_, AppState>,
    job_id: u64,
    changes: Changes,
) -> CommandResult<usize> {
    let job = state.job(job_id)?;
    let statements = statements(&state, &job, &changes)?;
    let conn = state.live(job.session_id).await?;
    stop_jobs(&state, job.session_id);
    dml::apply(conn.as_ref(), &statements)
        .await
        .map_err(|e| match e {
            ApplyError::Failed { index, error } => {
                CommandError::from(&error).context(format!("Statement {}", index + 1))
            }
            e @ ApplyError::RowCount { .. } => CommandError::new("row_count", e.to_string()),
        })?;
    Ok(statements.len())
}

/// The full values of a loaded row, as text (`None` for NULL): the grid shows them cut to
/// one line, the editor needs them whole.
#[tauri::command]
pub fn row_values(
    state: State<'_, AppState>,
    job_id: u64,
    row: usize,
) -> CommandResult<Vec<Option<String>>> {
    row_text(&state.job(job_id)?, row)
}
