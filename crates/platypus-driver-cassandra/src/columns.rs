//! Result columns: the Arrow type of each CQL type, and the arrays pages of rows become.

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::builder::{
    BinaryBuilder, BooleanBuilder, Date32Builder, Float32Builder, Float64Builder, Int8Builder,
    Int16Builder, Int32Builder, Int64Builder, StringBuilder, Time64NanosecondBuilder,
    TimestampMillisecondBuilder,
};
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{DataType, Field, Schema, SchemaRef, TimeUnit};
use platypus_core::{META_DB_TYPE, Result};
use scylla::frame::response::result::{CollectionType, ColumnSpec, ColumnType, NativeType};
use scylla::value::{CqlValue, Row};

use crate::render;

/// Timestamps are instants; the grid shows them in UTC.
const UTC: &str = "+00:00";

/// How a column's values go into Arrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Bool,
    Int8,
    Int16,
    Int32,
    Int64,
    Float32,
    Float64,
    Date,
    Timestamp,
    Time,
    Blob,
    /// Text of a value Arrow has no type for: `varint`, `decimal`, `uuid`, `inet`, `duration`
    /// and the structures (as JSON).
    Rendered,
}

impl Kind {
    fn of(ty: &ColumnType<'_>) -> Self {
        match ty {
            ColumnType::Native(native) => match native {
                NativeType::Ascii | NativeType::Text => Self::Text,
                NativeType::Boolean => Self::Bool,
                NativeType::TinyInt => Self::Int8,
                NativeType::SmallInt => Self::Int16,
                NativeType::Int => Self::Int32,
                NativeType::BigInt | NativeType::Counter => Self::Int64,
                NativeType::Float => Self::Float32,
                NativeType::Double => Self::Float64,
                NativeType::Date => Self::Date,
                NativeType::Timestamp => Self::Timestamp,
                NativeType::Time => Self::Time,
                NativeType::Blob => Self::Blob,
                _ => Self::Rendered,
            },
            _ => Self::Rendered,
        }
    }

    fn data_type(self) -> DataType {
        match self {
            Self::Text | Self::Rendered => DataType::Utf8,
            Self::Bool => DataType::Boolean,
            Self::Int8 => DataType::Int8,
            Self::Int16 => DataType::Int16,
            Self::Int32 => DataType::Int32,
            Self::Int64 => DataType::Int64,
            Self::Float32 => DataType::Float32,
            Self::Float64 => DataType::Float64,
            Self::Date => DataType::Date32,
            Self::Timestamp => DataType::Timestamp(TimeUnit::Millisecond, Some(UTC.into())),
            Self::Time => DataType::Time64(TimeUnit::Nanosecond),
            Self::Blob => DataType::Binary,
        }
    }
}

/// The type as CQL writes it: `map<text, frozen<list<int>>>`, `address` for a user-defined
/// type, `tuple<int, text>`.
#[must_use]
pub fn type_name(ty: &ColumnType<'_>) -> String {
    match ty {
        ColumnType::Native(native) => native_name(native).to_owned(),
        ColumnType::Collection { typ, .. } => match typ {
            CollectionType::List(item) => format!("list<{}>", type_name(item)),
            CollectionType::Set(item) => format!("set<{}>", type_name(item)),
            CollectionType::Map(key, value) => {
                format!("map<{}, {}>", type_name(key), type_name(value))
            }
            _ => "collection".to_owned(),
        },
        ColumnType::Vector { typ, dimensions } => {
            format!("vector<{}, {dimensions}>", type_name(typ))
        }
        ColumnType::UserDefinedType { definition, .. } => definition.name.to_string(),
        ColumnType::Tuple(items) => format!(
            "tuple<{}>",
            items.iter().map(type_name).collect::<Vec<_>>().join(", ")
        ),
        _ => "unknown".to_owned(),
    }
}

const fn native_name(native: &NativeType) -> &'static str {
    match native {
        NativeType::Ascii => "ascii",
        NativeType::Boolean => "boolean",
        NativeType::Blob => "blob",
        NativeType::Counter => "counter",
        NativeType::Date => "date",
        NativeType::Decimal => "decimal",
        NativeType::Double => "double",
        NativeType::Duration => "duration",
        NativeType::Float => "float",
        NativeType::Int => "int",
        NativeType::BigInt => "bigint",
        NativeType::Text => "text",
        NativeType::Timestamp => "timestamp",
        NativeType::Inet => "inet",
        NativeType::SmallInt => "smallint",
        NativeType::TinyInt => "tinyint",
        NativeType::Time => "time",
        NativeType::Timeuuid => "timeuuid",
        NativeType::Uuid => "uuid",
        NativeType::Varint => "varint",
        _ => "unknown",
    }
}

/// The columns of a result: names, CQL types and how each is converted.
#[derive(Debug, Clone)]
pub struct Layout {
    kinds: Vec<Kind>,
    schema: SchemaRef,
}

impl Layout {
    /// The layout of a result with these columns.
    pub fn of<'a>(specs: impl IntoIterator<Item = &'a ColumnSpec<'a>>) -> Self {
        let mut kinds = Vec::new();
        let mut fields = Vec::new();
        for spec in specs {
            let kind = Kind::of(spec.typ());
            kinds.push(kind);
            fields.push(
                Field::new(spec.name(), kind.data_type(), true).with_metadata(HashMap::from([(
                    META_DB_TYPE.to_owned(),
                    type_name(spec.typ()),
                )])),
            );
        }
        Self {
            kinds,
            schema: Arc::new(Schema::new(fields)),
        }
    }

    #[must_use]
    pub fn schema(&self) -> SchemaRef {
        Arc::clone(&self.schema)
    }

    /// A batch of `rows`.
    pub fn batch(&self, rows: &[Row]) -> Result<RecordBatch> {
        let columns: Vec<ArrayRef> = self
            .kinds
            .iter()
            .enumerate()
            .map(|(i, kind)| {
                build(
                    *kind,
                    rows.iter()
                        .map(|row| row.columns.get(i).and_then(Option::as_ref)),
                )
            })
            .collect();
        Ok(RecordBatch::try_new_with_options(
            self.schema(),
            columns,
            &arrow_array::RecordBatchOptions::new().with_row_count(Some(rows.len())),
        )?)
    }
}

/// Appends each value with `append`, null for a missing one or one of another type.
macro_rules! fill {
    ($builder:expr, $values:expr, |$v:ident| $get:expr) => {{
        let mut builder = $builder;
        for value in $values {
            match value.and_then(|$v| $get) {
                Some(x) => builder.append_value(x),
                None => builder.append_null(),
            }
        }
        Arc::new(builder.finish()) as ArrayRef
    }};
}

fn build<'a>(kind: Kind, values: impl Iterator<Item = Option<&'a CqlValue>>) -> ArrayRef {
    match kind {
        Kind::Text => fill!(StringBuilder::new(), values, |v| match v {
            CqlValue::Ascii(s) | CqlValue::Text(s) => Some(s.as_str()),
            _ => None,
        }),
        Kind::Rendered => fill!(StringBuilder::new(), values, |v| match v {
            CqlValue::Empty => None,
            other => Some(render::text(other)),
        }),
        Kind::Bool => fill!(BooleanBuilder::new(), values, |v| v.as_boolean()),
        Kind::Int8 => fill!(Int8Builder::new(), values, |v| v.as_tinyint()),
        Kind::Int16 => fill!(Int16Builder::new(), values, |v| v.as_smallint()),
        Kind::Int32 => fill!(Int32Builder::new(), values, |v| v.as_int()),
        Kind::Int64 => fill!(Int64Builder::new(), values, |v| match v {
            CqlValue::BigInt(n) => Some(*n),
            CqlValue::Counter(n) => Some(n.0),
            _ => None,
        }),
        Kind::Float32 => fill!(Float32Builder::new(), values, |v| v.as_float()),
        Kind::Float64 => fill!(Float64Builder::new(), values, |v| v.as_double()),
        Kind::Date => fill!(Date32Builder::new(), values, |v| v
            .as_cql_date()
            .and_then(|d| i32::try_from(render::days_since_epoch(d)).ok())),
        Kind::Timestamp => fill!(
            TimestampMillisecondBuilder::new().with_timezone(UTC),
            values,
            |v| v.as_cql_timestamp().map(|t| t.0)
        ),
        Kind::Time => fill!(Time64NanosecondBuilder::new(), values, |v| v
            .as_cql_time()
            .map(|t| t.0)),
        Kind::Blob => fill!(BinaryBuilder::new(), values, |v| v
            .as_blob()
            .map(Vec::as_slice)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::{Array, Int32Array, StringArray};
    use scylla::value::{CqlDate, CqlTimestamp};

    fn specs(types: Vec<(&str, ColumnType<'static>)>) -> Vec<ColumnSpec<'static>> {
        types
            .into_iter()
            .map(|(name, ty)| {
                ColumnSpec::owned(
                    name.to_owned(),
                    ty,
                    scylla::frame::response::result::TableSpec::owned("k".into(), "t".into()),
                )
            })
            .collect()
    }

    #[test]
    fn names_types_the_way_cql_writes_them() {
        let list = ColumnType::Collection {
            frozen: false,
            typ: CollectionType::List(Box::new(ColumnType::Native(NativeType::Int))),
        };
        assert_eq!(type_name(&list), "list<int>");
        let map = ColumnType::Collection {
            frozen: false,
            typ: CollectionType::Map(
                Box::new(ColumnType::Native(NativeType::Text)),
                Box::new(list),
            ),
        };
        assert_eq!(type_name(&map), "map<text, list<int>>");
        let tuple = ColumnType::Tuple(vec![
            ColumnType::Native(NativeType::Int),
            ColumnType::Native(NativeType::Text),
        ]);
        assert_eq!(type_name(&tuple), "tuple<int, text>");
    }

    #[test]
    fn converts_rows_to_typed_columns() {
        let specs = specs(vec![
            ("id", ColumnType::Native(NativeType::Int)),
            ("name", ColumnType::Native(NativeType::Text)),
            ("born", ColumnType::Native(NativeType::Date)),
            ("seen", ColumnType::Native(NativeType::Timestamp)),
            ("big", ColumnType::Native(NativeType::Varint)),
        ]);
        let layout = Layout::of(&specs);
        let rows = vec![
            Row {
                columns: vec![
                    Some(CqlValue::Int(1)),
                    Some(CqlValue::Text("a".into())),
                    Some(CqlValue::Date(CqlDate(1 << 31))),
                    Some(CqlValue::Timestamp(CqlTimestamp(1000))),
                    Some(CqlValue::Varint(
                        scylla::value::CqlVarint::from_signed_bytes_be(vec![0x01, 0x00]),
                    )),
                ],
            },
            Row {
                columns: vec![None, None, None, None, None],
            },
        ];
        let batch = layout.batch(&rows).unwrap();
        assert_eq!(batch.num_rows(), 2);
        let ids = batch
            .column(0)
            .as_any()
            .downcast_ref::<Int32Array>()
            .unwrap();
        assert_eq!((ids.value(0), ids.is_null(1)), (1, true));
        let big = batch
            .column(4)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(big.value(0), "256");
        assert_eq!(layout.schema().field(4).metadata()[META_DB_TYPE], "varint");
        assert_eq!(layout.schema().field(2).data_type(), &DataType::Date32);
    }
}
