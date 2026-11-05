//! The logic of `profile_table`: every column of a table described from a sample of its rows
//! (the app's profile, `platypus_local::table_profile`), as an agent may read it. A sensitive
//! column gives counts only, never a value of any kind.

use std::sync::Arc;

use arrow_array::RecordBatch;
use arrow_cast::display::FormatOptions;
use arrow_schema::Schema;
use futures::StreamExt;
use platypus_core::{Connection, DriverKind, Error, ExecOptions, ExecResult, ObjectKind};
use platypus_local::table_profile::{ColumnReport, TableProfile, profile_table};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::agent::{Agent, sample_sql};
use crate::compare::objects_of;
use crate::error::{AgentError, Result};
use crate::guard::{self, Asked};
use crate::masking::TableSensitivity;
use crate::query::{self, Limits, cut_value};
use crate::redact::Redactor;

/// Rows read for a profile when `sample_rows` is left out.
pub const DEFAULT_SAMPLE_ROWS: usize = 10_000;
/// The most rows a call may ask to profile.
pub const MAX_SAMPLE_ROWS: usize = 100_000;
/// Bytes of rows held for a profile; past it the sample ends.
const MEMORY_LIMIT: usize = 256 * 1024 * 1024;

/// The arguments of `profile_table`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProfileArgs {
    /// The connection's name, as `list_connections` shows it.
    pub connection: String,
    /// The table's schema (a database in MySQL and MongoDB, `main` in SQLite and DuckDB).
    pub schema: String,
    /// The table, view or collection.
    pub table: String,
    /// Rows to profile: the first ones the database reads (default 10000, up to 100000).
    pub sample_rows: Option<u64>,
    /// Bytes of output at most (default 32768, up to 1048576).
    pub max_bytes: Option<u64>,
    /// Seconds the read may take before it is cancelled (default 30, up to 300).
    pub timeout_seconds: Option<f64>,
}

/// Why the profile covers fewer rows than the table has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cut {
    Rows,
    Memory,
}

/// What a read of a table's sample found.
enum Read {
    NoSchema,
    NoTable,
    Found(Box<Sample>),
}

/// The profile of a sample.
struct Sample {
    profile: TableProfile,
    cut: Option<Cut>,
}

/// Whether `kind` is something with rows to profile.
const fn has_rows(kind: ObjectKind) -> bool {
    matches!(
        kind,
        ObjectKind::Table
            | ObjectKind::View
            | ObjectKind::MaterializedView
            | ObjectKind::ForeignTable
            | ObjectKind::Dictionary
    )
}

/// The batches of the first `rows` rows of `stream` (up to `memory` bytes), and what cut it.
async fn collect(
    mut stream: platypus_core::RecordBatchStream,
    rows: usize,
    memory: usize,
) -> Result<(Vec<RecordBatch>, Option<Cut>), Error> {
    let mut batches = Vec::new();
    let (mut count, mut bytes) = (0, 0);
    while let Some(batch) = stream.next().await {
        let batch = batch?;
        let room = rows - count;
        if batch.num_rows() > room {
            batches.push(batch.slice(0, room));
            return Ok((batches, Some(Cut::Rows)));
        }
        count += batch.num_rows();
        bytes += batch.get_array_memory_size();
        batches.push(batch);
        if bytes > memory {
            return Ok((batches, Some(Cut::Memory)));
        }
    }
    Ok((batches, None))
}

/// Profiles the first `rows` rows of `schema.table`, if it is a table or view of the connection.
async fn read_sample(
    conn: &dyn Connection,
    driver: DriverKind,
    (schema, table): (&str, &str),
    rows: usize,
) -> Result<Read, Error> {
    let Some(objects) = objects_of(conn, schema).await? else {
        return Ok(Read::NoSchema);
    };
    // As the catalog writes it: the exact name, else one that differs only in case.
    let found = objects
        .iter()
        .filter(|o| has_rows(o.kind))
        .find(|o| o.name == table)
        .or_else(|| {
            objects
                .iter()
                .filter(|o| has_rows(o.kind))
                .find(|o| o.name.eq_ignore_ascii_case(table))
        });
    let Some(found) = found else {
        return Ok(Read::NoTable);
    };
    let options = ExecOptions {
        paged: true,
        ..ExecOptions::default()
    };
    // One row more than asked tells a table of exactly `rows` rows from a longer one.
    let sql = sample_sql(driver, schema, &found.name, rows as u64 + 1);
    let ExecResult::Rows(stream) = conn.execute(&sql, &options).await? else {
        return Err(Error::Other(format!("{} returned no columns", found.name)));
    };
    let columns: Arc<Schema> = stream.schema();
    let (batches, cut) = collect(stream, rows, MEMORY_LIMIT).await?;
    let profile = tokio::task::spawn_blocking(move || {
        profile_table(&columns, &batches, &FormatOptions::default())
            .map_err(|e| Error::Other(e.to_string()))
    })
    .await
    .map_err(|e| Error::Other(e.to_string()))??;
    Ok(Read::Found(Box::new(Sample { profile, cut })))
}

/// `value` with every text redacted and cut as results are.
fn clean(value: &mut Value, redactor: &Redactor) {
    match value {
        Value::String(text) => {
            let mut cut = false;
            *text = cut_value(redactor.apply(text), &mut cut);
        }
        Value::Array(items) => items.iter_mut().for_each(|v| clean(v, redactor)),
        Value::Object(map) => map.values_mut().for_each(|v| clean(v, redactor)),
        _ => {}
    }
}

/// A column of the profile as the agent reads it. A `sensitive` column gives its counts and
/// nothing else: no minimum, maximum, top value, pattern, length or outlier, which would all
/// show something of the values.
fn column_json(report: &ColumnReport, sensitive: bool, redactor: &Redactor) -> Value {
    let p = &report.profile;
    let mut out = Map::new();
    out.insert("name".into(), json!(report.name));
    out.insert("data_type".into(), json!(report.data_type));
    out.insert("db_type".into(), json!(report.db_type));
    out.insert("kind".into(), json!(report.kind));
    out.insert("nulls".into(), json!(p.nulls));
    out.insert("distinct".into(), json!(p.distinct));
    out.insert("empty".into(), json!(report.empty));
    if sensitive {
        out.insert("sensitive".into(), json!(true));
        return Value::Object(out);
    }
    let mut values = Map::new();
    values.insert("min".into(), json!(p.min));
    values.insert("max".into(), json!(p.max));
    values.insert("top".into(), json!(p.top));
    values.insert("numbers".into(), json!(report.numbers));
    values.insert("lengths".into(), json!(report.lengths));
    values.insert("patterns".into(), json!(report.patterns));
    // What is not there is left out: an answer is short.
    values.retain(|_, v| !v.is_null() && !v.as_array().is_some_and(Vec::is_empty));
    let mut values = Value::Object(values);
    clean(&mut values, redactor);
    if let Value::Object(values) = values {
        out.extend(values);
    }
    Value::Object(out)
}

/// The answer as JSON text within `max_bytes`: columns from the end are left out, with a note,
/// until it fits.
fn render(table: &str, sample: &Sample, columns: &[Value], max_bytes: usize) -> String {
    let build = |shown: usize| {
        let mut answer = json!({
            "table": table,
            "rows_profiled": sample.profile.rows,
            "sample_complete": sample.cut.is_none(),
            "columns": &columns[..shown],
        });
        match sample.cut {
            Some(Cut::Rows) => {
                answer["sample_note"] = json!(
                    "the table has more rows than were read: this is the profile of the first \
                     ones the database returned (no order is implied), not of the whole table"
                );
            }
            Some(Cut::Memory) => {
                answer["sample_note"] = json!(
                    "reading stopped at the memory limit: this is the profile of the rows read so far"
                );
            }
            None => {}
        }
        if columns.iter().any(|c| c.get("sensitive").is_some()) {
            answer["sensitive_note"] = json!(
                "columns marked sensitive show counts only (nulls, distinct, empty): no value of any kind"
            );
        }
        if shown < columns.len() {
            answer["columns_omitted"] = json!(format!(
                "only the first {shown} of {} columns fit in max_bytes: raise it (up to {})",
                columns.len(),
                Limits::MAX_BYTES
            ));
        }
        answer.to_string()
    };
    let mut shown = columns.len();
    let mut text = build(shown);
    while text.len() > max_bytes && shown > 0 {
        shown -= 1;
        text = build(shown);
    }
    text
}

impl Agent {
    /// The profile of the first `sample_rows` rows of a table, view or collection: per column
    /// NULLs, distinct values, minimum and maximum, top values and patterns. The rows are read
    /// in the database's read-only transaction, within the connection's agent limits. A
    /// column that is sensitive (as for results, the marks on this table counting) gives its
    /// counts only. If the marks cannot be read, the call fails.
    pub async fn profile_table_until(
        &self,
        args: &ProfileArgs,
        stop: impl Future<Output = ()>,
    ) -> Result<String> {
        if args.schema.trim().is_empty() || args.table.trim().is_empty() {
            return Err(AgentError::Invalid(
                "schema and table are both needed".to_owned(),
            ));
        }
        let rows = match args.sample_rows {
            None => DEFAULT_SAMPLE_ROWS,
            Some(0) => {
                return Err(AgentError::Invalid(
                    "sample_rows must be greater than 0".to_owned(),
                ));
            }
            Some(n) => usize::try_from(n)
                .unwrap_or(usize::MAX)
                .min(MAX_SAMPLE_ROWS),
        };
        let asked = Limits::new(None, args.max_bytes, args.timeout_seconds)?;
        let (store, saved) = self.find(&args.connection)?;
        let requested = Asked {
            max_bytes: args.max_bytes,
            timeout_seconds: args.timeout_seconds,
            ..Asked::default()
        };
        let (limits, _) = guard::clamp(asked, &requested, &saved.agent_limits);
        // Its figures show the rows: no more of them than the connection gives agents.
        let rows = saved
            .agent_limits
            .max_rows
            .map_or(rows, |max| rows.min(usize::try_from(max).unwrap_or(rows)));
        // Before anything is read: marks that cannot be read would let values through.
        let sensitivity = self.table_sensitivity(&saved, &args.schema, &args.table)?;
        self.windows.count(&saved)?;
        let lease = self.lease(&store, &saved).await?;
        let work = read_sample(
            lease.conn(),
            saved.driver,
            (&args.schema, &args.table),
            rows,
        );
        let read =
            query::guarded(lease.conn(), Some(saved.driver), limits.timeout, stop, work).await;
        let redactor = lease.redactor().clone();
        let read = lease.finish(read, limits.timeout)?;
        let sample = match read {
            Read::NoSchema => {
                return Err(AgentError::NotFound(format!(
                    "`{}` has no schema `{}`",
                    saved.name, args.schema
                )));
            }
            Read::NoTable => {
                return Err(AgentError::NotFound(format!(
                    "`{}` has no table or view `{}.{}`",
                    saved.name, args.schema, args.table
                )));
            }
            Read::Found(sample) => sample,
        };
        let columns = columns_of(&sample.profile, &sensitivity, &redactor);
        let name = format!("{}.{}", args.schema, args.table);
        Ok(render(&name, &sample, &columns, limits.max_bytes))
    }
}

/// The columns of `profile` as the agent reads them.
fn columns_of(
    profile: &TableProfile,
    sensitivity: &TableSensitivity,
    redactor: &Redactor,
) -> Vec<Value> {
    profile
        .columns
        .iter()
        .map(|c| {
            let db_type = c.db_type.as_deref().unwrap_or(&c.data_type);
            column_json(c, sensitivity.is_sensitive(&c.name, db_type), redactor)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use arrow_array::{ArrayRef, Int32Array, StringArray};
    use arrow_schema::{DataType, Field};

    use super::*;

    fn profile_of(secret: &[&str]) -> TableProfile {
        let schema = Schema::new(vec![
            Field::new("email", DataType::Utf8, true),
            Field::new("age", DataType::Int32, true),
        ]);
        let emails: ArrayRef = Arc::new(StringArray::from(
            secret.iter().map(|s| Some(*s)).collect::<Vec<_>>(),
        ));
        let ages: ArrayRef = Arc::new(Int32Array::from(
            (0..i32::try_from(secret.len()).unwrap()).collect::<Vec<_>>(),
        ));
        let batch = RecordBatch::try_new(Arc::new(schema.clone()), vec![emails, ages]).unwrap();
        profile_table(&schema, &[batch], &FormatOptions::default()).unwrap()
    }

    #[test]
    fn a_sensitive_column_gives_counts_only() {
        let profile = profile_of(&["ann@example.com", "bob@example.com", "ann@example.com"]);
        let json = column_json(&profile.columns[0], true, &Redactor::default()).to_string();
        assert!(json.contains("\"distinct\":2"));
        assert!(json.contains("\"sensitive\":true"));
        assert!(!json.contains("example.com"));
        assert!(!json.contains("ann"));
        for key in ["min", "max", "top", "patterns", "lengths"] {
            assert!(!json.contains(&format!("\"{key}\"")), "{key} leaked");
        }
    }

    #[test]
    fn a_plain_column_gives_its_values() {
        let profile = profile_of(&["ann@example.com", "bob@example.com"]);
        let json = column_json(&profile.columns[0], false, &Redactor::default()).to_string();
        assert!(json.contains("ann@example.com"));
        assert!(json.contains("\"top\""));
    }

    #[test]
    fn values_are_redacted_and_cut() {
        let long = "x".repeat(MAX_VALUE_TEST * 2);
        let profile = profile_of(&["hunter2-secret", &long]);
        let mut redactor = Redactor::default();
        redactor.add("hunter2-secret");
        let json = column_json(&profile.columns[0], false, &redactor).to_string();
        assert!(!json.contains("hunter2-secret"));
        assert!(json.contains("***"));
        assert!(!json.contains(&long));
    }

    const MAX_VALUE_TEST: usize = crate::query::MAX_VALUE_CHARS;

    #[test]
    fn columns_are_dropped_from_the_end_to_fit() {
        let profile = profile_of(&["a", "b"]);
        let sample = Sample {
            profile,
            cut: Some(Cut::Rows),
        };
        let columns: Vec<Value> = (0..50)
            .map(|i| json!({ "name": format!("column_{i}"), "nulls": 0 }))
            .collect();
        let text = render("main.t", &sample, &columns, 1500);
        assert!(text.len() <= 1500);
        let out: Value = serde_json::from_str(&text).unwrap();
        assert!(out["columns"].as_array().unwrap().len() < 50);
        assert!(out["columns_omitted"].is_string());
        assert!(out["sample_note"].is_string());
        assert_eq!(out["sample_complete"], false);
    }

    #[test]
    fn rows_beyond_the_sample_are_cut() {
        let batch = |n: i32| {
            let schema = Arc::new(Schema::new(vec![Field::new("n", DataType::Int32, false)]));
            RecordBatch::try_new(schema, vec![Arc::new(Int32Array::from_iter_values(0..n))])
                .unwrap()
        };
        let read = |sizes: &[i32], rows: usize| {
            let items: Vec<platypus_core::Result<RecordBatch>> =
                sizes.iter().map(|&n| Ok(batch(n))).collect();
            let schema = batch(1).schema();
            let stream =
                platypus_core::RecordBatchStream::new(schema, futures::stream::iter(items).boxed());
            let (batches, cut) =
                futures::executor::block_on(collect(stream, rows, usize::MAX)).unwrap();
            (
                batches.iter().map(RecordBatch::num_rows).sum::<usize>(),
                cut,
            )
        };
        assert_eq!(read(&[3, 4], 10), (7, None));
        assert_eq!(read(&[3, 4], 5), (5, Some(Cut::Rows)));
    }
}
