//! Replies as rows. A single value is one row of a `value` column; an array a row per item
//! (`index`, `value`), a set a row per member, a map a row per field (`field`, `value`).
//! Nested replies arrive as JSON text with the database type `mixed`, which the grid's
//! document panel unfolds as a tree. Values that are not UTF-8 show quoted with escapes
//! ([`crate::command::display`]).

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::{ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use futures::stream::{self, StreamExt};
use platypus_core::{Error, ExecResult, META_DB_TYPE, RecordBatchStream, Result};
use redis::Value;

use crate::command::display;

/// One column of rows built in memory.
#[derive(Debug)]
pub enum Column {
    Int(Vec<Option<i64>>),
    Float(Vec<Option<f64>>),
    Bool(Vec<Option<bool>>),
    Text(Vec<Option<String>>),
}

impl Column {
    fn data_type(&self) -> DataType {
        match self {
            Self::Int(_) => DataType::Int64,
            Self::Float(_) => DataType::Float64,
            Self::Bool(_) => DataType::Boolean,
            Self::Text(_) => DataType::Utf8,
        }
    }

    fn array(self) -> ArrayRef {
        match self {
            Self::Int(v) => Arc::new(Int64Array::from(v)),
            Self::Float(v) => Arc::new(Float64Array::from(v)),
            Self::Bool(v) => Arc::new(BooleanArray::from(v)),
            Self::Text(v) => Arc::new(StringArray::from(v)),
        }
    }
}

/// A field of `kind` with the database type the grid shows.
#[must_use]
pub fn field(name: &str, kind: DataType, db_type: &str) -> Field {
    Field::new(name, kind, true).with_metadata(HashMap::from([(
        META_DB_TYPE.to_owned(),
        db_type.to_owned(),
    )]))
}

/// The schema of `columns`, as `(name, column, db_type)`.
fn schema_of(columns: &[(&str, Column, &str)]) -> SchemaRef {
    Arc::new(Schema::new(
        columns
            .iter()
            .map(|(name, column, db_type)| field(name, column.data_type(), db_type))
            .collect::<Vec<_>>(),
    ))
}

/// A batch of `columns` of `schema`.
pub fn batch(schema: &SchemaRef, columns: Vec<Column>) -> Result<RecordBatch> {
    let arrays = columns.into_iter().map(Column::array).collect();
    Ok(RecordBatch::try_new(Arc::clone(schema), arrays)?)
}

/// Rows read at once, as batches of `size` rows.
#[must_use]
pub fn rows_result(schema: SchemaRef, batch: &RecordBatch, size: usize) -> ExecResult {
    let size = size.max(1);
    let rows = batch.num_rows();
    let batches: Vec<Result<RecordBatch>> = (0..rows.max(1))
        .step_by(size)
        .map(|offset| Ok(batch.slice(offset, size.min(rows - offset.min(rows)))))
        .collect();
    ExecResult::Rows(RecordBatchStream::new(
        schema,
        stream::iter(batches).boxed(),
    ))
}

/// A value whose items are values themselves.
const fn is_nested(value: &Value) -> bool {
    matches!(
        value,
        Value::Array(_) | Value::Set(_) | Value::Map(_) | Value::Push { .. }
    ) || matches!(value, Value::Attribute { data, .. } if is_nested(data))
}

/// The reply to a command as rows; `json` when its strings are JSON text (`JSON.GET`).
pub fn reply(value: Value, json: bool, size: usize) -> Result<ExecResult> {
    let value = crate::error::reply(unwrap_attribute(value))?;
    let columns = match value {
        Value::Map(pairs) => {
            let (fields, values): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
            let fields = Column::Text(fields.iter().map(|f| Some(text(f))).collect());
            let (values, db_type) = items(values, json);
            vec![("field", fields, "string"), ("value", values, db_type)]
        }
        Value::Array(list) | Value::Push { data: list, .. } => {
            let index = Column::Int((0..list.len()).map(|i| i64::try_from(i).ok()).collect());
            let (values, db_type) = items(list, json);
            vec![("index", index, "integer"), ("value", values, db_type)]
        }
        Value::Set(members) => {
            let (values, db_type) = items(members, json);
            vec![("member", values, db_type)]
        }
        scalar => {
            let (values, db_type) = items(vec![scalar], json);
            vec![("value", values, db_type)]
        }
    };
    let schema = schema_of(&columns);
    let batch = batch(&schema, columns.into_iter().map(|(_, c, _)| c).collect())?;
    Ok(rows_result(schema, &batch, size))
}

fn unwrap_attribute(value: Value) -> Value {
    match value {
        Value::Attribute { data, .. } => unwrap_attribute(*data),
        other => other,
    }
}

/// A column of `values` and its database type: numbers when they all are, else text
/// (nested values as JSON).
fn items(values: Vec<Value>, json: bool) -> (Column, &'static str) {
    let values: Vec<Value> = values.into_iter().map(unwrap_attribute).collect();
    let present = || values.iter().filter(|v| !matches!(v, Value::Nil));
    if !json && present().count() > 0 {
        if present().all(|v| matches!(v, Value::Int(_))) {
            let ints = values
                .iter()
                .map(|v| match v {
                    Value::Int(n) => Some(*n),
                    _ => None,
                })
                .collect();
            return (Column::Int(ints), "integer");
        }
        if present().all(|v| matches!(v, Value::Double(_))) {
            let floats = values
                .iter()
                .map(|v| match v {
                    Value::Double(n) => Some(*n),
                    _ => None,
                })
                .collect();
            return (Column::Float(floats), "double");
        }
        if present().all(|v| matches!(v, Value::Boolean(_))) {
            let bools = values
                .iter()
                .map(|v| match v {
                    Value::Boolean(b) => Some(*b),
                    _ => None,
                })
                .collect();
            return (Column::Bool(bools), "boolean");
        }
    }
    let nested = json || values.iter().any(is_nested);
    let texts = values
        .iter()
        .map(|v| match v {
            Value::Nil => None,
            v if is_nested(v) => Some(to_json(v).to_string()),
            v => Some(text(v)),
        })
        .collect();
    (Column::Text(texts), if nested { "mixed" } else { "string" })
}

/// A scalar as text.
#[must_use]
pub fn text(value: &Value) -> String {
    match value {
        Value::Nil => String::new(),
        Value::Int(n) => n.to_string(),
        Value::BulkString(b) => display(b),
        Value::SimpleString(s) => s.clone(),
        Value::Okay => "OK".to_owned(),
        Value::Double(d) => d.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::VerbatimString { text, .. } => text.clone(),
        Value::BigNumber(n) => String::from_utf8_lossy(n).into_owned(),
        Value::ServerError(e) => crate::error::server(e).to_string(),
        nested => to_json(nested).to_string(),
    }
}

/// A value as JSON: maps as objects (their keys as text), arrays and sets as arrays.
#[must_use]
pub fn to_json(value: &Value) -> serde_json::Value {
    use serde_json::Value as J;
    match value {
        Value::Nil => J::Null,
        Value::Int(n) => J::from(*n),
        Value::Double(d) => {
            serde_json::Number::from_f64(*d).map_or_else(|| J::String(d.to_string()), J::Number)
        }
        Value::Boolean(b) => J::Bool(*b),
        Value::Array(items) | Value::Set(items) | Value::Push { data: items, .. } => {
            J::Array(items.iter().map(to_json).collect())
        }
        Value::Map(pairs) => J::Object(pairs.iter().map(|(k, v)| (text(k), to_json(v))).collect()),
        Value::Attribute { data, .. } => to_json(data),
        scalar => J::String(text(scalar)),
    }
}

/// The number of rows a result of `affected` writes, for a write that answers a count.
#[must_use]
pub const fn count(value: &Value) -> Option<u64> {
    match value {
        Value::Int(n) if *n >= 0 => Some(n.unsigned_abs()),
        _ => None,
    }
}

/// An error for a reply that is not of the expected shape.
#[must_use]
pub fn unexpected(what: &str, value: &Value) -> Error {
    Error::Other(format!("unexpected reply to {what}: {}", text(value)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::Array;
    use futures::TryStreamExt;

    async fn rows(value: Value) -> (SchemaRef, Vec<RecordBatch>) {
        let ExecResult::Rows(stream) = reply(value, false, 2).unwrap() else {
            panic!("rows expected");
        };
        let schema = stream.schema();
        (schema, stream.try_collect().await.unwrap())
    }

    fn db_type(schema: &Schema, i: usize) -> &str {
        &schema.field(i).metadata()[META_DB_TYPE]
    }

    #[tokio::test]
    async fn scalars_are_one_row() {
        let (schema, batches) = rows(Value::Int(3)).await;
        assert_eq!(schema.field(0).data_type(), &DataType::Int64);
        assert_eq!(batches[0].num_rows(), 1);
        let (schema, batches) = rows(Value::Nil).await;
        assert_eq!(schema.field(0).data_type(), &DataType::Utf8);
        assert!(batches[0].column(0).is_null(0));
        let (_, batches) = rows(Value::BulkString(vec![0xff])).await;
        let col = batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(col.value(0), r#""\xff""#);
    }

    #[tokio::test]
    async fn arrays_are_a_row_per_item_in_batches() {
        let list = Value::Array(
            (0..5)
                .map(|i| Value::BulkString(format!("v{i}").into()))
                .collect(),
        );
        let (schema, batches) = rows(list).await;
        assert_eq!(schema.field(0).name(), "index");
        assert_eq!(batches.len(), 3);
        assert_eq!(batches.iter().map(RecordBatch::num_rows).sum::<usize>(), 5);
        let (_, batches) = rows(Value::Array(Vec::new())).await;
        assert_eq!(batches.iter().map(RecordBatch::num_rows).sum::<usize>(), 0);
    }

    #[tokio::test]
    async fn maps_and_nested_replies() {
        let map = Value::Map(vec![(
            Value::BulkString(b"f".to_vec()),
            Value::Array(vec![Value::Int(1), Value::Nil]),
        )]);
        let (schema, batches) = rows(map).await;
        assert_eq!(schema.field(0).name(), "field");
        assert_eq!(db_type(&schema, 1), "mixed");
        let col = batches[0]
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(col.value(0), "[1,null]");
        let (schema, _) = rows(Value::Set(vec![Value::BulkString(b"m".to_vec())])).await;
        assert_eq!(schema.field(0).name(), "member");
    }
}
