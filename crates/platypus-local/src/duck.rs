//! A DuckDB database filled with result rows, to query them with SQL.

use std::collections::HashSet;
use std::sync::Arc;

use arrow_array::cast::AsArray;
use arrow_array::types::Decimal128Type;
use arrow_array::{Array, ArrayRef, BooleanArray, Decimal128Array, RecordBatch, StringArray};
use arrow_cast::display::FormatOptions;
use arrow_schema::{ArrowError, DataType, Field, Schema, SchemaRef};
use duckdb::vtab::arrow::{ArrowBatchRegistration, ArrowVTab};
use platypus_core::{META_DB_TYPE, special};

use crate::{LocalError, values};

/// The table function that reads a registered Arrow batch.
const SCAN: &str = "platypus_arrow";

impl From<duckdb::Error> for LocalError {
    fn from(e: duckdb::Error) -> Self {
        Self::Duck(e.to_string())
    }
}

/// Creates `table` in `conn`, replacing one of that name, with the rows of `batches`; returns
/// how many. Columns DuckDB cannot store become text; repeated or empty names get new ones.
pub fn load(
    conn: &duckdb::Connection,
    table: &str,
    schema: &SchemaRef,
    batches: &[RecordBatch],
) -> Result<usize, LocalError> {
    register(conn)?;
    let rows = storable(schema, batches)?;
    let name = quote(table);
    let select = format!("SELECT {} FROM {SCAN}(?)", rows.select);
    conn.execute_batch(&format!("DROP TABLE IF EXISTS {name}"))?;
    let empty = ArrowBatchRegistration::new(RecordBatch::new_empty(rows.schema));
    conn.execute(&format!("CREATE TABLE {name} AS {select}"), [&empty])?;
    let mut insert = conn.prepare(&format!("INSERT INTO {name} {select}"))?;
    let mut count = 0;
    for batch in rows.batches {
        count += batch.num_rows();
        let registration = ArrowBatchRegistration::new(batch);
        insert.execute([&registration])?;
    }
    Ok(count)
}

/// Drops `table` from `conn` if it is there.
pub fn drop_table(conn: &duckdb::Connection, table: &str) -> Result<(), LocalError> {
    conn.execute_batch(&format!("DROP TABLE IF EXISTS {}", quote(table)))?;
    Ok(())
}

/// `name` as a DuckDB identifier.
#[must_use]
pub fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Column names DuckDB accepts in one table: never empty, distinct ignoring case, the later
/// ones of a name numbered from 2.
#[must_use]
pub fn unique_names(names: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let base = if name.is_empty() {
                format!("column{}", i + 1)
            } else {
                name.clone()
            };
            let mut candidate = base.clone();
            let mut n = 2;
            while !seen.insert(candidate.to_lowercase()) {
                candidate = format!("{base}_{n}");
                n += 1;
            }
            candidate
        })
        .collect()
}

/// Registers the Arrow scan once per database: its connections share it.
fn register(conn: &duckdb::Connection) -> Result<(), LocalError> {
    let present: i64 = conn.query_row(
        "SELECT count(*) FROM duckdb_functions() WHERE function_name = ?",
        [SCAN],
        |r| r.get(0),
    )?;
    if present == 0 {
        conn.register_table_function::<ArrowVTab>(SCAN)?;
    }
    Ok(())
}

/// How a column goes into DuckDB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Store {
    /// As it is.
    AsIs,
    /// As the text the grid shows.
    Text,
    /// An interval or a duration, with a column of its NULLs next to it: the Arrow scan writes
    /// the NULLs of an interval as zero, so the copy puts them back.
    Interval,
    /// A decimal with more digits than its type declares, widened to `MAX_DIGITS`.
    Wide(i8),
    /// Whole numbers past `MAX_DIGITS` (a HUGEINT read as `Decimal128(38, 0)`): as text, cast
    /// back to HUGEINT by the copy.
    Huge,
    /// Numbers a driver sends as text (PostgreSQL's `numeric` without a precision), cast by
    /// the copy to a DECIMAL of this scale, which holds them all.
    Decimal(u8),
    /// Numbers past `MAX_DIGITS` or with NaN or infinities (also a decimal column holding a
    /// numeric NaN): as text, cast to DOUBLE by the copy.
    Double,
}

/// The most digits of DuckDB's DECIMAL.
const MAX_DIGITS: u8 = 38;

/// The rows as DuckDB can store them, and the select list that copies them into the table.
struct Storable {
    schema: SchemaRef,
    batches: Vec<RecordBatch>,
    select: String,
}

/// What DuckDB can store: names made unique, text in place of the types it cannot take, the
/// NULLs of intervals in columns of their own after the others.
fn storable(schema: &SchemaRef, batches: &[RecordBatch]) -> Result<Storable, LocalError> {
    let stores = schema
        .fields()
        .iter()
        .enumerate()
        .map(|(c, f)| store(f, batches, c))
        .collect::<Result<Vec<_>, ArrowError>>()?;
    let width = stores.len();
    let masked: Vec<usize> = (0..width)
        .filter(|&c| stores[c] == Store::Interval)
        .collect();
    // The NULL columns come last, so the result's own names keep their spelling.
    let mut names: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();
    let masks: Vec<String> = masked
        .iter()
        .map(|&c| format!("{}_null", names[c]))
        .collect();
    names.extend(masks);
    let names = unique_names(&names);
    let mut fields: Vec<Field> = schema
        .fields()
        .iter()
        .zip(&stores)
        .zip(&names)
        .map(|((f, store), name)| {
            let data_type = match store {
                Store::AsIs | Store::Interval | Store::Decimal(_) => f.data_type().clone(),
                Store::Text | Store::Huge | Store::Double => DataType::Utf8,
                Store::Wide(scale) => DataType::Decimal128(MAX_DIGITS, *scale),
            };
            Field::new(name, data_type, true)
        })
        .collect();
    fields.extend(
        names[width..]
            .iter()
            .map(|name| Field::new(name, DataType::Boolean, false)),
    );
    let schema = Arc::new(Schema::new(fields));
    let batches = batches
        .iter()
        .map(|batch| {
            let mut columns = batch
                .columns()
                .iter()
                .zip(batch.schema_ref().fields())
                .zip(&stores)
                .map(|((c, f), store)| stored(f, c, *store))
                .collect::<Result<Vec<_>, LocalError>>()?;
            columns.extend(masked.iter().map(|&c| nulls_of(batch.column(c))));
            Ok(RecordBatch::try_new(Arc::clone(&schema), columns)?)
        })
        .collect::<Result<Vec<_>, LocalError>>()?;
    // One NULL column per interval, in the order of the intervals.
    let mut masks = names[width..].iter().map(|m| quote(m));
    let select = names[..width]
        .iter()
        .zip(&stores)
        .map(|(name, store)| {
            let column = quote(name);
            match store {
                Store::Interval => {
                    let mask = masks.next().unwrap_or_default();
                    format!("CASE WHEN {mask} THEN NULL ELSE {column} END AS {column}")
                }
                Store::Huge => format!("CAST({column} AS HUGEINT) AS {column}"),
                Store::Decimal(scale) => {
                    format!("CAST({column} AS DECIMAL({MAX_DIGITS}, {scale})) AS {column}")
                }
                Store::Double => format!("CAST({column} AS DOUBLE) AS {column}"),
                Store::AsIs | Store::Text | Store::Wide(_) => column,
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    Ok(Storable {
        schema,
        batches,
        select,
    })
}

/// How column `column` of `batches` goes into DuckDB.
fn store(field: &Field, batches: &[RecordBatch], column: usize) -> Result<Store, ArrowError> {
    match *field.data_type() {
        DataType::Interval(_) | DataType::Duration(_) => return Ok(Store::Interval),
        DataType::Decimal32(declared, scale)
        | DataType::Decimal64(declared, scale)
        | DataType::Decimal128(declared, scale)
            if scale >= 0 =>
        {
            let Some(digits) = most_digits(field, batches, column)? else {
                // A PostgreSQL numeric NaN: the column goes as DOUBLE, which holds it.
                return Ok(Store::Double);
            };
            return Ok(if digits <= u32::from(declared) {
                Store::AsIs
            } else if digits <= u32::from(MAX_DIGITS) {
                Store::Wide(scale)
            } else if scale == 0 {
                Store::Huge
            } else {
                Store::Text
            });
        }
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View if numeric_text(field) => {
            return text_numbers(batches, column);
        }
        _ => {}
    }
    let takes = writable(field.data_type())
        && duckdb::vtab::to_duckdb_logical_type_for_field(field).is_ok();
    Ok(if takes { Store::AsIs } else { Store::Text })
}

/// `array` as `store` puts it in DuckDB.
fn stored(field: &Field, array: &ArrayRef, store: Store) -> Result<ArrayRef, LocalError> {
    match store {
        Store::AsIs | Store::Interval | Store::Decimal(_) => Ok(Arc::clone(array)),
        // Cast from the text shown, which writes reserved values as DuckDB reads them (NaN).
        Store::Text | Store::Huge | Store::Double => as_text(field, array),
        Store::Wide(scale) => Ok(Arc::new(
            decimals(array)?.with_precision_and_scale(MAX_DIGITS, scale)?,
        )),
    }
}

/// The Arrow scan writes values of the type with their NULLs. Not: intervals inside lists or
/// structs (their NULLs become zero), `Decimal256` (binds but cannot be written), dictionaries
/// and half floats (no conversion), and the types it lacks.
fn writable(data_type: &DataType) -> bool {
    match data_type {
        DataType::Boolean
        | DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64
        | DataType::Float32
        | DataType::Float64
        | DataType::Decimal32(_, _)
        | DataType::Decimal64(_, _)
        | DataType::Decimal128(_, _)
        | DataType::Timestamp(_, _)
        | DataType::Date32
        | DataType::Date64
        | DataType::Time32(_)
        | DataType::Time64(_)
        | DataType::Utf8
        | DataType::LargeUtf8
        | DataType::Utf8View
        | DataType::Binary
        | DataType::LargeBinary
        | DataType::BinaryView
        | DataType::FixedSizeBinary(_) => true,
        DataType::List(f)
        | DataType::LargeList(f)
        | DataType::FixedSizeList(f, _)
        | DataType::Map(f, _) => writable(f.data_type()),
        DataType::Struct(fields) => fields.iter().all(|f| writable(f.data_type())),
        _ => false,
    }
}

/// A text column the database calls a decimal number (PostgreSQL's `numeric` without a
/// precision reaches Arrow as text).
fn numeric_text(field: &Field) -> bool {
    field.metadata().get(META_DB_TYPE).is_some_and(|t| {
        let t = t.to_ascii_lowercase();
        (t.starts_with("numeric") || t.starts_with("decimal")) && !t.contains('[')
    })
}

/// How a column of numbers as text goes into DuckDB: a DECIMAL that holds every value
/// exactly, else DOUBLE; text when one of them is not a number.
fn text_numbers(batches: &[RecordBatch], column: usize) -> Result<Store, ArrowError> {
    let (mut whole, mut fraction, mut special) = (0, 0, false);
    for batch in batches {
        let texts = values::column_texts(batch, column, &FormatOptions::default())?;
        for text in texts.iter().flatten() {
            let digits = text.trim().trim_start_matches(['-', '+']);
            let (int, frac) = digits.split_once('.').unwrap_or((digits, ""));
            let number = !(int.is_empty() && frac.is_empty())
                && int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit());
            if number {
                whole = whole.max(int.trim_start_matches('0').len());
                fraction = fraction.max(frac.len());
            } else if matches!(digits, "NaN" | "Infinity" | "inf") {
                special = true;
            } else {
                return Ok(Store::Text);
            }
        }
    }
    let fits = whole + fraction <= usize::from(MAX_DIGITS);
    Ok(match u8::try_from(fraction) {
        Ok(scale) if fits && !special => Store::Decimal(scale),
        _ => Store::Double,
    })
}

/// The digits of the widest value of a decimal column, at least 1.
/// `None` when a value is a numeric `NaN` (in a field that holds them), which no DECIMAL holds.
fn most_digits(
    field: &Field,
    batches: &[RecordBatch],
    column: usize,
) -> Result<Option<u32>, ArrowError> {
    let nan = special::holds_decimal_nan(field);
    let mut widest = 0u128;
    for batch in batches {
        let values = decimals(batch.column(column))?;
        for v in values.iter().flatten() {
            if nan && v == special::DECIMAL_NAN {
                return Ok(None);
            }
            widest = widest.max(v.unsigned_abs());
        }
    }
    Ok(Some(widest.checked_ilog10().map_or(1, |d| d + 1)))
}

/// A decimal column as `Decimal128`, its scale kept.
fn decimals(array: &ArrayRef) -> Result<Decimal128Array, ArrowError> {
    let scale = match *array.data_type() {
        DataType::Decimal128(_, _) => return Ok(array.as_primitive::<Decimal128Type>().clone()),
        DataType::Decimal32(_, scale) | DataType::Decimal64(_, scale) => scale,
        ref other => {
            return Err(ArrowError::CastError(format!("{other} is not a decimal")));
        }
    };
    let wide = arrow_cast::cast(array, &DataType::Decimal128(MAX_DIGITS, scale))?;
    Ok(wide.as_primitive::<Decimal128Type>().clone())
}

/// Which values of `array` are NULL.
fn nulls_of(array: &ArrayRef) -> ArrayRef {
    Arc::new(BooleanArray::from(
        (0..array.len())
            .map(|r| array.is_null(r))
            .collect::<Vec<_>>(),
    ))
}

/// The values as the text the grid shows.
fn as_text(field: &Field, array: &ArrayRef) -> Result<ArrayRef, LocalError> {
    let texts = values::shown(field, array.as_ref(), &FormatOptions::default())?;
    Ok(Arc::new(StringArray::from(texts)))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use arrow_array::builder::{
        Int32Builder, IntervalMonthDayNanoBuilder, ListBuilder, MapBuilder, StringBuilder,
        StructBuilder,
    };
    use arrow_array::types::{Int32Type, IntervalMonthDayNanoType};
    use arrow_array::{
        ArrayRef, FixedSizeBinaryArray, Int32Array, Int64Array, ListArray, NullArray, RecordBatch,
        StringArray,
    };
    use arrow_schema::{DataType, Field, IntervalUnit, Schema, SchemaRef, TimeUnit};

    use super::*;

    fn people(ids: &[i32]) -> (SchemaRef, RecordBatch) {
        let schema: SchemaRef = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("name", DataType::Utf8, true),
        ]));
        let names: Vec<String> = ids.iter().map(|i| format!("p{i}")).collect();
        let columns: Vec<ArrayRef> = vec![
            Arc::new(Int32Array::from(ids.to_vec())),
            Arc::new(StringArray::from(names)),
        ];
        let batch = RecordBatch::try_new(Arc::clone(&schema), columns).unwrap();
        (schema, batch)
    }

    fn one(conn: &duckdb::Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn loads_batches_as_a_table() {
        platypus_driver_duckdb::testing::load();
        let conn = duckdb::Connection::open_in_memory().unwrap();
        let (schema, first) = people(&[1, 2]);
        let (_, second) = people(&[3]);
        assert_eq!(load(&conn, "result", &schema, &[first, second]).unwrap(), 3);
        assert_eq!(one(&conn, "SELECT count(*) FROM result"), 3);
        assert_eq!(
            one(
                &conn,
                "SELECT sum(id)::BIGINT FROM result WHERE name = 'p2'"
            ),
            2
        );
    }

    #[test]
    fn loading_again_replaces_the_table() {
        platypus_driver_duckdb::testing::load();
        let conn = duckdb::Connection::open_in_memory().unwrap();
        let (schema, batch) = people(&[1, 2, 3]);
        load(&conn, "result", &schema, &[batch]).unwrap();
        let (_, batch) = people(&[9]);
        load(&conn, "result", &schema, &[batch]).unwrap();
        assert_eq!(one(&conn, "SELECT count(*) FROM result"), 1);
        drop_table(&conn, "result").unwrap();
        assert_eq!(
            one(
                &conn,
                "SELECT count(*) FROM duckdb_tables() WHERE table_name = 'result'"
            ),
            0
        );
    }

    #[test]
    fn an_empty_result_is_an_empty_table() {
        platypus_driver_duckdb::testing::load();
        let conn = duckdb::Connection::open_in_memory().unwrap();
        let (schema, _) = people(&[]);
        assert_eq!(load(&conn, "r1", &schema, &[]).unwrap(), 0);
        assert_eq!(one(&conn, "SELECT count(*) FROM r1"), 0);
    }

    #[test]
    fn names_and_types_duckdb_cannot_take() {
        platypus_driver_duckdb::testing::load();
        let conn = duckdb::Connection::open_in_memory().unwrap();
        let schema: SchemaRef = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int32, false),
            Field::new("ID", DataType::Int32, false),
            Field::new("", DataType::Null, true),
        ]));
        let columns: Vec<ArrayRef> = vec![
            Arc::new(Int32Array::from(vec![1])),
            Arc::new(Int32Array::from(vec![2])),
            Arc::new(NullArray::new(1)),
        ];
        let batch = RecordBatch::try_new(Arc::clone(&schema), columns).unwrap();
        load(&conn, "odd", &schema, &[batch]).unwrap();
        let mut stmt = conn
            .prepare("SELECT column_name, data_type FROM duckdb_columns() WHERE table_name = 'odd' ORDER BY column_index")
            .unwrap();
        let columns: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let expected = [
            ("id", "INTEGER"),
            ("ID_2", "INTEGER"),
            ("column3", "VARCHAR"),
        ]
        .map(|(n, t)| (n.to_owned(), t.to_owned()));
        assert_eq!(columns, expected);
    }

    fn table_of(columns: Vec<(&str, ArrayRef)>) -> (SchemaRef, RecordBatch) {
        let fields: Vec<Field> = columns
            .iter()
            .map(|(name, array)| Field::new(*name, array.data_type().clone(), true))
            .collect();
        let schema: SchemaRef = Arc::new(Schema::new(fields));
        let arrays = columns.into_iter().map(|(_, array)| array).collect();
        let batch = RecordBatch::try_new(Arc::clone(&schema), arrays).unwrap();
        (schema, batch)
    }

    fn text(conn: &duckdb::Connection, sql: &str) -> String {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// `[value, NULL]` of `data_type`, cast from a whole number or a text.
    fn value_and_null(data_type: &DataType) -> ArrayRef {
        let from: ArrayRef = match data_type {
            DataType::Date32 | DataType::Time32(_) => {
                Arc::new(Int32Array::from(vec![Some(1), None]))
            }
            DataType::Interval(IntervalUnit::YearMonth) => {
                Arc::new(StringArray::from(vec![Some("1 year"), None]))
            }
            DataType::Interval(_) => Arc::new(StringArray::from(vec![Some("1 day"), None])),
            DataType::LargeUtf8
            | DataType::Utf8View
            | DataType::Binary
            | DataType::LargeBinary
            | DataType::Dictionary(_, _) => Arc::new(StringArray::from(vec![Some("x"), None])),
            _ => Arc::new(Int64Array::from(vec![Some(1), None])),
        };
        arrow_cast::cast(&from, data_type).unwrap()
    }

    #[test]
    fn every_type_keeps_its_nulls() {
        let utc = Some(Arc::from("+00:00"));
        let mut columns: Vec<(String, ArrayRef)> = [
            DataType::Boolean,
            DataType::Int8,
            DataType::Int16,
            DataType::Int32,
            DataType::Int64,
            DataType::UInt8,
            DataType::UInt16,
            DataType::UInt32,
            DataType::UInt64,
            DataType::Float16,
            DataType::Float32,
            DataType::Float64,
            DataType::Decimal32(9, 2),
            DataType::Decimal64(18, 2),
            DataType::Decimal128(10, 2),
            DataType::Decimal256(40, 2),
            DataType::Date32,
            DataType::Date64,
            DataType::Time32(TimeUnit::Second),
            DataType::Time64(TimeUnit::Microsecond),
            DataType::Timestamp(TimeUnit::Second, None),
            DataType::Timestamp(TimeUnit::Millisecond, None),
            DataType::Timestamp(TimeUnit::Microsecond, None),
            DataType::Timestamp(TimeUnit::Nanosecond, None),
            DataType::Timestamp(TimeUnit::Microsecond, utc),
            DataType::Duration(TimeUnit::Millisecond),
            DataType::Duration(TimeUnit::Nanosecond),
            DataType::Interval(IntervalUnit::YearMonth),
            DataType::Interval(IntervalUnit::DayTime),
            DataType::Interval(IntervalUnit::MonthDayNano),
            DataType::Utf8,
            DataType::LargeUtf8,
            DataType::Utf8View,
            DataType::Binary,
            DataType::LargeBinary,
            DataType::Dictionary(Box::new(DataType::Int32), Box::new(DataType::Utf8)),
        ]
        .iter()
        .enumerate()
        .map(|(i, t)| (format!("c{i}"), value_and_null(t)))
        .collect();
        columns.extend(nested_columns());
        let named = columns
            .iter()
            .map(|(n, a)| (n.as_str(), Arc::clone(a)))
            .collect();
        let (schema, batch) = table_of(named);
        let conn = duckdb::Connection::open_in_memory().unwrap();
        assert_eq!(load(&conn, "every", &schema, &[batch]).unwrap(), 2);
        for (name, _) in &columns {
            let c = quote(name);
            let nulls = one(
                &conn,
                &format!("SELECT count(*) FILTER ({c} IS NULL) FROM every"),
            );
            let expected = if name == "nothing" { 2 } else { 1 };
            let data_type = schema.field_with_name(name).unwrap().data_type();
            assert_eq!(nulls, expected, "{name} {data_type}");
        }
        every_type_keeps_its_values(&conn, columns.len());
    }

    /// `[value, NULL]` of binary, nested and NULL types.
    fn nested_columns() -> Vec<(String, ArrayRef)> {
        let mut columns: Vec<(String, ArrayRef)> = Vec::new();
        let bytes = FixedSizeBinaryArray::try_from_sparse_iter_with_size(
            vec![Some(b"ab".as_ref()), None].into_iter(),
            2,
        )
        .unwrap();
        columns.push(("fixed".to_owned(), Arc::new(bytes)));
        let list =
            ListArray::from_iter_primitive::<Int32Type, _, _>(vec![Some(vec![Some(1)]), None]);
        columns.push(("list".to_owned(), Arc::new(list)));
        let mut parts = StructBuilder::from_fields(vec![Field::new("a", DataType::Int32, true)], 2);
        for valid in [true, false] {
            parts
                .field_builder::<Int32Builder>(0)
                .unwrap()
                .append_value(1);
            parts.append(valid);
        }
        columns.push(("struct".to_owned(), Arc::new(parts.finish())));
        let mut map = MapBuilder::new(None, StringBuilder::new(), Int32Builder::new());
        map.keys().append_value("k");
        map.values().append_value(1);
        map.append(true).unwrap();
        map.append(false).unwrap();
        columns.push(("map".to_owned(), Arc::new(map.finish())));
        // An interval NULL inside a list: stored as text, the NULL row stays NULL.
        let mut spans = ListBuilder::new(IntervalMonthDayNanoBuilder::new());
        spans
            .values()
            .append_value(IntervalMonthDayNanoType::make_value(0, 1, 0));
        spans.values().append_null();
        spans.append(true);
        spans.append(false);
        columns.push(("spans".to_owned(), Arc::new(spans.finish())));
        columns.push(("nothing".to_owned(), Arc::new(NullArray::new(2))));
        columns
    }

    /// The intervals of `every_type_keeps_its_nulls` keep their value too, and the columns of
    /// their NULLs are gone.
    fn every_type_keeps_its_values(conn: &duckdb::Connection, width: usize) {
        let day = one(
            conn,
            "SELECT count(*) FROM every WHERE c29 = INTERVAL 1 DAY",
        );
        assert_eq!(day, 1);
        assert_eq!(
            text(conn, "SELECT c25::VARCHAR FROM every WHERE c25 IS NOT NULL"),
            "00:00:00.001"
        );
        assert_eq!(
            text(conn, "SELECT typeof(c29) FROM every LIMIT 1"),
            "INTERVAL"
        );
        // No column of the NULLs is left in the table.
        assert_eq!(
            one(
                conn,
                "SELECT count(*) FROM duckdb_columns() WHERE table_name = 'every'"
            ),
            i64::try_from(width).unwrap()
        );
    }

    #[test]
    fn decimals_wider_than_their_type() {
        // 39 digits: DuckDB's largest HUGEINT, a number wherever it is not a marked NaN.
        const BIG: i128 = i128::MAX;
        let decimal = |values: Vec<Option<i128>>, precision, scale| -> ArrayRef {
            Arc::new(
                Decimal128Array::from(values)
                    .with_precision_and_scale(precision, scale)
                    .unwrap(),
            )
        };
        let (schema, batch) = table_of(vec![
            // A HUGEINT as the DuckDB driver reads it: 39 digits in a DECIMAL(38, 0).
            ("h", decimal(vec![Some(BIG), None, Some(-BIG)], 38, 0)),
            // Digits past the declared precision, within 38.
            ("w", decimal(vec![Some(12_345_678), None, Some(1)], 5, 2)),
            // Past 38 digits with a scale: text.
            ("t", decimal(vec![Some(BIG), None, Some(1)], 38, 2)),
            // A PostgreSQL numeric NaN: DOUBLE.
            (
                "nan",
                decimal(vec![Some(150), None, Some(special::DECIMAL_NAN)], 10, 2),
            ),
            ("n", Arc::new(Int32Array::from(vec![1, 2, 3]))),
        ]);
        // The NaN column as the PostgreSQL driver marks it.
        let fields: Vec<Field> = schema
            .fields()
            .iter()
            .map(|f| {
                let f = f.as_ref().clone();
                if f.name() == "nan" {
                    special::with_decimal_nan(f)
                } else {
                    f
                }
            })
            .collect();
        let schema: SchemaRef = Arc::new(Schema::new(fields));
        let batch = RecordBatch::try_new(Arc::clone(&schema), batch.columns().to_vec()).unwrap();
        let conn = duckdb::Connection::open_in_memory().unwrap();
        load(&conn, "wide", &schema, &[batch]).unwrap();
        assert_eq!(one(&conn, "SELECT sum(n)::BIGINT FROM wide"), 6);
        assert_eq!(
            text(&conn, "SELECT max(h)::VARCHAR FROM wide"),
            BIG.to_string()
        );
        assert_eq!(
            text(&conn, "SELECT min(h)::VARCHAR FROM wide"),
            (-BIG).to_string()
        );
        assert_eq!(text(&conn, "SELECT typeof(h) FROM wide LIMIT 1"), "HUGEINT");
        assert_eq!(
            text(&conn, "SELECT typeof(nan) FROM wide LIMIT 1"),
            "DOUBLE"
        );
        assert_eq!(
            one(&conn, "SELECT count(*) FILTER (isnan(nan)) FROM wide"),
            1
        );
        assert_eq!(
            one(&conn, "SELECT count(*) FILTER (nan = 1.5) FROM wide"),
            1
        );
        assert_eq!(text(&conn, "SELECT max(w)::VARCHAR FROM wide"), "123456.78");
        assert_eq!(
            text(&conn, "SELECT typeof(w) FROM wide LIMIT 1"),
            "DECIMAL(38,2)"
        );
        assert_eq!(
            text(&conn, "SELECT t FROM wide WHERE n = 1"),
            "1701411834604692317316873037158841057.27"
        );
        for c in ["h", "w", "t", "nan"] {
            let nulls = one(
                &conn,
                &format!("SELECT count(*) FILTER ({c} IS NULL) FROM wide"),
            );
            assert_eq!(nulls, 1, "{c}");
        }
    }

    #[test]
    fn numbers_sent_as_text_are_numbers() {
        let numeric = |name: &str| {
            Field::new(name, DataType::Utf8, true).with_metadata(HashMap::from([(
                META_DB_TYPE.to_owned(),
                "numeric".to_owned(),
            )]))
        };
        let long = format!("1{}", "0".repeat(44));
        let schema: SchemaRef = Arc::new(Schema::new(vec![
            numeric("exact"),
            numeric("nan"),
            numeric("long"),
            numeric("odd"),
            Field::new("plain", DataType::Utf8, true),
        ]));
        let columns: Vec<ArrayRef> = vec![
            Arc::new(StringArray::from(vec![
                Some("4.5"),
                Some("30.0"),
                Some("-2"),
                None,
            ])),
            Arc::new(StringArray::from(vec![
                Some("1"),
                Some("NaN"),
                Some("-Infinity"),
                None,
            ])),
            Arc::new(StringArray::from(vec![
                Some(long.as_str()),
                Some("2"),
                Some("1.5"),
                None,
            ])),
            Arc::new(StringArray::from(vec![
                Some("1"),
                Some("x"),
                Some("2"),
                None,
            ])),
            Arc::new(StringArray::from(vec![
                Some("10"),
                Some("9"),
                Some("2"),
                None,
            ])),
        ];
        let batch = RecordBatch::try_new(Arc::clone(&schema), columns).unwrap();
        let conn = duckdb::Connection::open_in_memory().unwrap();
        load(&conn, "n", &schema, &[batch]).unwrap();
        let type_of = |c: &str| text(&conn, &format!("SELECT typeof({c}) FROM n LIMIT 1"));
        assert_eq!(type_of("exact"), "DECIMAL(38,1)");
        assert_eq!(type_of("nan"), "DOUBLE");
        assert_eq!(type_of("long"), "DOUBLE");
        assert_eq!(type_of("odd"), "VARCHAR");
        assert_eq!(type_of("plain"), "VARCHAR");
        assert_eq!(
            text(
                &conn,
                "SELECT exact::VARCHAR FROM n ORDER BY exact DESC NULLS LAST LIMIT 1"
            ),
            "30.0"
        );
        assert_eq!(
            one(
                &conn,
                "SELECT count(*) FILTER (exact IS NULL AND nan IS NULL) FROM n"
            ),
            1
        );
    }

    #[test]
    fn unique_names_ignore_case() {
        platypus_driver_duckdb::testing::load();
        let names = ["a", "A", "a", ""].map(String::from);
        assert_eq!(unique_names(&names), ["a", "A_2", "a_3", "column4"]);
        assert_eq!(quote("we\"ird"), "\"we\"\"ird\"");
    }
}
