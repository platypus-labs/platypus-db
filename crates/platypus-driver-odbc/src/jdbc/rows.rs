//! The rows the bridge sends, as Arrow batches. The bridge reads every column as one of four
//! kinds: booleans, integers and floating point numbers keep their type, and the rest (decimals,
//! dates, text, binary as hex) is text, so a value is never rounded on the way.

use std::sync::Arc;

use arrow_array::{ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use platypus_core::{Error, Result};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Int,
    Double,
    Text,
}

impl Kind {
    fn of(name: Option<&str>) -> Self {
        match name {
            Some("bool") => Self::Bool,
            Some("int") => Self::Int,
            Some("double") => Self::Double,
            _ => Self::Text,
        }
    }

    fn data_type(self) -> DataType {
        match self {
            Self::Bool => DataType::Boolean,
            Self::Int => DataType::Int64,
            Self::Double => DataType::Float64,
            Self::Text => DataType::Utf8,
        }
    }
}

/// The schema of a result from the bridge's `columns`, and the kind of each column.
pub fn schema(columns: &[Value]) -> (SchemaRef, Vec<Kind>) {
    let kinds: Vec<Kind> = columns
        .iter()
        .map(|c| Kind::of(c["kind"].as_str()))
        .collect();
    let fields: Vec<Field> = columns
        .iter()
        .zip(&kinds)
        .map(|(c, kind)| {
            Field::new(
                c["name"].as_str().unwrap_or_default(),
                kind.data_type(),
                true,
            )
        })
        .collect();
    (Arc::new(Schema::new(fields)), kinds)
}

/// A floating point value: a number, or the text the bridge sends for `NaN` and the infinities.
fn number(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.parse().ok())
}

/// The batch of `rows` (arrays of values, one per column).
pub fn batch(schema: &SchemaRef, kinds: &[Kind], rows: &[Value]) -> Result<RecordBatch> {
    let columns: Vec<ArrayRef> = kinds
        .iter()
        .enumerate()
        .map(|(i, kind)| -> ArrayRef {
            match kind {
                Kind::Bool => Arc::new(
                    rows.iter()
                        .map(|r| r[i].as_bool())
                        .collect::<BooleanArray>(),
                ),
                Kind::Int => Arc::new(rows.iter().map(|r| r[i].as_i64()).collect::<Int64Array>()),
                Kind::Double => {
                    Arc::new(rows.iter().map(|r| number(&r[i])).collect::<Float64Array>())
                }
                Kind::Text => Arc::new(rows.iter().map(|r| r[i].as_str()).collect::<StringArray>()),
            }
        })
        .collect();
    RecordBatch::try_new(Arc::clone(schema), columns).map_err(Error::Arrow)
}

#[cfg(test)]
mod tests {
    use arrow_array::Array as _;
    use serde_json::json;

    use super::*;

    #[test]
    fn rows_become_typed_columns_with_nulls() {
        let columns = json!([
            {"name": "A", "kind": "int"},
            {"name": "B", "kind": "text"},
            {"name": "C", "kind": "double"},
            {"name": "D", "kind": "bool"},
            {"name": "E", "kind": "text"}
        ]);
        let (schema, kinds) = schema(columns.as_array().unwrap());
        assert_eq!(schema.field(0).data_type(), &DataType::Int64);
        assert_eq!(schema.field(4).data_type(), &DataType::Utf8);
        let rows = json!([[1, "x", 1.5, true, "12.34"], [2, null, "NaN", null, null]]);
        let batch = batch(&schema, &kinds, rows.as_array().unwrap()).unwrap();
        assert_eq!(batch.num_rows(), 2);
        assert!(batch.column(1).is_null(1));
        assert!(batch.column(3).is_null(1));
        let doubles = batch
            .column(2)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!(doubles.value(1).is_nan());
        assert!((doubles.value(0) - 1.5).abs() < f64::EPSILON);
    }
}
