//! Running an agent's statement: inside the database's read-only transaction when the
//! connection is read-only for agents, within a row limit, a byte limit and a timeout.

use std::time::Duration;

use arrow_array::{Array, RecordBatch};
use arrow_cast::display::{ArrayFormatter, FormatOptions};
use arrow_schema::DataType;
use futures::StreamExt;
use platypus_core::{
    Connection, DriverKind, Error, ExecOptions, ExecResult, META_DB_TYPE, RecordBatchStream,
};
use platypus_sql::Dialect;

use crate::error::{AgentError, Result};

/// Longest value kept whole, in characters; longer ones are cut.
pub const MAX_VALUE_CHARS: usize = 1000;

/// How much of a result an agent gets back, and how long the statement may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_rows: usize,
    /// Bytes of the rendered result.
    pub max_bytes: usize,
    pub timeout: Duration,
}

impl Limits {
    pub const DEFAULT_ROWS: usize = 100;
    pub const MAX_ROWS: usize = 10_000;
    pub const DEFAULT_BYTES: usize = 32 * 1024;
    pub const MAX_BYTES: usize = 1024 * 1024;
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
    pub const MAX_TIMEOUT: Duration = Duration::from_secs(300);

    /// The defaults with what the caller asked for, each up to its cap.
    pub fn new(
        max_rows: Option<u64>,
        max_bytes: Option<u64>,
        timeout_seconds: Option<f64>,
    ) -> Result<Self> {
        let positive = |name: &str| AgentError::Invalid(format!("{name} must be greater than 0"));
        let capped = |value: Option<u64>, default: usize, cap: usize, name: &str| match value {
            None => Ok(default),
            Some(0) => Err(positive(name)),
            Some(n) => Ok(usize::try_from(n).unwrap_or(usize::MAX).min(cap)),
        };
        let timeout = match timeout_seconds {
            None => Self::DEFAULT_TIMEOUT,
            Some(s) if s.is_finite() && s > 0.0 => {
                Duration::from_secs_f64(s.min(Self::MAX_TIMEOUT.as_secs_f64()))
            }
            Some(_) => return Err(positive("timeout_seconds")),
        };
        Ok(Self {
            max_rows: capped(max_rows, Self::DEFAULT_ROWS, Self::MAX_ROWS, "max_rows")?,
            max_bytes: capped(max_bytes, Self::DEFAULT_BYTES, Self::MAX_BYTES, "max_bytes")?,
            timeout,
        })
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_rows: Self::DEFAULT_ROWS,
            max_bytes: Self::DEFAULT_BYTES,
            timeout: Self::DEFAULT_TIMEOUT,
        }
    }
}

/// One value of a result, typed as far as JSON goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Null,
    Bool(bool),
    /// A finite integer or float, as the database printed it.
    Number(String),
    Text(String),
}

impl Cell {
    /// The value as a text table shows it.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Null => "NULL",
            Self::Bool(true) => "true",
            Self::Bool(false) => "false",
            Self::Number(s) | Self::Text(s) => s,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    /// The database's type name, else the Arrow type.
    pub db_type: String,
}

/// Why a result has fewer rows than the statement returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    MaxRows,
    MaxBytes,
}

/// The rows an agent gets back.
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Cell>>,
    pub cut: Option<Cut>,
    /// Some values were longer than [`MAX_VALUE_CHARS`] and were cut.
    pub values_cut: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Rows(Table),
    Affected(u64),
}

/// Why a statement did not complete.
#[derive(Debug)]
pub enum RunError {
    Database(Error),
    /// The timeout ran out; the statement was cancelled.
    Timeout,
}

impl From<Error> for RunError {
    fn from(e: Error) -> Self {
        Self::Database(e)
    }
}

/// Runs a statement and drops its result.
async fn run_plain(conn: &dyn Connection, sql: &str) -> Result<(), Error> {
    if let ExecResult::Rows(stream) = conn.execute(sql, &ExecOptions::default()).await? {
        drop(stream);
    }
    Ok(())
}

/// The statements that open and close the read-only transaction of `driver`: its
/// dialect's ([`Dialect::read_only_begin`]), except on libSQL servers, which refuse
/// `PRAGMA query_only`: there a write is only rolled back. MongoDB has none: only the
/// statement check stops writes.
fn read_only_statements(driver: DriverKind) -> (&'static [&'static str], &'static [&'static str]) {
    match driver {
        DriverKind::Libsql => (&["BEGIN"], &["ROLLBACK"]),
        DriverKind::Mongo => (&[], &[]),
        _ => {
            let dialect = Dialect::from(driver);
            (dialect.read_only_begin(), dialect.read_only_end())
        }
    }
}

/// Whether the read-only transaction of `driver` refuses writes, rather than only rolling
/// them back afterwards.
#[must_use]
pub fn refuses_writes_when_read_only(driver: DriverKind) -> bool {
    !matches!(driver, DriverKind::Libsql | DriverKind::Mongo)
        && Dialect::from(driver).refuses_writes_when_read_only()
}

/// Runs `statement`, already checked. With `read_only`, it runs in that driver's
/// read-only transaction, always rolled back.
pub async fn run(
    conn: &dyn Connection,
    statement: &str,
    read_only: Option<DriverKind>,
    limits: &Limits,
) -> std::result::Result<Outcome, RunError> {
    let cancel = conn.cancel_handle();
    let (begin, end) = read_only.map_or((&[][..], &[][..]), read_only_statements);
    let work = async {
        for sql in begin {
            run_plain(conn, sql).await?;
        }
        execute(conn, statement, limits).await
    };
    let outcome = if let Ok(outcome) = tokio::time::timeout(limits.timeout, work).await {
        outcome.map_err(RunError::from)
    } else {
        let _ = cancel.cancel().await;
        Err(RunError::Timeout)
    };
    // Rolled back whatever happened; a failure here leaves a transaction open, which
    // makes the next call open a new connection.
    for sql in end {
        let _ = run_plain(conn, sql).await;
    }
    outcome
}

async fn execute(
    conn: &dyn Connection,
    statement: &str,
    limits: &Limits,
) -> Result<Outcome, Error> {
    let options = ExecOptions {
        // One more row than wanted tells whether the result has more.
        batch_size: (limits.max_rows + 1).min(ExecOptions::default().batch_size),
        paged: true,
    };
    match conn.execute(statement, &options).await? {
        ExecResult::Affected { rows } => Ok(Outcome::Affected(rows)),
        ExecResult::Rows(stream) => read(stream, limits).await.map(Outcome::Rows),
    }
}

/// Reads rows until the limits; dropping the stream then cancels the rest.
async fn read(mut stream: RecordBatchStream, limits: &Limits) -> Result<Table, Error> {
    let schema = stream.schema();
    let mut table = Table {
        columns: schema
            .fields()
            .iter()
            .map(|f| Column {
                name: f.name().clone(),
                db_type: f
                    .metadata()
                    .get(META_DB_TYPE)
                    .cloned()
                    .unwrap_or_else(|| f.data_type().to_string()),
            })
            .collect(),
        rows: Vec::new(),
        cut: None,
        values_cut: false,
    };
    // Every rendering takes at least the values' bytes: once they pass the limit, no more
    // rows can fit.
    let mut bytes = 0usize;
    let options = FormatOptions::default();
    'batches: while let Some(batch) = stream.next().await {
        let batch = batch?;
        let reader = BatchReader::new(&batch, &options)?;
        for row in 0..batch.num_rows() {
            if table.rows.len() == limits.max_rows {
                table.cut = Some(Cut::MaxRows);
                break 'batches;
            }
            let cells = reader.row(row, &mut table.values_cut);
            bytes += cells.iter().map(|c| c.text().len() + 1).sum::<usize>();
            if bytes > limits.max_bytes {
                table.cut = Some(Cut::MaxBytes);
                break 'batches;
            }
            table.rows.push(cells);
        }
    }
    Ok(table)
}

/// Turns the values of a batch into cells.
struct BatchReader<'a> {
    columns: Vec<(&'a DataType, &'a dyn Array, ArrayFormatter<'a>)>,
}

impl<'a> BatchReader<'a> {
    fn new(batch: &'a RecordBatch, options: &'a FormatOptions<'a>) -> Result<Self, Error> {
        let columns = batch
            .columns()
            .iter()
            .map(|c| {
                platypus_core::special::formatter(c.as_ref(), options)
                    .map(|f| (c.data_type(), c.as_ref(), f))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { columns })
    }

    fn row(&self, row: usize, values_cut: &mut bool) -> Vec<Cell> {
        self.columns
            .iter()
            .map(|(data_type, array, formatter)| {
                if array.is_null(row) {
                    return Cell::Null;
                }
                let text = formatter.value(row).to_string();
                match data_type {
                    DataType::Boolean => Cell::Bool(text == "true"),
                    t if t.is_integer() => Cell::Number(text),
                    t if t.is_floating() && text.parse::<f64>().is_ok_and(f64::is_finite) => {
                        Cell::Number(text)
                    }
                    _ => Cell::Text(cut_value(text, values_cut)),
                }
            })
            .collect()
    }
}

/// `text` up to [`MAX_VALUE_CHARS`] characters, with `…` when it was longer.
fn cut_value(text: String, cut: &mut bool) -> String {
    match text.char_indices().nth(MAX_VALUE_CHARS) {
        Some((end, _)) => {
            *cut = true;
            let mut short = text[..end].to_owned();
            short.push('…');
            short
        }
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_default_cap_and_refuse_zero() {
        assert_eq!(Limits::new(None, None, None).unwrap(), Limits::default());
        let big = Limits::new(Some(1 << 40), Some(1 << 40), Some(1e9)).unwrap();
        assert_eq!(big.max_rows, Limits::MAX_ROWS);
        assert_eq!(big.max_bytes, Limits::MAX_BYTES);
        assert_eq!(big.timeout, Limits::MAX_TIMEOUT);
        let small = Limits::new(Some(5), Some(100), Some(0.5)).unwrap();
        assert_eq!(
            (small.max_rows, small.max_bytes, small.timeout),
            (5, 100, Duration::from_millis(500))
        );
        assert!(Limits::new(Some(0), None, None).is_err());
        assert!(Limits::new(None, Some(0), None).is_err());
        assert!(Limits::new(None, None, Some(-1.0)).is_err());
        assert!(Limits::new(None, None, Some(f64::NAN)).is_err());
    }

    #[test]
    fn cuts_long_values_on_character_boundaries() {
        let mut cut = false;
        assert_eq!(cut_value("short".to_owned(), &mut cut), "short");
        assert!(!cut);
        let long = "è".repeat(MAX_VALUE_CHARS + 5);
        let short = cut_value(long, &mut cut);
        assert!(cut);
        assert_eq!(short.chars().count(), MAX_VALUE_CHARS + 1);
        assert!(short.ends_with('…'));
    }
}
