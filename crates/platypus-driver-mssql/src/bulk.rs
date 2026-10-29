//! [`Connection::bulk_load`](platypus_core::Connection::bulk_load) with SQL Server's bulk copy
//! (`INSERT BULK`, the protocol `bcp` and `SqlBulkCopy` use): the rows go to the table in a
//! stream of TDS rows instead of statements.
//!
//! The bulk copy takes values already of their column's type, so the text of each value is
//! converted here, for the types of the columns `SELECT TOP 0` describes. All the rows are
//! converted before the copy starts: a value that does not convert fails the load with
//! nothing written. A column of a type the bulk copy cannot write (`money`, `xml`, `text`,
//! `image`, `sql_variant`, CLR types) makes the whole load [`Error::Unsupported`], and the
//! import writes those rows its other way.

use std::borrow::Cow;

use platypus_core::{BulkRows, DbError, Error, Result};
use tiberius::numeric::Numeric;
use tiberius::time::{Date, DateTime, DateTime2, DateTimeOffset, SmallDateTime, Time};
use tiberius::{
    ColumnData, ColumnFlag, FixedLenType, MetaDataColumn, TokenRow, TypeInfo, Uuid, VarLenType,
};

use crate::{Session, map_error};

/// Days from 1 January of year 1 to 1 January 1900, where `datetime` counts from.
const DAYS_TO_1900: u32 = 693_595;

/// What a column's values are converted to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    TinyInt,
    SmallInt,
    Int,
    BigInt,
    Bit,
    Real,
    Float,
    Numeric {
        precision: u8,
        scale: u8,
    },
    Guid,
    Text,
    Binary,
    Date,
    /// `time(scale)`.
    Time(u8),
    DateTime2(u8),
    DateTimeOffset(u8),
    DateTime,
    SmallDateTime,
}

impl Kind {
    /// The kind of a column, `None` for a type the bulk copy cannot write.
    fn of(column: &MetaDataColumn<'_>) -> Option<Self> {
        Some(match &column.base.ty {
            TypeInfo::FixedLen(ty) => match ty {
                FixedLenType::Int1 => Self::TinyInt,
                FixedLenType::Int2 => Self::SmallInt,
                FixedLenType::Int4 => Self::Int,
                FixedLenType::Int8 => Self::BigInt,
                FixedLenType::Bit => Self::Bit,
                FixedLenType::Float4 => Self::Real,
                FixedLenType::Float8 => Self::Float,
                FixedLenType::Datetime => Self::DateTime,
                FixedLenType::Datetime4 => Self::SmallDateTime,
                FixedLenType::Money | FixedLenType::Money4 | FixedLenType::Null => return None,
            },
            TypeInfo::VarLenSized(ctx) => match (ctx.r#type(), ctx.len()) {
                (VarLenType::Intn, 1) => Self::TinyInt,
                (VarLenType::Intn, 2) => Self::SmallInt,
                (VarLenType::Intn, 4) => Self::Int,
                (VarLenType::Intn, 8) => Self::BigInt,
                (VarLenType::Bitn, _) => Self::Bit,
                (VarLenType::Floatn, 4) => Self::Real,
                (VarLenType::Floatn, 8) => Self::Float,
                (VarLenType::Guid, _) => Self::Guid,
                (
                    VarLenType::BigChar
                    | VarLenType::BigVarChar
                    | VarLenType::NChar
                    | VarLenType::NVarchar,
                    _,
                ) => Self::Text,
                (VarLenType::BigBinary | VarLenType::BigVarBin, _) => Self::Binary,
                (VarLenType::Datetimen, 8) => Self::DateTime,
                (VarLenType::Datetimen, 4) => Self::SmallDateTime,
                (VarLenType::Daten, _) => Self::Date,
                // The length of these types is their scale.
                (VarLenType::Timen, scale) => Self::Time(u8::try_from(scale).ok()?),
                (VarLenType::Datetime2, scale) => Self::DateTime2(u8::try_from(scale).ok()?),
                (VarLenType::DatetimeOffsetn, scale) => {
                    Self::DateTimeOffset(u8::try_from(scale).ok()?)
                }
                _ => return None,
            },
            TypeInfo::VarLenSizedPrecision {
                ty: VarLenType::Decimaln | VarLenType::Numericn,
                precision,
                scale,
                ..
            } => Self::Numeric {
                precision: *precision,
                scale: *scale,
            },
            // The server refuses `xml` from a bulk copy client.
            TypeInfo::VarLenSizedPrecision { .. } | TypeInfo::Xml { .. } => return None,
        })
    }

    /// `text` as a value of this kind; `hex` when it is the hex text of bytes.
    fn value(self, text: &str, hex: bool) -> Option<ColumnData<'_>> {
        let int = || text.trim().parse::<i64>().ok();
        Some(match self {
            Self::TinyInt => ColumnData::U8(Some(u8::try_from(int()?).ok()?)),
            Self::SmallInt => ColumnData::I16(Some(i16::try_from(int()?).ok()?)),
            Self::Int => ColumnData::I32(Some(i32::try_from(int()?).ok()?)),
            Self::BigInt => ColumnData::I64(Some(int()?)),
            Self::Bit => ColumnData::Bit(Some(bit(text.trim())?)),
            Self::Real => ColumnData::F32(Some(text.trim().parse().ok()?)),
            Self::Float => ColumnData::F64(Some(text.trim().parse().ok()?)),
            Self::Numeric { precision, scale } => {
                ColumnData::Numeric(Some(numeric(text.trim(), precision, scale)?))
            }
            Self::Guid => ColumnData::Guid(Some(Uuid::parse_str(text.trim()).ok()?)),
            Self::Text => ColumnData::String(Some(Cow::Borrowed(text))),
            Self::Binary if hex => ColumnData::Binary(Some(Cow::Owned(unhex(text)?))),
            Self::Binary => ColumnData::Binary(Some(Cow::Borrowed(text.as_bytes()))),
            Self::Date => ColumnData::Date(Some(Date::new(date(text.trim())?))),
            Self::Time(scale) => ColumnData::Time(Some(time(text.trim(), scale)?)),
            Self::DateTime2(scale) => {
                let (date, time, _) = datetime(text.trim(), scale)?;
                ColumnData::DateTime2(Some(DateTime2::new(date, time)))
            }
            Self::DateTimeOffset(scale) => ColumnData::DateTimeOffset(Some(offset(text, scale)?)),
            Self::DateTime => {
                let (days, ticks) = since_1900(text.trim(), 300)?;
                // 1/300 of a second.
                ColumnData::DateTime(Some(DateTime::new(days, u32::try_from(ticks).ok()?)))
            }
            Self::SmallDateTime => {
                let (days, minutes) = since_1900(text.trim(), 0)?;
                ColumnData::SmallDateTime(Some(SmallDateTime::new(
                    u16::try_from(days).ok()?,
                    u16::try_from(minutes).ok()?,
                )))
            }
        })
    }
}

/// Writes `rows` with a bulk copy, in the session's transaction; the rows written. Called
/// with the session locked.
pub(crate) async fn load(session: &mut Session, rows: &BulkRows<'_>) -> Result<u64> {
    let sql = format!("INSERT BULK {}", rows.table);
    let columns = session
        .bulk_columns(rows.table, &rows.columns.join(", "))
        .await
        .map_err(|e| map_error(e, &sql))?;
    let kinds = columns
        .iter()
        .map(|column| {
            Kind::of(column).ok_or_else(|| {
                Error::Unsupported(format!(
                    "bulk copy into column {} ({:?})",
                    column.col_name, column.base.ty
                ))
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let data = rows
        .rows
        .iter()
        .map(|row| token_row(row, rows.hex, &columns, &kinds))
        .collect::<Result<Vec<_>>>()?;
    // As an `INSERT`: NULLs stay NULL, constraints and triggers apply, and the values of an
    // identity column are kept (the import names one only with `IDENTITY_INSERT`).
    let hints = "KEEP_NULLS, CHECK_CONSTRAINTS, FIRE_TRIGGERS";
    let mut request = session
        .bulk_insert_columns(rows.table, columns, hints)
        .await
        .map_err(|e| map_error(e, &sql))?;
    for row in data {
        request.send(row).await.map_err(|e| map_error(e, &sql))?;
    }
    let done = request.finalize().await.map_err(|e| map_error(e, &sql))?;
    Ok(done.total())
}

/// One row as the bulk copy sends it.
fn token_row<'a>(
    row: &'a [Option<String>],
    hex: &[bool],
    columns: &[MetaDataColumn<'_>],
    kinds: &[Kind],
) -> Result<TokenRow<'a>> {
    let mut out = TokenRow::with_capacity(kinds.len());
    for (((value, &hex), column), &kind) in row.iter().zip(hex).zip(columns).zip(kinds) {
        let data = match value {
            Some(text) => kind.value(text, hex).ok_or_else(|| {
                invalid(format!(
                    "'{text}' is not a value of column {}",
                    column.col_name
                ))
            })?,
            None if !column.base.flags.contains(ColumnFlag::Nullable) => {
                return Err(invalid(format!(
                    "column {} does not allow NULL",
                    column.col_name
                )));
            }
            None => null(kind),
        };
        out.push(data);
    }
    Ok(out)
}

fn invalid(message: String) -> Error {
    Error::Database(DbError {
        message,
        ..DbError::default()
    })
}

/// The NULL of a kind.
fn null(kind: Kind) -> ColumnData<'static> {
    match kind {
        Kind::TinyInt => ColumnData::U8(None),
        Kind::SmallInt => ColumnData::I16(None),
        Kind::Int => ColumnData::I32(None),
        Kind::BigInt => ColumnData::I64(None),
        Kind::Bit => ColumnData::Bit(None),
        Kind::Real => ColumnData::F32(None),
        Kind::Float => ColumnData::F64(None),
        Kind::Numeric { .. } => ColumnData::Numeric(None),
        Kind::Guid => ColumnData::Guid(None),
        Kind::Text => ColumnData::String(None),
        Kind::Binary => ColumnData::Binary(None),
        Kind::Date => ColumnData::Date(None),
        Kind::Time(_) => ColumnData::Time(None),
        Kind::DateTime2(_) => ColumnData::DateTime2(None),
        Kind::DateTimeOffset(_) => ColumnData::DateTimeOffset(None),
        Kind::DateTime => ColumnData::DateTime(None),
        Kind::SmallDateTime => ColumnData::SmallDateTime(None),
    }
}

fn bit(text: &str) -> Option<bool> {
    match text.to_ascii_lowercase().as_str() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

/// A decimal number (`-12.345`) at `scale`, rounded half away from zero as SQL Server
/// rounds; `None` when it has more than `precision` digits.
fn numeric(text: &str, precision: u8, scale: u8) -> Option<Numeric> {
    let (negative, digits) = match text.as_bytes().first()? {
        b'-' => (true, &text[1..]),
        b'+' => (false, &text[1..]),
        _ => (false, text),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty() && fraction.is_empty()
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let scale_len = usize::from(scale);
    let mut value: i128 = 0;
    for b in whole.bytes().chain(
        fraction
            .bytes()
            .chain(std::iter::repeat(b'0'))
            .take(scale_len),
    ) {
        value = value.checked_mul(10)?.checked_add(i128::from(b - b'0'))?;
    }
    if fraction
        .as_bytes()
        .get(scale_len)
        .is_some_and(|&b| b >= b'5')
    {
        value += 1;
    }
    if value >= 10_i128.checked_pow(u32::from(precision))? {
        return None;
    }
    Some(Numeric::new_with_scale(
        if negative { -value } else { value },
        scale,
    ))
}

/// The bytes of hex text (`00ff`).
fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Days from 1 January of year 1 to `YYYY-MM-DD`.
fn date(text: &str) -> Option<u32> {
    let mut parts = text.splitn(3, '-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) || day == 0 {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if day > month_days[month as usize - 1] {
        return None;
    }
    let before = year - 1;
    let days = before * 365 + before / 4 - before / 100
        + before / 400
        + i64::from(month_days[..month as usize - 1].iter().sum::<u32>())
        + i64::from(day - 1);
    u32::try_from(days).ok()
}

/// `HH:MM[:SS[.fffffff]]` as increments of 10^-`scale` seconds since midnight, the fraction
/// rounded to `scale` digits.
fn time(text: &str, scale: u8) -> Option<Time> {
    let increments = time_increments(text, scale)?;
    Some(Time::new(increments, scale))
}

fn time_increments(text: &str, scale: u8) -> Option<u64> {
    let (clock, fraction) = text.split_once('.').unwrap_or((text, ""));
    let mut parts = clock.splitn(3, ':');
    let hours: u64 = parts.next()?.parse().ok()?;
    let minutes: u64 = parts.next()?.parse().ok()?;
    let seconds: u64 = parts.next().map_or(Some(0), |s| s.parse().ok())?;
    if hours > 23 || minutes > 59 || seconds > 59 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let unit = 10_u64.pow(u32::from(scale));
    let scale_len = usize::from(scale);
    let digits: String = fraction
        .chars()
        .chain(std::iter::repeat('0'))
        .take(scale_len)
        .collect();
    let mut increments =
        (hours * 3600 + minutes * 60 + seconds) * unit + digits.parse::<u64>().unwrap_or(0);
    if fraction
        .as_bytes()
        .get(scale_len)
        .is_some_and(|&b| b >= b'5')
    {
        increments += 1;
    }
    // 23:59:59.9999999 rounded up stays within the day.
    Some(increments.min(86_400 * unit - 1))
}

/// `YYYY-MM-DD[( |T)HH:MM[:SS[.f]]]` and what follows the time (an offset).
fn datetime(text: &str, scale: u8) -> Option<(Date, Time, &str)> {
    let (day, rest) = match text.find([' ', 'T']) {
        Some(at) => (&text[..at], text[at + 1..].trim_start()),
        None => (text, ""),
    };
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == ':' || c == '.'))
        .unwrap_or(rest.len());
    let (clock, tail) = rest.split_at(end);
    let time = if clock.is_empty() {
        Time::new(0, scale)
    } else {
        time(clock, scale)?
    };
    Some((Date::new(date(day)?), time, tail.trim()))
}

/// A date and time with an offset (`+01:00`, `-0530`, `Z`; none is UTC): the UTC time and
/// the offset in minutes, as `datetimeoffset` stores them.
fn offset(text: &str, scale: u8) -> Option<DateTimeOffset> {
    let (date, time, tail) = datetime(text.trim(), scale)?;
    let minutes: i64 = match tail {
        "" | "Z" | "z" => 0,
        _ => {
            let sign = match tail.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let digits: String = tail[1..].chars().filter(|c| *c != ':').collect();
            if digits.len() != 2 && digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            let hours: i64 = digits[..2].parse().ok()?;
            let mins: i64 = if digits.len() == 4 {
                digits[2..].parse().ok()?
            } else {
                0
            };
            sign * (hours * 60 + mins)
        }
    };
    if minutes.abs() > 14 * 60 {
        return None;
    }
    let unit = 10_i64.pow(u32::from(scale));
    let day = 86_400 * unit;
    let at =
        i64::from(date.days()) * day + i64::try_from(time.increments()).ok()? - minutes * 60 * unit;
    if at < 0 {
        return None;
    }
    let utc = DateTime2::new(
        Date::new(u32::try_from(at / day).ok()?),
        Time::new(u64::try_from(at % day).ok()?, scale),
    );
    Some(DateTimeOffset::new(utc, i16::try_from(minutes).ok()?))
}

/// Days since 1 January 1900 and the time of day: in 1/`per_second` of a second, or in
/// minutes (rounded to the nearest) when `per_second` is 0, for `datetime` and
/// `smalldatetime`.
fn since_1900(text: &str, per_second: u64) -> Option<(i32, u64)> {
    // Milliseconds: `datetime` keeps 1/300 of a second.
    let (date, time, tail) = datetime(text, 3)?;
    if !tail.is_empty() {
        return None;
    }
    let days = i32::try_from(i64::from(date.days()) - i64::from(DAYS_TO_1900)).ok()?;
    let ms = time.increments();
    let (ticks, per_day) = if per_second == 0 {
        ((ms + 30_000) / 60_000, 1440)
    } else {
        ((ms * per_second + 500) / 1000, 86_400 * per_second)
    };
    // A time rounded up to midnight is the next day's.
    Some(if ticks >= per_day {
        (days + 1, 0)
    } else {
        (days, ticks)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_round_to_the_scale() {
        let n = |t| numeric(t, 10, 2).map(|n| (n.value(), n.scale()));
        assert_eq!(n("12.345"), Some((1235, 2)));
        assert_eq!(n("-12.344"), Some((-1234, 2)));
        assert_eq!(n("7"), Some((700, 2)));
        assert_eq!(n(".5"), Some((50, 2)));
        assert_eq!(n("99999999.99"), Some((9_999_999_999, 2)));
        assert_eq!(n("100000000"), None);
        assert_eq!(n("1e3"), None);
        assert_eq!(n("-"), None);
    }

    #[test]
    fn dates_count_days_from_year_one() {
        assert_eq!(date("0001-01-01"), Some(0));
        assert_eq!(date("1900-01-01"), Some(693_595));
        assert_eq!(date("2000-03-01"), Some(730_179));
        assert_eq!(date("2023-02-29"), None);
        assert_eq!(date("2024-02-29"), Some(738_944));
    }

    #[test]
    fn times_round_to_the_scale() {
        assert_eq!(time_increments("01:02:03.4567", 3), Some(3_723_457));
        assert_eq!(time_increments("01:02", 0), Some(3720));
        assert_eq!(time_increments("23:59:59.9999999", 3), Some(86_399_999));
        assert_eq!(time_increments("24:00:00", 0), None);
    }

    #[test]
    fn offsets_store_utc() {
        let dto = offset("2024-01-02 01:30:00+02:00", 0).unwrap();
        assert_eq!(dto.offset(), 120);
        assert_eq!(dto.datetime2().date().days(), date("2024-01-01").unwrap());
        assert_eq!(dto.datetime2().time().increments(), 23 * 3600 + 30 * 60);
        assert_eq!(offset("2024-01-02T03:04:05Z", 0).unwrap().offset(), 0);
        assert_eq!(
            offset("2024-01-02 03:04:05 -0530", 0).unwrap().offset(),
            -330
        );
        assert_eq!(offset("2024-01-02 03:04:05-05", 0).unwrap().offset(), -300);
        assert!(offset("2024-01-02 03:04:05+2", 0).is_none());
    }

    #[test]
    fn legacy_datetimes_count_from_1900() {
        assert_eq!(since_1900("1900-01-02 00:00:01", 300), Some((1, 300)));
        assert_eq!(since_1900("1900-01-01 00:00:00.010", 300), Some((0, 3)));
        assert_eq!(since_1900("2000-01-01 10:29:31", 0), Some((36_524, 630)));
        assert_eq!(since_1900("2000-01-01 23:59:45", 0), Some((36_525, 0)));
    }

    #[test]
    fn values_convert_or_fail() {
        assert!(Kind::TinyInt.value("256", false).is_none());
        assert!(matches!(
            Kind::Bit.value("TRUE", false),
            Some(ColumnData::Bit(Some(true)))
        ));
        assert!(matches!(
            Kind::Binary.value("00ff", true),
            Some(ColumnData::Binary(Some(b))) if *b == [0, 255]
        ));
        assert!(Kind::Guid.value("not a guid", false).is_none());
    }
}
