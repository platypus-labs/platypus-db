//! DuckDB type names for the result columns, kept in the Arrow field metadata
//! ([`META_DB_TYPE`]) so the grid shows `UUID` or `JSON` where Arrow only has `Utf8`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use arrow_schema::{DataType, FieldRef, Schema, SchemaRef};
use duckdb::core::{LogicalTypeHandle, LogicalTypeId};
use platypus_core::META_DB_TYPE;

/// Nested types deeper than this are named by their outer levels only.
const MAX_DEPTH: usize = 8;

/// The zone of DuckDB's `TIMESTAMP WITH TIME ZONE` values in Arrow. Arrow cannot read
/// named zones without `chrono-tz`; the offset is the same and portable.
const UTC: &str = "+00:00";

/// `data_type` with timestamps in named zones moved to [`UTC`] and the type of NULLs to
/// text; `None` when it has neither.
/// DuckDB exports every zoned timestamp in `UTC`, so the values stay the same.
pub fn portable(data_type: &DataType) -> Option<DataType> {
    let field = |f: &FieldRef| -> Option<FieldRef> {
        portable(f.data_type()).map(|t| Arc::new(f.as_ref().clone().with_data_type(t)))
    };
    match data_type {
        // A column of NULLs; Arrow's null arrays have no null flags to read.
        DataType::Null => Some(DataType::Utf8),
        DataType::Timestamp(unit, Some(tz)) if tz.as_ref() != UTC => {
            Some(DataType::Timestamp(*unit, Some(UTC.into())))
        }
        DataType::List(f) => field(f).map(DataType::List),
        DataType::LargeList(f) => field(f).map(DataType::LargeList),
        DataType::FixedSizeList(f, n) => field(f).map(|f| DataType::FixedSizeList(f, *n)),
        DataType::Map(f, sorted) => field(f).map(|f| DataType::Map(f, *sorted)),
        DataType::Struct(fields) => {
            let changed: Vec<Option<FieldRef>> = fields.iter().map(field).collect();
            changed.iter().any(Option::is_some).then(|| {
                DataType::Struct(
                    fields
                        .iter()
                        .zip(changed)
                        .map(|(f, c)| c.unwrap_or_else(|| Arc::clone(f)))
                        .collect(),
                )
            })
        }
        _ => None,
    }
}

/// `schema` with each field tagged with the DuckDB type of its result column, and zoned
/// timestamps made [`portable`].
pub fn tag_schema(schema: &Schema, stmt: &duckdb::Statement<'_>) -> SchemaRef {
    let fields: Vec<_> = schema
        .fields()
        .iter()
        .enumerate()
        .map(|(i, field)| {
            let mut metadata = field.metadata().clone();
            let name = type_name(&stmt.column_logical_type(i), 0);
            let bits = name == "BIT";
            metadata.insert(META_DB_TYPE.to_owned(), name);
            let field = field.as_ref().clone().with_metadata(metadata);
            match portable(field.data_type()) {
                // `BIT` arrives as DuckDB's bytes: shown as its digits, see [`bit_text`].
                _ if bits && *field.data_type() == DataType::Binary => {
                    field.with_data_type(DataType::Utf8)
                }
                Some(t) => field.with_data_type(t),
                None => field,
            }
        })
        .collect();
    Arc::new(Schema::new_with_metadata(
        fields,
        schema
            .metadata()
            .clone()
            .into_iter()
            .collect::<HashMap<_, _>>(),
    ))
}

/// A `BIT` value as DuckDB prints it (`1010`) from its bytes: the count of padding bits,
/// then the bits, the padding first.
#[must_use]
pub fn bit_text(bytes: &[u8]) -> Option<String> {
    let (&padding, bits) = bytes.split_first()?;
    let total = bits.len() * 8;
    let padding = usize::from(padding);
    (padding <= total).then(|| {
        (padding..total)
            .map(|i| {
                if (bits[i / 8] >> (7 - i % 8)) & 1 == 1 {
                    '1'
                } else {
                    '0'
                }
            })
            .collect()
    })
}

/// The type as DuckDB prints it: `DECIMAL(12,2)`, `INTEGER[]`, `STRUCT(a INTEGER)`; a
/// named type (an enum, `JSON`) by its name.
pub fn type_name(t: &LogicalTypeHandle, depth: usize) -> String {
    if let Some(alias) = t.get_alias() {
        return alias;
    }
    let Ok(id) = t.try_id() else {
        return "UNKNOWN".to_owned();
    };
    let nested = depth < MAX_DEPTH;
    match id {
        LogicalTypeId::Decimal => format!("DECIMAL({},{})", t.decimal_width(), t.decimal_scale()),
        LogicalTypeId::List | LogicalTypeId::Array if nested => {
            format!("{}[]", type_name(&t.child(0), depth + 1))
        }
        LogicalTypeId::Map if nested => format!(
            "MAP({}, {})",
            type_name(&t.child(0), depth + 1),
            type_name(&t.child(1), depth + 1)
        ),
        LogicalTypeId::Struct | LogicalTypeId::Union if nested => {
            let mut out = if id == LogicalTypeId::Struct {
                "STRUCT(".to_owned()
            } else {
                "UNION(".to_owned()
            };
            for i in 0..t.num_children() {
                if i > 0 {
                    out.push_str(", ");
                }
                let _ = write!(
                    out,
                    "{} {}",
                    t.child_name(i),
                    type_name(&t.child(i), depth + 1)
                );
            }
            out.push(')');
            out
        }
        other => scalar_name(other).to_owned(),
    }
}

const fn scalar_name(id: LogicalTypeId) -> &'static str {
    match id {
        LogicalTypeId::Boolean => "BOOLEAN",
        LogicalTypeId::Tinyint => "TINYINT",
        LogicalTypeId::Smallint => "SMALLINT",
        LogicalTypeId::Integer => "INTEGER",
        LogicalTypeId::Bigint => "BIGINT",
        LogicalTypeId::UTinyint => "UTINYINT",
        LogicalTypeId::USmallint => "USMALLINT",
        LogicalTypeId::UInteger => "UINTEGER",
        LogicalTypeId::UBigint => "UBIGINT",
        LogicalTypeId::Hugeint => "HUGEINT",
        LogicalTypeId::UHugeint => "UHUGEINT",
        LogicalTypeId::Float => "FLOAT",
        LogicalTypeId::Double => "DOUBLE",
        LogicalTypeId::Bignum => "BIGNUM",
        LogicalTypeId::Timestamp => "TIMESTAMP",
        LogicalTypeId::TimestampS => "TIMESTAMP_S",
        LogicalTypeId::TimestampMs => "TIMESTAMP_MS",
        LogicalTypeId::TimestampNs => "TIMESTAMP_NS",
        LogicalTypeId::TimestampTZ => "TIMESTAMP WITH TIME ZONE",
        LogicalTypeId::Date => "DATE",
        LogicalTypeId::Time => "TIME",
        LogicalTypeId::TimeNs => "TIME_NS",
        LogicalTypeId::TimeTZ => "TIME WITH TIME ZONE",
        LogicalTypeId::Interval => "INTERVAL",
        LogicalTypeId::Varchar => "VARCHAR",
        LogicalTypeId::Blob => "BLOB",
        LogicalTypeId::Bit => "BIT",
        LogicalTypeId::Uuid => "UUID",
        LogicalTypeId::Enum => "ENUM",
        LogicalTypeId::Geometry => "GEOMETRY",
        LogicalTypeId::Variant => "VARIANT",
        LogicalTypeId::SqlNull => "NULL",
        LogicalTypeId::List | LogicalTypeId::Array => "LIST",
        LogicalTypeId::Struct => "STRUCT",
        LogicalTypeId::Map => "MAP",
        LogicalTypeId::Union => "UNION",
        _ => "UNKNOWN",
    }
}
