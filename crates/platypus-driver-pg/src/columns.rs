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
use platypus_core::META_DB_TYPE;
use postgres_protocol::types as pg;
use tokio_postgres::Column;
use tokio_postgres::types::{Kind, Type};

use crate::decode::{
    self, DecodeResult, PG_EPOCH_DAYS, PG_EPOCH_MICROS, format_value, numeric_from_sql,
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
    /// `numeric(p,s)` with `p <= 38`. Non-finite values (`NaN`, infinities) become null.
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
    /// `timestamp` and `timestamptz` (`utc`). `±infinity` saturate to the `i64` extremes.
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
}

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
        Type::NUMERIC => match numeric_typmod(column.type_modifier()) {
            Some((precision, scale)) if (1..=DECIMAL128_MAX_PRECISION).contains(&precision) => {
                ColumnBuilder::Decimal {
                    builder: Decimal128Builder::new().with_precision_and_scale(precision, scale)?,
                    precision,
                    scale,
                }
            }
            _ => formatted(ty),
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
        _ => formatted(ty),
    };
    let field = Field::new(column.name(), builder.data_type(), true).with_metadata(HashMap::from(
        [(META_DB_TYPE.to_owned(), type_label(column))],
    ));
    Ok((field, builder))
}

fn formatted(ty: &Type) -> ColumnBuilder {
    ColumnBuilder::Formatted {
        builder: StringBuilder::new(),
        ty: ty.clone(),
    }
}

impl ColumnBuilder {
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
            Self::Text(_) | Self::Jsonb(_) | Self::Formatted { .. } => DataType::Utf8,
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
                builder.append_option(numeric_from_sql(raw)?.to_i128(*scale));
            }
            Self::Text(b) => b.append_value(std::str::from_utf8(raw)?),
            Self::Jsonb(b) => {
                // Binary jsonb is a version byte (1) followed by the JSON text.
                b.append_value(std::str::from_utf8(raw.get(1..).ok_or("empty jsonb")?)?);
            }
            Self::Binary(b) => b.append_value(raw),
            Self::Date(b) => b.append_value(pg::date_from_sql(raw)?.saturating_add(PG_EPOCH_DAYS)),
            Self::Time(b) => b.append_value(pg::time_from_sql(raw)?),
            Self::Timestamp { builder: b, .. } => {
                b.append_value(pg::timestamp_from_sql(raw)?.saturating_add(PG_EPOCH_MICROS));
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
            Self::Text(b) | Self::Jsonb(b) | Self::Formatted { builder: b, .. } => b.append_null(),
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
            Self::Text(b) | Self::Jsonb(b) | Self::Formatted { builder: b, .. } => {
                Arc::new(b.finish())
            }
            Self::Binary(b) => Arc::new(b.finish()),
            Self::Date(b) => Arc::new(b.finish()),
            Self::Time(b) => Arc::new(b.finish()),
            Self::Timestamp { builder: b, .. } => Arc::new(b.finish()),
            Self::Interval(b) => Arc::new(b.finish()),
        }
    }
}
