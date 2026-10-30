//! SQL results: Elasticsearch's `POST /_sql?format=json` (`columns`, `rows`, `cursor`) and
//! `OpenSearch`'s `POST /_plugins/_sql?format=jdbc` (`schema`, `datarows`, `cursor`). The
//! first reply gives the columns; the cursor's next pages are fetched as the reader asks.

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::{
    ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch, RecordBatchOptions, StringArray,
    TimestampMicrosecondArray, UInt64Array,
};
use arrow_schema::{DataType, Field, Schema, SchemaRef, TimeUnit};
use futures::StreamExt;
use futures::stream;
use platypus_core::{Error, ExecOptions, ExecResult, META_DB_TYPE, RecordBatchStream, Result};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::doc::collect_cells;
use crate::http::Payload;
use crate::session::{Flavor, Shared};
use crate::target::Target;

const TIME_ZONE: &str = "+00:00";
/// Rows a cursor page holds at most.
const MAX_FETCH: usize = 10_000;

#[derive(Deserialize)]
struct ColumnDef {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    alias: Option<String>,
}

/// An SQL reply in either product's words.
#[derive(Deserialize)]
struct SqlReply {
    columns: Option<Vec<ColumnDef>>,
    schema: Option<Vec<ColumnDef>>,
    rows: Option<Vec<Vec<Value>>>,
    datarows: Option<Vec<Vec<Value>>>,
    cursor: Option<String>,
}

impl SqlReply {
    fn parse(body: &str) -> Result<Self> {
        serde_json::from_str(body).map_err(|e| Error::Other(format!("unexpected SQL reply: {e}")))
    }

    fn rows(&mut self) -> Vec<Vec<Value>> {
        self.rows
            .take()
            .or_else(|| self.datarows.take())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    Int,
    UInt,
    Float,
    Bool,
    Time,
    Text,
}

impl Cell {
    fn of(es_type: &str) -> Self {
        match es_type.to_ascii_lowercase().as_str() {
            "long" | "integer" | "int" | "short" | "byte" => Self::Int,
            "unsigned_long" => Self::UInt,
            "double" | "float" | "half_float" | "scaled_float" => Self::Float,
            "boolean" => Self::Bool,
            "date" | "datetime" | "timestamp" | "date_nanos" => Self::Time,
            _ => Self::Text,
        }
    }

    fn data_type(self) -> DataType {
        match self {
            Self::Int => DataType::Int64,
            Self::UInt => DataType::UInt64,
            Self::Float => DataType::Float64,
            Self::Bool => DataType::Boolean,
            Self::Time => DataType::Timestamp(TimeUnit::Microsecond, Some(TIME_ZONE.into())),
            Self::Text => DataType::Utf8,
        }
    }
}

/// The type the grid shows for a column: the server's, where objects and arrays are the
/// `document` and `array` the UI unfolds.
fn db_type(es_type: &str) -> String {
    match es_type {
        "object" | "nested" | "struct" | "geo_shape" | "shape" => "document".to_owned(),
        "array" => "array".to_owned(),
        other => other.to_owned(),
    }
}

/// The columns of an SQL result.
struct Columns {
    names: Vec<String>,
    cells: Vec<Cell>,
    schema: SchemaRef,
}

impl Columns {
    fn new(defs: &[ColumnDef]) -> Self {
        let names: Vec<String> = defs
            .iter()
            .map(|d| d.alias.clone().unwrap_or_else(|| d.name.clone()))
            .collect();
        let cells: Vec<Cell> = defs.iter().map(|d| Cell::of(&d.kind)).collect();
        let fields: Vec<Field> = defs
            .iter()
            .zip(&names)
            .zip(&cells)
            .map(|((def, name), cell)| {
                Field::new(name, cell.data_type(), true).with_metadata(HashMap::from([(
                    META_DB_TYPE.to_owned(),
                    db_type(&def.kind),
                )]))
            })
            .collect();
        Self {
            names,
            cells,
            schema: Arc::new(Schema::new(fields)),
        }
    }

    fn batch(&self, rows: &[Vec<Value>]) -> Result<RecordBatch> {
        let columns = (0..self.cells.len())
            .map(|c| self.column(c, rows))
            .collect::<Result<Vec<_>>>()?;
        Ok(RecordBatch::try_new_with_options(
            Arc::clone(&self.schema),
            columns,
            &RecordBatchOptions::new().with_row_count(Some(rows.len())),
        )?)
    }

    fn column(&self, c: usize, rows: &[Vec<Value>]) -> Result<ArrayRef> {
        let cells = rows.iter().map(|row| row.get(c).filter(|v| !v.is_null()));
        let cell = self.cells[c];
        let bad = |value: &Value| Error::Decode {
            column: self.names[c].clone(),
            db_type: format!("{cell:?}").to_ascii_lowercase(),
            message: format!(
                "unexpected value {}",
                value.to_string().chars().take(40).collect::<String>()
            ),
        };
        Ok(match cell {
            Cell::Int => Arc::new(collect_cells::<_, _, Int64Array>(cells, |v| {
                number(v, Value::as_i64, str::parse).ok_or_else(|| bad(v))
            })?),
            Cell::UInt => Arc::new(collect_cells::<_, _, UInt64Array>(cells, |v| {
                number(v, Value::as_u64, str::parse).ok_or_else(|| bad(v))
            })?),
            Cell::Float => Arc::new(collect_cells::<_, _, Float64Array>(cells, |v| {
                number(v, Value::as_f64, str::parse).ok_or_else(|| bad(v))
            })?),
            Cell::Bool => Arc::new(collect_cells::<_, _, BooleanArray>(cells, |v| {
                v.as_bool().ok_or_else(|| bad(v))
            })?),
            Cell::Time => Arc::new(
                collect_cells::<_, _, TimestampMicrosecondArray>(cells, |v| {
                    v.as_str().and_then(parse_micros).ok_or_else(|| bad(v))
                })?
                .with_timezone(TIME_ZONE),
            ),
            Cell::Text => Arc::new(
                cells
                    .map(|v| {
                        v.map(|v| match v {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                    })
                    .collect::<StringArray>(),
            ),
        })
    }
}

/// A number, from a JSON number or from the text of one (`"NaN"`, `"Infinity"`).
fn number<T>(
    value: &Value,
    native: fn(&Value) -> Option<T>,
    text: fn(&str) -> std::result::Result<T, <T as std::str::FromStr>::Err>,
) -> Option<T>
where
    T: std::str::FromStr,
{
    match value {
        Value::String(s) => text(s).ok(),
        other => native(other),
    }
}

/// Microseconds since the epoch of `2024-05-06`, `2024-05-06T07:08:09.123Z`,
/// `2024-05-06 07:08:09+02:00`…
fn parse_micros(text: &str) -> Option<i64> {
    let text = text.trim();
    let (date, rest) = match text.find(['T', ' ']) {
        Some(i) => (&text[..i], &text[i + 1..]),
        None => (text, ""),
    };
    let mut parts = date.splitn(3, '-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (time, offset) = split_offset(rest)?;
    let mut seconds = days_from_civil(year, month, day) * 86_400 - offset;
    let mut micros = 0;
    if !time.is_empty() {
        let (clock, fraction) = time.split_once('.').unwrap_or((time, ""));
        let mut clock = clock.split(':');
        let hours: i64 = clock.next()?.parse().ok()?;
        let minutes: i64 = clock.next().unwrap_or("0").parse().ok()?;
        let secs: i64 = clock.next().unwrap_or("0").parse().ok()?;
        seconds += hours * 3600 + minutes * 60 + secs;
        let digits: String = fraction.chars().take(6).collect();
        if !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        micros = format!("{digits:0<6}").parse::<i64>().ok()?;
    }
    Some(seconds * 1_000_000 + micros)
}

/// The clock part and the UTC offset in seconds of `Z`, `+02:00`, `-0530` or nothing.
fn split_offset(rest: &str) -> Option<(&str, i64)> {
    if let Some(time) = rest.strip_suffix('Z') {
        return Some((time, 0));
    }
    let Some(at) = rest.rfind(['+', '-']).filter(|&i| i > 0) else {
        return Some((rest, 0));
    };
    let sign = if rest.as_bytes()[at] == b'-' { -1 } else { 1 };
    let digits: String = rest[at + 1..].chars().filter(|c| *c != ':').collect();
    if digits.len() < 2 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i64 = digits[..2].parse().ok()?;
    let minutes: i64 = digits.get(2..4).map_or(Some(0), |m| m.parse().ok())?;
    Some((&rest[..at], sign * (hours * 3600 + minutes * 60)))
}

/// Days from 1970-01-01 to a date of the proleptic Gregorian calendar.
const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Where the statements of a product go.
struct Endpoint {
    run: &'static str,
    close: &'static str,
}

const fn endpoint(flavor: Flavor) -> Endpoint {
    match flavor {
        Flavor::Elasticsearch => Endpoint {
            run: "/_sql?format=json",
            close: "/_sql/close",
        },
        Flavor::OpenSearch => Endpoint {
            run: "/_plugins/_sql?format=jdbc",
            close: "/_plugins/_sql/close",
        },
    }
}

/// Closes the cursor of a result the reader left before its end.
struct Cursor {
    shared: Arc<Shared>,
    cursor: Option<String>,
}

impl Drop for Cursor {
    fn drop(&mut self) {
        if let Some(cursor) = self.cursor.take() {
            let close = endpoint(self.shared.flavor).close;
            self.shared
                .later("POST", close, json!({ "cursor": cursor }).to_string());
        }
    }
}

struct State {
    columns: Arc<Columns>,
    /// The rows of the page read last, of which those from `next` on are still to give.
    rows: Vec<Vec<Value>>,
    next: usize,
    cursor: Cursor,
    size: usize,
}

/// Runs an SQL statement; its rows are read a page of the cursor at a time.
pub async fn run(shared: &Arc<Shared>, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
    let size = options.batch_size.max(1);
    // A reader that takes a page at a time gets that page per request.
    let fetch = if options.paged { size } else { size.max(1000) }.min(MAX_FETCH);
    let target = Target::new(endpoint(shared.flavor).run)?;
    let mut body = json!({ "query": sql, "fetch_size": fetch });
    if shared.flavor == Flavor::Elasticsearch {
        // An array in a column shows its first value instead of failing the statement.
        body["field_multi_value_leniency"] = json!(true);
    }
    let reply = shared
        .call("POST", &target, Payload::Json(&body.to_string()), Some(sql))
        .await?;
    let mut reply = SqlReply::parse(&reply.body)?;
    let Some(defs) = reply.columns.take().or_else(|| reply.schema.take()) else {
        return Err(Error::Other("the SQL reply has no columns".to_owned()));
    };
    let columns = Arc::new(Columns::new(&defs));
    let schema = Arc::clone(&columns.schema);
    let state = State {
        columns,
        rows: reply.rows(),
        next: 0,
        cursor: Cursor {
            shared: Arc::clone(shared),
            cursor: reply.cursor,
        },
        size,
    };
    let pages = stream::try_unfold(state, next_batch).boxed();
    Ok(ExecResult::Rows(RecordBatchStream::new(schema, pages)))
}

async fn next_batch(mut state: State) -> Result<Option<(RecordBatch, State)>> {
    loop {
        if state.next < state.rows.len() {
            let end = state.rows.len().min(state.next + state.size);
            let batch = state.columns.batch(&state.rows[state.next..end])?;
            state.next = end;
            return Ok(Some((batch, state)));
        }
        let Some(cursor) = state.cursor.cursor.clone() else {
            return Ok(None);
        };
        let shared = Arc::clone(&state.cursor.shared);
        let target = Target::new(endpoint(shared.flavor).run)?;
        let body = json!({ "cursor": cursor }).to_string();
        let reply = shared
            .call("POST", &target, Payload::Json(&body), None)
            .await?;
        let mut reply = SqlReply::parse(&reply.body)?;
        state.rows = reply.rows();
        state.next = 0;
        state.cursor.cursor = reply.cursor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_timestamps() {
        for (text, want) in [
            ("1970-01-01", Some(0)),
            ("1970-01-02T00:00:00Z", Some(86_400_000_000)),
            ("2024-05-06T07:08:09.123Z", Some(1_714_979_289_123_000)),
            ("2024-05-06 07:08:09.5", Some(1_714_979_289_500_000)),
            ("2024-05-06T09:08:09+02:00", Some(1_714_979_289_000_000)),
            ("2024-05-06T01:38:09-0530", Some(1_714_979_289_000_000)),
            (
                "2024-05-06T07:08:09.123456789Z",
                Some(1_714_979_289_123_456),
            ),
            ("1969-12-31T23:59:59Z", Some(-1_000_000)),
            ("not a date", None),
            ("2024-13-01", None),
        ] {
            assert_eq!(parse_micros(text), want, "{text}");
        }
    }

    #[test]
    fn builds_typed_columns() {
        let defs: Vec<ColumnDef> = serde_json::from_str(
            r#"[{"name":"n","type":"long"},{"name":"d","type":"double"},{"name":"b","type":"boolean"},
                {"name":"t","type":"datetime"},{"name":"k","type":"keyword"},{"name":"o","type":"object"}]"#,
        )
        .unwrap();
        let columns = Columns::new(&defs);
        let rows: Vec<Vec<Value>> = serde_json::from_str(
            r#"[[1,1.5,true,"2024-05-06T07:08:09.123Z","a",{"x":1}],[null,"NaN",null,null,null,null]]"#,
        )
        .unwrap();
        let batch = columns.batch(&rows).unwrap();
        assert_eq!(batch.num_rows(), 2);
        let schema = batch.schema();
        let types: Vec<_> = schema
            .fields()
            .iter()
            .map(|f| f.data_type().clone())
            .collect();
        assert_eq!(types[0], DataType::Int64);
        assert_eq!(
            types[3],
            DataType::Timestamp(TimeUnit::Microsecond, Some("+00:00".into()))
        );
        assert_eq!(schema.field(5).metadata()[META_DB_TYPE], "document");
        let doubles = batch
            .column(1)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!(doubles.value(1).is_nan());
        let bad: Vec<Vec<Value>> =
            serde_json::from_str(r#"[["x",1,true,null,null,null]]"#).unwrap();
        assert!(matches!(columns.batch(&bad), Err(Error::Decode { .. })));
    }

    #[test]
    fn reads_both_products_replies() {
        let mut es = SqlReply::parse(
            r#"{"columns":[{"name":"a","type":"long"}],"rows":[[1]],"cursor":"c"}"#,
        )
        .unwrap();
        assert_eq!(es.rows().len(), 1);
        assert_eq!(es.cursor.as_deref(), Some("c"));
        let mut os = SqlReply::parse(
            r#"{"schema":[{"name":"a","type":"integer","alias":"x"}],"datarows":[[1],[2]],"total":2,"size":2,"status":200}"#,
        )
        .unwrap();
        assert_eq!(os.rows().len(), 2);
        let columns = Columns::new(&os.schema.take().unwrap());
        assert_eq!(columns.names, ["x"]);
    }
}
