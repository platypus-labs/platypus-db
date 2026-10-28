//! Maps PostgreSQL result columns to Arrow fields and incremental array builders.

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::ArrayRef;
use arrow_array::builder::{
    BinaryBuilder, BooleanBuilder, Date32Builder, Decimal128Builder, Float32Builder,
    Float64Builder, Int16Builder, Int32Builder, Int64Builder, IntervalMonthDayNanoBuilder,
    StringBuilder, Time64MicrosecondBuilder, TimestampMicrosecondBuilder, UInt32Builder,
};
use arrow_array::types::IntervalMonthDayNano;
use arrow_schema::{ArrowError, DataType, Field, IntervalUnit, TimeUnit};
use platypus_core::{META_DB_TYPE, special};
use postgres_protocol::types as pg;
use tokio_postgres::Column;
use tokio_postgres::types::{Kind, Type};

use crate::decode::{
    self, DecodeResult, Numeric, PG_EPOCH_DAYS, PG_EPOCH_MICROS, format_value, numeric_from_sql,
    numeric_typmod,
};

// Arrow cannot parse named zones without `chrono-tz`; an offset is equivalent and portable.
const UTC: &str = "+00:00";

/// Largest precision an Arrow `Decimal128` can hold.
const DECIMAL128_MAX_PRECISION: u8 = 38;

#[derive(Debug)]
pub enum ColumnBuilder {
    Bool(BooleanBuilder),
    Int16(Int16Builder),
    Int32(Int32Builder),
    Int64(Int64Builder),
    UInt32(UInt32Builder),
    Float32(Float32Builder),
    Float64(Float64Builder),
    /// `numeric(p,s)` with `0 < p <= 38` and `s <= p`. `NaN` is [`special::DECIMAL_NAN`], in a
    /// field marked with [`special::META_DECIMAL_NAN`]; such a column holds no infinity.
    Decimal {
        builder: Decimal128Builder,
        precision: u8,
        scale: i8,
    },
    /// Types whose binary format is plain UTF-8 text.
    Text(StringBuilder),
    Jsonb(StringBuilder),
    Binary(BinaryBuilder),
    Date(Date32Builder),
    Time(Time64MicrosecondBuilder),
    /// `timestamp` and `timestamptz` (`utc`). `Â±infinity` saturate to the `i64` extremes.
    Timestamp {
        builder: TimestampMicrosecondBuilder,
        utc: bool,
    },
    Interval(IntervalMonthDayNanoBuilder),
    /// Everything else, rendered as text with [`format_value`].
    Formatted {
        builder: StringBuilder,
        ty: Type,
    },
    /// An OID alias type (`regclass`, `regtype`, …): the name of each OID, read from the
    /// catalog with [`Self::name_query`] before the rows; an OID without one is its number.
    Named {
        builder: StringBuilder,
        ty: Type,
        names: HashMap<u32, String>,
    },
}

/// The catalog of each OID alias type, whose OIDs the type names.
const NAMED: &[(Type, &str)] = &[
    (Type::REGCLASS, "pg_class"),
    (Type::REGTYPE, "pg_type"),
    (Type::REGPROC, "pg_proc"),
    (Type::REGPROCEDURE, "pg_proc"),
    (Type::REGOPER, "pg_operator"),
    (Type::REGOPERATOR, "pg_operator"),
    (Type::REGNAMESPACE, "pg_namespace"),
    (Type::REGROLE, "pg_roles"),
    (Type::REGCONFIG, "pg_ts_config"),
    (Type::REGDICTIONARY, "pg_ts_dict"),
    (Type::REGCOLLATION, "pg_collation"),
];

/// Strips domains down to their base type.
fn base_type(ty: &Type) -> &Type {
    match ty.kind() {
        Kind::Domain(base) => base_type(base),
        _ => ty,
    }
}

/// Human-readable type name including modifiers, e.g. `numeric(12,2)` or `int4[]`.
fn type_label(column: &Column) -> String {
    let ty = column.type_();
    let typmod = column.type_modifier();
    match *ty {
        Type::NUMERIC => {
            if let Some((p, s)) = numeric_typmod(typmod) {
                return format!("numeric({p},{s})");
            }
        }
        Type::VARCHAR | Type::BPCHAR if typmod >= 4 => {
            let name = if *ty == Type::VARCHAR {
                "varchar"
            } else {
                "char"
            };
            return format!("{name}({})", typmod - 4);
        }
        _ => {}
    }
    match ty.kind() {
        Kind::Array(element) => format!("{}[]", element.name()),
        _ => ty.name().to_owned(),
    }
}

pub fn column_builder(column: &Column) -> Result<(Field, ColumnBuilder), ArrowError> {
    let ty = column.type_();
    let base = base_type(ty);
    let builder = match *base {
        Type::BOOL => ColumnBuilder::Bool(BooleanBuilder::new()),
        Type::INT2 => ColumnBuilder::Int16(Int16Builder::new()),
        Type::INT4 => ColumnBuilder::Int32(Int32Builder::new()),
        Type::INT8 => ColumnBuilder::Int64(Int64Builder::new()),
        Type::OID => ColumnBuilder::UInt32(UInt32Builder::new()),
        Type::FLOAT4 => ColumnBuilder::Float32(Float32Builder::new()),
        Type::FLOAT8 => ColumnBuilder::Float64(Float64Builder::new()),
        Type::NUMERIC => match decimal_type(column.type_modifier()) {
            Some((precision, scale)) => ColumnBuilder::Decimal {
                builder: Decimal128Builder::new().with_precision_and_scale(precision, scale)?,
                precision,
                scale,
            },
            None => formatted(ty),
        },
        Type::TEXT
        | Type::VARCHAR
        | Type::BPCHAR
        | Type::NAME
        | Type::JSON
        | Type::XML
        | Type::UNKNOWN => ColumnBuilder::Text(StringBuilder::new()),
        Type::JSONB => ColumnBuilder::Jsonb(StringBuilder::new()),
        Type::BYTEA => ColumnBuilder::Binary(BinaryBuilder::new()),
        Type::DATE => ColumnBuilder::Date(Date32Builder::new()),
        Type::TIME => ColumnBuilder::Time(Time64MicrosecondBuilder::new()),
        Type::TIMESTAMP => ColumnBuilder::timestamp(false),
        Type::TIMESTAMPTZ => ColumnBuilder::timestamp(true),
        Type::INTERVAL => ColumnBuilder::Interval(IntervalMonthDayNanoBuilder::new()),
        _ if matches!(base.kind(), Kind::Enum(_)) => ColumnBuilder::Text(StringBuilder::new()),
        _ if NAMED.iter().any(|(t, _)| t == base) => ColumnBuilder::Named {
            builder: StringBuilder::new(),
            ty: base.clone(),
            names: HashMap::new(),
        },
        _ => formatted(ty),
    };
    let field = Field::new(column.name(), builder.data_type(), true).with_metadata(HashMap::from(
        [(META_DB_TYPE.to_owned(), type_label(column))],
    ));
    // Its `NaN` is a reserved decimal, which the field tells from a number.
    let field = if matches!(builder, ColumnBuilder::Decimal { .. }) {
        special::with_decimal_nan(field)
    } else {
        field
    };
    Ok((field, builder))
}

/// Reads the names of the OIDs of the OID alias columns among `builders`, on the session
/// that runs the query (so `regclass` names follow its search path). A catalog that cannot
/// be read leaves the numbers.
pub async fn load_names(client: &tokio_postgres::Client, builders: &mut [ColumnBuilder]) {
    for builder in builders.iter_mut() {
        let Some(sql) = builder.name_query() else {
            continue;
        };
        // Unnamed, in one round trip: a pooler in transaction mode may not keep a name.
        if let Ok(rows) = client.query_typed(&sql, &[]).await {
            builder.set_names(
                rows.iter()
                    .filter_map(|r| Some((r.try_get(0).ok()?, r.try_get(1).ok()?)))
                    .collect(),
            );
        }
    }
}

/// `(precision, scale)` of a `numeric` type modifier that an Arrow `Decimal128` holds.
/// PostgreSQL 15 allows a scale above the precision, e.g. `numeric(3,5)`, which Arrow does
/// not: those, and `numeric` without a modifier, are text.
fn decimal_type(typmod: i32) -> Option<(u8, i8)> {
    numeric_typmod(typmod).filter(|&(precision, scale)| {
        (1..=DECIMAL128_MAX_PRECISION).contains(&precision)
            && i16::from(scale) <= i16::from(precision)
    })
}

fn formatted(ty: &Type) -> ColumnBuilder {
    ColumnBuilder::Formatted {
        builder: StringBuilder::new(),
        ty: ty.clone(),
    }
}

impl ColumnBuilder {
    /// For an OID alias column, the query of its OIDs and their names.
    #[must_use]
    pub fn name_query(&self) -> Option<String> {
        let Self::Named { ty, .. } = self else {
            return None;
        };
        let (_, catalog) = NAMED.iter().find(|(t, _)| t == ty)?;
        Some(format!(
            "SELECT oid, oid::{}::text FROM pg_catalog.{catalog}",
            ty.name()
        ))
    }

    /// Sets the names of an OID alias column's OIDs.
    pub fn set_names(&mut self, names: HashMap<u32, String>) {
        if let Self::Named { names: n, .. } = self {
            *n = names;
        }
    }

    #[must_use]
    pub fn timestamp(utc: bool) -> Self {
        let builder = TimestampMicrosecondBuilder::new();
        let builder = if utc {
            builder.with_timezone(UTC)
        } else {
            builder
        };
        Self::Timestamp { builder, utc }
    }

    fn data_type(&self) -> DataType {
        match self {
            Self::Bool(_) => DataType::Boolean,
            Self::Int16(_) => DataType::Int16,
            Self::Int32(_) => DataType::Int32,
            Self::Int64(_) => DataType::Int64,
            Self::UInt32(_) => DataType::UInt32,
            Self::Float32(_) => DataType::Float32,
            Self::Float64(_) => DataType::Float64,
            Self::Decimal {
                precision, scale, ..
            } => DataType::Decimal128(*precision, *scale),
            Self::Text(_) | Self::Jsonb(_) | Self::Formatted { .. } | Self::Named { .. } => {
                DataType::Utf8
            }
            Self::Binary(_) => DataType::Binary,
            Self::Date(_) => DataType::Date32,
            Self::Time(_) => DataType::Time64(TimeUnit::Microsecond),
            Self::Timestamp { utc, .. } => {
                DataType::Timestamp(TimeUnit::Microsecond, utc.then(|| UTC.into()))
            }
            Self::Interval(_) => DataType::Interval(IntervalUnit::MonthDayNano),
        }
    }

    /// Appends one wire value; `None` is SQL `NULL`.
    pub fn append(&mut self, raw: Option<&[u8]>) -> DecodeResult<()> {
        let Some(raw) = raw else {
            self.append_null();
            return Ok(());
        };
        match self {
            Self::Bool(b) => b.append_value(pg::bool_from_sql(raw)?),
            Self::Int16(b) => b.append_value(pg::int2_from_sql(raw)?),
            Self::Int32(b) => b.append_value(pg::int4_from_sql(raw)?),
            Self::Int64(b) => b.append_value(pg::int8_from_sql(raw)?),
            Self::UInt32(b) => b.append_value(pg::oid_from_sql(raw)?),
            Self::Float32(b) => b.append_value(pg::float4_from_sql(raw)?),
            Self::Float64(b) => b.append_value(pg::float8_from_sql(raw)?),
            Self::Decimal { builder, scale, .. } => {
                let value = match numeric_from_sql(raw)? {
                    Numeric::NaN => special::DECIMAL_NAN,
                    n => n
                        .to_i128(*scale)
                        .ok_or_else(|| format!("{n} does not fit the column's numeric type"))?,
                };
                builder.append_value(value);
            }
            Self::Text(b) => b.append_value(std::str::from_utf8(raw)?),
            Self::Jsonb(b) => {
                // Binary jsonb is a version byte (1) followed by the JSON text.
                b.append_value(std::str::from_utf8(raw.get(1..).ok_or("empty jsonb")?)?);
            }
            Self::Binary(b) => b.append_value(raw),
            Self::Date(b) => b.append_value(match pg::date_from_sql(raw)? {
                i32::MAX => special::DATE_INFINITY,
                i32::MIN => special::DATE_NEG_INFINITY,
                days => days.saturating_add(PG_EPOCH_DAYS),
            }),
            Self::Time(b) => b.append_value(pg::time_from_sql(raw)?),
            Self::Timestamp { builder: b, .. } => {
                b.append_value(match pg::timestamp_from_sql(raw)? {
                    i64::MAX => special::TIMESTAMP_INFINITY,
                    i64::MIN => special::TIMESTAMP_NEG_INFINITY,
                    micros => micros.saturating_add(PG_EPOCH_MICROS),
                });
            }
            Self::Interval(b) => {
                let (micros, days, months) = decode::interval_from_sql(raw)?;
                b.append_value(IntervalMonthDayNano::new(
                    months,
                    days,
                    micros.saturating_mul(1000),
                ));
            }
            Self::Formatted { builder, ty } => builder.append_value(format_value(ty, raw)?),
            Self::Named { builder, names, .. } => {
                let oid = pg::oid_from_sql(raw)?;
                match names.get(&oid) {
                    Some(name) => builder.append_value(name),
                    // PostgreSQL writes `-` for no object.
                    None if oid == 0 => builder.append_value("-"),
                    None => builder.append_value(oid.to_string()),
                }
            }
        }
        Ok(())
    }

    fn append_null(&mut self) {
        match self {
            Self::Bool(b) => b.append_null(),
            Self::Int16(b) => b.append_null(),
            Self::Int32(b) => b.append_null(),
            Self::Int64(b) => b.append_null(),
            Self::UInt32(b) => b.append_null(),
            Self::Float32(b) => b.append_null(),
            Self::Float64(b) => b.append_null(),
            Self::Decimal { builder, .. } => builder.append_null(),
            Self::Text(b)
            | Self::Jsonb(b)
            | Self::Formatted { builder: b, .. }
            | Self::Named { builder: b, .. } => b.append_null(),
            Self::Binary(b) => b.append_null(),
            Self::Date(b) => b.append_null(),
            Self::Time(b) => b.append_null(),
            Self::Timestamp { builder: b, .. } => b.append_null(),
            Self::Interval(b) => b.append_null(),
        }
    }

    /// Returns the values appended so far and resets the builder for the next batch.
    pub fn finish(&mut self) -> ArrayRef {
        match self {
            Self::Bool(b) => Arc::new(b.finish()),
            Self::Int16(b) => Arc::new(b.finish()),
            Self::Int32(b) => Arc::new(b.finish()),
            Self::Int64(b) => Arc::new(b.finish()),
            Self::UInt32(b) => Arc::new(b.finish()),
            Self::Float32(b) => Arc::new(b.finish()),
            Self::Float64(b) => Arc::new(b.finish()),
            Self::Decimal { builder, .. } => Arc::new(builder.finish()),
            Self::Text(b)
            | Self::Jsonb(b)
            | Self::Formatted { builder: b, .. }
            | Self::Named { builder: b, .. } => Arc::new(b.finish()),
            Self::Binary(b) => Arc::new(b.finish()),
            Self::Date(b) => Arc::new(b.finish()),
            Self::Time(b) => Arc::new(b.finish()),
            Self::Timestamp { builder: b, .. } => Arc::new(b.finish()),
            Self::Interval(b) => Arc::new(b.finish()),
        }
    }
}

#[cfg(test)]
mod tests {
    use arrow_array::Array;

    use super::*;

    /// The values as every text output shows them.
    fn shown(builder: &mut ColumnBuilder, values: &[&[u8]]) -> Vec<String> {
        for v in values {
            builder.append(Some(v)).unwrap();
        }
        let array = builder.finish();
        let options = arrow_cast::display::FormatOptions::default();
        let f = special::formatter(array.as_ref(), &options).unwrap();
        (0..array.len()).map(|i| f.value(i).to_string()).collect()
    }

    #[test]
    fn keeps_infinite_dates_and_timestamps() {
        let mut dates = ColumnBuilder::Date(Date32Builder::new());
        assert_eq!(
            shown(
                &mut dates,
                &[
                    &i32::MAX.to_be_bytes(),
                    &i32::MIN.to_be_bytes(),
                    &0_i32.to_be_bytes()
                ]
            ),
            ["infinity", "-infinity", "2000-01-01"]
        );
        let mut stamps = ColumnBuilder::timestamp(true);
        assert_eq!(
            shown(
                &mut stamps,
                &[
                    &i64::MAX.to_be_bytes(),
                    &i64::MIN.to_be_bytes(),
                    &0_i64.to_be_bytes()
                ]
            ),
            ["infinity", "-infinity", "2000-01-01T00:00:00Z"]
        );
    }

    #[test]
    fn keeps_numeric_nan() {
        let mut numbers = ColumnBuilder::Decimal {
            builder: Decimal128Builder::new()
                .with_precision_and_scale(10, 2)
                .unwrap(),
            precision: 10,
            scale: 2,
        };
        // ndigits, weight, sign, dscale; then the base-10000 digits.
        let nan: &[u8] = &[0, 0, 0, 0, 0xC0, 0, 0, 0];
        let one_and_a_half: &[u8] = &[0, 2, 0, 0, 0, 0, 0, 2, 0, 1, 0x13, 0x88];
        numbers.append(Some(nan)).unwrap();
        numbers.append(Some(one_and_a_half)).unwrap();
        let array = numbers.finish();
        let field = special::with_decimal_nan(Field::new("n", array.data_type().clone(), true));
        let options = arrow_cast::display::FormatOptions::default();
        let f = special::column_formatter(&field, array.as_ref(), &options).unwrap();
        let shown: Vec<String> = (0..2).map(|i| f.value(i).to_string()).collect();
        assert_eq!(shown, ["NaN", "1.50"]);
        // A constrained numeric holds no infinity: it is an error, not a NULL.
        let infinity: &[u8] = &[0, 0, 0, 0, 0xD0, 0, 0, 0];
        assert!(numbers.append(Some(infinity)).is_err());
    }

    #[test]
    fn decimal_columns_only_for_scales_arrow_takes() {
        let typmod = |p: i32, s: i32| ((p << 16) | (s & 0x7ff)) + 4;
        assert_eq!(decimal_type(typmod(10, 2)), Some((10, 2)));
        assert_eq!(decimal_type(typmod(5, -2)), Some((5, -2)));
        assert_eq!(decimal_type(typmod(3, 5)), None, "scale above precision");
        assert_eq!(decimal_type(typmod(39, 0)), None);
        assert_eq!(decimal_type(-1), None, "no modifier");
    }
}
