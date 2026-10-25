//! Documents to Arrow: one column per top-level field of the first documents of a result.
//!
//! A field whose values share a type gets the matching Arrow type; nested documents and
//! arrays become relaxed Extended JSON text, and fields of mixed types text too. The
//! column's [`META_DB_TYPE`] says which BSON type it holds (`document` and `array` for the
//! JSON columns), so the UI can show those as a tree.

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::builder::{
    BinaryBuilder, BooleanBuilder, Float64Builder, Int32Builder, Int64Builder, StringBuilder,
    TimestampMillisecondBuilder,
};
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{DataType, Field, Schema, SchemaRef, TimeUnit};
use mongodb::bson::{Bson, Document};
use platypus_core::{META_DB_TYPE, Result};

/// The BSON type of a column, as far as the Arrow type goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bool,
    Int32,
    Int64,
    Double,
    String,
    ObjectId,
    Date,
    Decimal,
    Binary,
    Document,
    Array,
    /// Any other type, named by mongosh's `$type` alias.
    Other(&'static str),
    Mixed,
}

impl Kind {
    fn of(value: &Bson) -> Option<Self> {
        Some(match value {
            Bson::Null | Bson::Undefined => return None,
            Bson::Boolean(_) => Self::Bool,
            Bson::Int32(_) => Self::Int32,
            Bson::Int64(_) => Self::Int64,
            Bson::Double(_) => Self::Double,
            Bson::String(_) => Self::String,
            Bson::ObjectId(_) => Self::ObjectId,
            Bson::DateTime(_) => Self::Date,
            Bson::Decimal128(_) => Self::Decimal,
            Bson::Binary(_) => Self::Binary,
            Bson::Document(_) => Self::Document,
            Bson::Array(_) => Self::Array,
            Bson::RegularExpression(_) => Self::Other("regex"),
            Bson::Timestamp(_) => Self::Other("timestamp"),
            Bson::JavaScriptCode(_) | Bson::JavaScriptCodeWithScope(_) => Self::Other("javascript"),
            Bson::Symbol(_) => Self::Other("symbol"),
            Bson::MinKey => Self::Other("minKey"),
            Bson::MaxKey => Self::Other("maxKey"),
            Bson::DbPointer(_) => Self::Other("dbPointer"),
        })
    }

    /// The kind of a column that holds both kinds.
    fn merge(self, other: Self) -> Self {
        use Kind::{Double, Int32, Int64, Mixed};
        match (self, other) {
            (a, b) if a == b => a,
            (Int32 | Int64, Int32 | Int64) => Int64,
            (Int32 | Int64 | Double, Int32 | Int64 | Double) => Double,
            _ => Mixed,
        }
    }

    fn data_type(self) -> DataType {
        match self {
            Self::Bool => DataType::Boolean,
            Self::Int32 => DataType::Int32,
            Self::Int64 => DataType::Int64,
            Self::Double => DataType::Float64,
            Self::Date => DataType::Timestamp(TimeUnit::Millisecond, Some("UTC".into())),
            Self::Binary => DataType::Binary,
            _ => DataType::Utf8,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int32 => "int",
            Self::Int64 => "long",
            Self::Double => "double",
            Self::String => "string",
            Self::ObjectId => "objectId",
            Self::Date => "date",
            Self::Decimal => "decimal",
            Self::Binary => "binData",
            Self::Document => "document",
            Self::Array => "array",
            Self::Other(name) => name,
            Self::Mixed => "mixed",
        }
    }

    fn from_name(name: &str) -> Self {
        match name {
            "bool" => Self::Bool,
            "int" => Self::Int32,
            "long" => Self::Int64,
            "double" => Self::Double,
            "string" => Self::String,
            "objectId" => Self::ObjectId,
            "date" => Self::Date,
            "decimal" => Self::Decimal,
            "binData" => Self::Binary,
            "document" => Self::Document,
            "array" => Self::Array,
            _ => Self::Mixed,
        }
    }
}

/// The schema of documents like `sample`: their top-level fields, `_id` first, then in
/// the order they first appear. A field with only nulls is text.
#[must_use]
pub fn infer_schema(sample: &[Document]) -> SchemaRef {
    let mut order: Vec<&str> = Vec::new();
    let mut kinds: HashMap<&str, Option<Kind>> = HashMap::new();
    for doc in sample {
        for (key, value) in doc {
            let kind = Kind::of(value);
            if let Some(slot) = kinds.get_mut(key.as_str()) {
                *slot = match (*slot, kind) {
                    (Some(a), Some(b)) => Some(a.merge(b)),
                    (a, b) => a.or(b),
                };
            } else {
                order.push(key);
                kinds.insert(key, kind);
            }
        }
    }
    if let Some(i) = order.iter().position(|&k| k == "_id") {
        let id = order.remove(i);
        order.insert(0, id);
    }
    let fields: Vec<Field> = order
        .into_iter()
        .map(|name| {
            let kind = kinds[name].unwrap_or(Kind::String);
            field(name, kind)
        })
        .collect();
    Arc::new(Schema::new(fields))
}

fn field(name: &str, kind: Kind) -> Field {
    Field::new(name, kind.data_type(), true).with_metadata(HashMap::from([(
        META_DB_TYPE.to_owned(),
        kind.name().to_owned(),
    )]))
}

/// A one-column schema for values of any type, e.g. the result of `distinct`.
#[must_use]
pub fn value_schema(name: &str, values: &[Bson]) -> SchemaRef {
    let kind = values
        .iter()
        .filter_map(Kind::of)
        .reduce(Kind::merge)
        .unwrap_or(Kind::String);
    Arc::new(Schema::new(vec![field(name, kind)]))
}

/// The documents as a batch of `schema`; fields outside the schema are left out, and a
/// value that does not fit its column's type is null (text columns take any value).
pub fn to_batch(schema: &SchemaRef, docs: &[Document]) -> Result<RecordBatch> {
    let columns = schema
        .fields()
        .iter()
        .map(|f| column(f, docs.iter().map(|d| d.get(f.name()))))
        .collect();
    Ok(RecordBatch::try_new(Arc::clone(schema), columns)?)
}

fn kind_of_field(field: &Field) -> Kind {
    field
        .metadata()
        .get(META_DB_TYPE)
        .map_or(Kind::Mixed, |n| Kind::from_name(n))
}

#[allow(clippy::cast_possible_truncation)]
fn column<'a>(field: &Field, values: impl Iterator<Item = Option<&'a Bson>>) -> ArrayRef {
    match kind_of_field(field) {
        Kind::Bool => {
            let mut b = BooleanBuilder::new();
            values.for_each(|v| b.append_option(v.and_then(Bson::as_bool)));
            Arc::new(b.finish())
        }
        Kind::Int32 => {
            let mut b = Int32Builder::new();
            values.for_each(|v| b.append_option(v.and_then(Bson::as_i32)));
            Arc::new(b.finish())
        }
        Kind::Int64 => {
            let mut b = Int64Builder::new();
            values.for_each(|v| b.append_option(v.and_then(as_i64)));
            Arc::new(b.finish())
        }
        Kind::Double => {
            let mut b = Float64Builder::new();
            values.for_each(|v| b.append_option(v.and_then(as_f64)));
            Arc::new(b.finish())
        }
        Kind::Date => {
            let mut b = TimestampMillisecondBuilder::new().with_timezone("UTC");
            values.for_each(|v| match v {
                Some(Bson::DateTime(d)) => b.append_value(d.timestamp_millis()),
                _ => b.append_null(),
            });
            Arc::new(b.finish())
        }
        Kind::Binary => {
            let mut b = BinaryBuilder::new();
            values.for_each(|v| match v {
                Some(Bson::Binary(bin)) => b.append_value(&bin.bytes),
                _ => b.append_null(),
            });
            Arc::new(b.finish())
        }
        _ => {
            let mut b = StringBuilder::new();
            values.for_each(|v| b.append_option(v.and_then(text)));
            Arc::new(b.finish())
        }
    }
}

fn as_i64(value: &Bson) -> Option<i64> {
    match value {
        Bson::Int32(n) => Some(i64::from(*n)),
        Bson::Int64(n) => Some(*n),
        _ => None,
    }
}

#[allow(clippy::cast_precision_loss)]
fn as_f64(value: &Bson) -> Option<f64> {
    match value {
        Bson::Int32(n) => Some(f64::from(*n)),
        Bson::Int64(n) => Some(*n as f64),
        Bson::Double(n) => Some(*n),
        _ => None,
    }
}

/// A value as a text cell: strings as they are, `ObjectId`s as hex, dates in RFC 3339 and
/// anything else as relaxed Extended JSON. `None` for null.
#[must_use]
pub fn text(value: &Bson) -> Option<String> {
    Some(match value {
        Bson::Null | Bson::Undefined => return None,
        Bson::String(s) => s.clone(),
        Bson::ObjectId(id) => id.to_hex(),
        Bson::Decimal128(d) => d.to_string(),
        Bson::DateTime(d) => d
            .try_to_rfc3339_string()
            .unwrap_or_else(|_| d.timestamp_millis().to_string()),
        other => other.clone().into_relaxed_extjson().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use arrow_array::cast::AsArray;
    use arrow_array::types::{Int32Type, Int64Type, TimestampMillisecondType};
    use mongodb::bson::{DateTime, doc, oid::ObjectId};

    use super::*;

    fn db_type(schema: &Schema, i: usize) -> &str {
        &schema.field(i).metadata()[META_DB_TYPE]
    }

    #[test]
    fn infers_columns_from_the_sample() {
        let id = ObjectId::new();
        let docs = vec![
            doc! { "name": "a", "_id": id, "n": 1, "tags": ["x"], "when": DateTime::from_millis(5) },
            doc! { "_id": 2, "n": 3_000_000_000_i64, "extra": { "k": true }, "name": null },
            doc! { "_id": 3, "n": 1.5, "flag": true },
        ];
        let schema = infer_schema(&docs);
        let names: Vec<_> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        assert_eq!(names, ["_id", "name", "n", "tags", "when", "extra", "flag"]);
        assert_eq!(db_type(&schema, 0), "mixed");
        assert_eq!(schema.field(1).data_type(), &DataType::Utf8);
        assert_eq!(schema.field(2).data_type(), &DataType::Float64);
        assert_eq!(db_type(&schema, 3), "array");
        assert!(matches!(
            schema.field(4).data_type(),
            DataType::Timestamp(TimeUnit::Millisecond, _)
        ));
        assert_eq!(db_type(&schema, 5), "document");
        assert_eq!(schema.field(6).data_type(), &DataType::Boolean);

        let batch = to_batch(&schema, &docs).unwrap();
        assert_eq!(batch.num_rows(), 3);
        let ids = batch.column(0).as_string::<i32>();
        assert_eq!(ids.value(0), id.to_hex());
        assert_eq!(ids.value(1), "2");
        let n = batch
            .column(2)
            .as_primitive::<arrow_array::types::Float64Type>();
        assert!((n.value(1) - 3e9).abs() < 1.0);
        assert_eq!(batch.column(3).as_string::<i32>().value(0), r#"["x"]"#);
        assert_eq!(batch.column(5).as_string::<i32>().value(1), r#"{"k":true}"#);
        assert!(batch.column(5).is_null(0));
        assert_eq!(
            batch
                .column(4)
                .as_primitive::<TimestampMillisecondType>()
                .value(0),
            5
        );
    }

    #[test]
    fn keeps_integer_types_and_nulls_misfits() {
        let schema = infer_schema(&[doc! { "a": 1, "b": 1_i64 }, doc! { "a": 2, "b": 2 }]);
        assert_eq!(schema.field(0).data_type(), &DataType::Int32);
        assert_eq!(schema.field(1).data_type(), &DataType::Int64);
        // A later batch with values of another type and an unknown field.
        let batch = to_batch(&schema, &[doc! { "a": "x", "b": 7, "c": 1 }]).unwrap();
        assert!(batch.column(0).is_null(0));
        assert_eq!(batch.column(1).as_primitive::<Int64Type>().value(0), 7);
        assert_eq!(batch.num_columns(), 2);
        let _ = batch.column(0).as_primitive::<Int32Type>();
    }

    #[test]
    fn types_value_columns() {
        let values = [Bson::Int32(1), Bson::Null, Bson::Int32(3)];
        let schema = value_schema("n", &values);
        assert_eq!(schema.field(0).data_type(), &DataType::Int32);
        assert_eq!(value_schema("x", &[]).field(0).data_type(), &DataType::Utf8);
    }
}
