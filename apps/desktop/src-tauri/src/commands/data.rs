//! Editing table data in the data viewer. The UI keeps the pending changes; these commands
//! turn them into UPDATE, INSERT and DELETE statements for a preview, or run them all or
//! none on the session of the result being edited.

use arrow_array::RecordBatch;
use platypus_sql::dml::{self, ApplyError, Column, Value, ValueKind};
use platypus_sql::{Dialect, pg_literal};
use serde::{Deserialize, Serialize};
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
    fn as_value(&self) -> Value<'_> {
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

/// The pending changes to a result whose rows are a table's rows.
#[derive(Debug, Deserialize)]
pub struct Changes {
    table: TableRef,
    /// Result columns that identify a row: the primary key, a non-null unique key or a
    /// virtual key the user set.
    key: Vec<usize>,
    /// Result columns that are the table's own (not expressions), by which the undo script
    /// inserts deleted rows back.
    table_columns: Vec<usize>,
    /// Default expressions by result column, for dialects without a `DEFAULT` keyword.
    defaults: Vec<Option<String>>,
    updates: Vec<RowUpdate>,
    /// New rows, as the values the user set; other columns take their default.
    inserts: Vec<Vec<ColumnValue>>,
    /// Result rows to delete.
    deletes: Vec<usize>,
}

fn pairs(values: &[ColumnValue]) -> Vec<(usize, Value<'_>)> {
    values
        .iter()
        .map(|v| (v.column, v.value.as_value()))
        .collect()
}

fn borrowed(key: &[(usize, Option<String>)]) -> Vec<(usize, Option<&str>)> {
    key.iter().map(|(c, v)| (*c, v.as_deref())).collect()
}

/// A loaded value, as the statements that put it back write it.
fn loaded(text: &[Option<String>], column: usize) -> Value<'_> {
    text.get(column)
        .and_then(Option::as_deref)
        .map_or(Value::Null, Value::Text)
}

/// The key a statement's values give a row; `None` when one is left to the database.
fn key_in<'a>(
    key: &[usize],
    values: &[(usize, Value<'a>)],
) -> Option<Vec<(usize, Option<&'a str>)>> {
    key.iter()
        .map(|c| match values.iter().find(|(v, _)| v == c)?.1 {
            Value::Text(t) => Some((*c, Some(t))),
            Value::Null => Some((*c, None)),
            Value::Default => None,
        })
        .collect()
}

/// The full text of the buffered result rows `offset..offset + limit`.
pub(super) fn rows_text(
    job: &Job,
    offset: usize,
    limit: usize,
) -> CommandResult<Vec<Vec<Option<String>>>> {
    let (_, batches) = job
        .window(offset, limit)
        .ok_or_else(|| CommandError::new("not_ready", "query has no result set yet"))?;
    let mut rows = Vec::with_capacity(limit.min(batches.iter().map(RecordBatch::num_rows).sum()));
    for batch in &batches {
        display::rows_text(batch, &mut rows)?;
    }
    Ok(rows)
}

fn row_text(job: &Job, row: usize) -> CommandResult<Vec<Option<String>>> {
    rows_text(job, row, 1)?
        .pop()
        .ok_or_else(|| CommandError::not_found("row", row + 1))
}

/// What saving runs, and the script that puts the loaded rows back afterwards.
struct Script {
    statements: Vec<String>,
    /// Statements, and `--` comments for what no statement can undo.
    undo: Vec<String>,
}

impl Script {
    fn undo_text(&self) -> String {
        let mut text = String::new();
        for line in &self.undo {
            text.push_str(line);
            text.push_str(if line.starts_with("--") { "\n" } else { ";\n" });
        }
        text
    }
}

/// The statements for `changes`: deletes first, which frees unique values for the updates
/// and inserts that follow. The undo script does the opposite, in the opposite order.
fn script(state: &AppState, job: &Job, changes: &Changes) -> CommandResult<Script> {
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
    let table = dialect.qualified(Some(&changes.table.schema), &changes.table.name);
    let invalid = |e: dml::InvalidValue| CommandError::new("invalid_value", e.to_string());
    let key_of = |text: &[Option<String>]| -> CommandResult<Vec<(usize, Option<String>)>> {
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

    let count = changes.deletes.len() + changes.updates.len() + changes.inserts.len();
    let mut statements = Vec::with_capacity(count);
    let mut undo_inserts = Vec::with_capacity(changes.deletes.len());
    let mut undo_updates = Vec::with_capacity(changes.updates.len());
    let mut undo = Vec::with_capacity(count);
    for &row in &changes.deletes {
        let text = row_text(job, row)?;
        let key = key_of(&text)?;
        statements.push(dml::delete(dialect, &table, &columns, &borrowed(&key)).map_err(invalid)?);
        let values: Vec<_> = changes
            .table_columns
            .iter()
            .map(|&c| (c, loaded(&text, c)))
            .collect();
        undo_inserts.push(dml::insert(dialect, &table, &columns, &values).map_err(invalid)?);
    }
    for update in &changes.updates {
        let text = row_text(job, update.row)?;
        let key = key_of(&text)?;
        let set = pairs(&update.values);
        statements
            .push(dml::update(dialect, &table, &columns, &set, &borrowed(&key)).map_err(invalid)?);
        // The row is found again by its key as saved.
        let saved_key: Option<Vec<(usize, Option<&str>)>> = key
            .iter()
            .map(|(c, v)| match set.iter().find(|(s, _)| s == c) {
                Some((_, Value::Text(t))) => Some((*c, Some(*t))),
                Some((_, Value::Null)) => Some((*c, None)),
                Some((_, Value::Default)) => None,
                None => Some((*c, v.as_deref())),
            })
            .collect();
        let back: Vec<_> = set.iter().map(|&(c, _)| (c, loaded(&text, c))).collect();
        undo_updates.push(match saved_key {
            Some(k) => dml::update(dialect, &table, &columns, &back, &k).map_err(invalid)?,
            None => format!(
                "-- Row {}: its key was set to its default, restore the row by hand",
                update.row + 1
            ),
        });
    }
    for (n, values) in changes.inserts.iter().enumerate() {
        let set = pairs(values);
        statements.push(dml::insert(dialect, &table, &columns, &set).map_err(invalid)?);
        undo.push(match key_in(&changes.key, &set) {
            Some(k) => dml::delete(dialect, &table, &columns, &k).map_err(invalid)?,
            None => format!(
                "-- New row {}: its key was generated by the database, delete it by hand",
                n + 1
            ),
        });
    }
    undo.append(&mut undo_updates);
    undo.append(&mut undo_inserts);
    Ok(Script { statements, undo })
}

/// The SQL that saving `changes` would run.
#[tauri::command]
pub fn preview_changes(
    state: State<'_, AppState>,
    job_id: u64,
    changes: Changes,
) -> CommandResult<Vec<String>> {
    let job = state.job(job_id)?;
    Ok(script(&state, &job, &changes)?.statements)
}

#[derive(Debug, Serialize)]
pub struct Saved {
    statements: usize,
    /// SQL that puts the rows back as they were loaded.
    undo: String,
}

/// Runs the statements for `changes` on the result's session, all or none (see
/// [`dml::apply`]). Fetching of the session's unfinished results stops first: they hold the
/// session. The UI reloads the data afterwards.
#[tauri::command]
pub async fn save_changes(
    state: State<'_, AppState>,
    job_id: u64,
    changes: Changes,
) -> CommandResult<Saved> {
    let job = state.job(job_id)?;
    let script = script(&state, &job, &changes)?;
    let dialect = Dialect::from(state.session(job.session_id)?.config.driver);
    let conn = state.live(job.session_id).await?;
    stop_jobs(&state, job.session_id);
    dml::apply(conn.as_ref(), dialect, &script.statements)
        .await
        .map_err(|e| match e {
            ApplyError::Failed { index, error } => {
                CommandError::from(&error).context(format!("Statement {}", index + 1))
            }
            e @ ApplyError::RowCount { .. } => CommandError::new("row_count", e.to_string()),
        })?;
    Ok(Saved {
        statements: script.statements.len(),
        undo: script.undo_text(),
    })
}

/// The full values of loaded rows `offset..offset + limit`, as text (`None` for NULL): the
/// grid shows them cut to one line, editing and copying need them whole.
#[tauri::command]
pub fn row_values(
    state: State<'_, AppState>,
    job_id: u64,
    offset: usize,
    limit: usize,
) -> CommandResult<Vec<Vec<Option<String>>>> {
    rows_text(&*state.job(job_id)?, offset, limit)
}

/// The elements of a PostgreSQL array of the type `type_name` (its delimiter is `;` for
/// boxes), or, `composite`, the fields of a composite value, for the data editor's list;
/// `None` when the text is not one, to edit it as text.
#[tauri::command]
pub fn split_literal(composite: bool, type_name: &str, text: &str) -> Option<Vec<Option<String>>> {
    if composite {
        pg_literal::split_composite(text)
    } else {
        pg_literal::split_array(text, pg_literal::array_delimiter(type_name))
    }
}

/// The array (or composite) literal of the data editor's list.
#[tauri::command]
pub fn join_literal(composite: bool, type_name: &str, items: Vec<Option<String>>) -> String {
    if composite {
        pg_literal::join_composite(&items)
    } else {
        pg_literal::join_array(&items, pg_literal::array_delimiter(type_name))
    }
}
