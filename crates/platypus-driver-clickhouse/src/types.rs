//! Column types. ClickHouse's Arrow output drops what the type means for `DateTime`
//! (plain `UInt32` seconds) and fails on types Arrow lacks (`UUID`, `JSON`, ...). Queries
//! are therefore described first: the declared types label the fields, turn `DateTime`
//! back into timestamps, and the types Arrow lacks or garbles are selected as text.

use std::sync::Arc;

use arrow_array::cast::AsArray;
use arrow_array::types::UInt32Type;
use arrow_array::{Array, ArrayRef, RecordBatch, TimestampSecondArray};
use arrow_schema::{DataType, Field, Schema, SchemaRef, TimeUnit};
use platypus_core::{META_DB_TYPE, Result};
use platypus_sql::{Dialect, Lexer, TokenKind};

/// Arrow cannot parse named zones without `chrono-tz`: timestamps are labelled UTC, the
/// instants are unchanged.
const UTC: &str = "+00:00";

/// Types selected as text: ClickHouse cannot write them as Arrow (`UUID`, `JSON`, ...)
/// or writes their bare storage (enum codes, IP addresses as numbers, 128- and 256-bit
/// integers and fixed strings as raw bytes).
const AS_TEXT: &[&str] = &[
    "UUID",
    "JSON",
    "Object(",
    "Interval",
    "Dynamic",
    "Variant(",
    "BFloat16",
    "Enum8(",
    "Enum16(",
    "IPv4",
    "IPv6",
    "Int128",
    "Int256",
    "FixedString(",
];

/// Whether `sql` is a query `DESCRIBE TABLE (...)` can describe.
pub fn describable(sql: &str) -> bool {
    let first = Lexer::new(sql, Dialect::ClickHouse).find(|t| !t.is_trivia());
    first.is_some_and(|t| match t.kind {
        TokenKind::Word => ["SELECT", "WITH", "FROM"]
            .iter()
            .any(|w| t.text(sql).eq_ignore_ascii_case(w)),
        TokenKind::Symbol => t.text(sql) == "(",
        _ => false,
    })
}

/// The statement that describes the columns of `sql`, one JSON array per line.
pub fn describe_sql(sql: &str) -> String {
    format!("DESCRIBE TABLE ({sql}) FORMAT JSONCompactEachRow")
}

/// `(name, type)` of each column, from the output of [`describe_sql`].
pub fn parse_describe(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let row: Vec<serde_json::Value> = serde_json::from_str(line).ok()?;
            Some((
                row.first()?.as_str()?.to_owned(),
                row.get(1)?.as_str()?.to_owned(),
            ))
        })
        .collect()
}

fn as_text(db_type: &str) -> bool {
    AS_TEXT.iter().any(|t| db_type.contains(t))
}

/// `sql` with the columns of [`AS_TEXT`] types converted to text, or `None` when none needs it.
pub fn arrow_compatible(sql: &str, columns: &[(String, String)]) -> Option<String> {
    if !columns.iter().any(|(_, t)| as_text(t)) {
        return None;
    }
    let d = Dialect::ClickHouse;
    let list: Vec<String> = columns
        .iter()
        .map(|(name, db_type)| {
            let q = d.quote_ident(name);
            if as_text(db_type) {
                format!("toString({q}) AS {q}")
            } else {
                q.into_owned()
            }
        })
        .collect();
    Some(format!("SELECT {} FROM ({sql})", list.join(", ")))
}

/// The type inside `Nullable(...)` and `LowCardinality(...)`.
fn base_type(db_type: &str) -> &str {
    let mut t = db_type;
    for wrapper in ["LowCardinality(", "Nullable("] {
        if let Some(inner) = t.strip_prefix(wrapper).and_then(|r| r.strip_suffix(')')) {
            t = inner;
        }
    }
    t
}

/// Maps the decoded batches to their final fields: database types in the metadata,
/// `DateTime` as timestamps, every timestamp labelled UTC.
#[derive(Debug, Clone)]
pub struct Adapter {
    schema: SchemaRef,
    /// Columns that hold `DateTime` seconds as `UInt32`.
    seconds: Vec<usize>,
}

impl Adapter {
    /// For the decoded `schema` and the described column types, when they line up.
    pub fn new(schema: &Schema, types: &[(String, String)]) -> Self {
        let types = (types.len() == schema.fields().len()).then_some(types);
        let mut seconds = Vec::new();
        let fields: Vec<Field> = schema
            .fields()
            .iter()
            .enumerate()
            .map(|(i, field)| {
                let db_type = types.map(|t| t[i].1.as_str());
                let data_type = match (field.data_type(), db_type.map(base_type)) {
                    (DataType::UInt32, Some(t)) if t.starts_with("DateTime") => {
                        seconds.push(i);
                        DataType::Timestamp(TimeUnit::Second, Some(UTC.into()))
                    }
                    (DataType::Timestamp(unit, Some(_)), _) => {
                        DataType::Timestamp(*unit, Some(UTC.into()))
                    }
                    (other, _) => other.clone(),
                };
                let mut metadata = field.metadata().clone();
                if let Some(t) = db_type {
                    metadata.insert(META_DB_TYPE.to_owned(), t.to_owned());
                }
                Field::new(field.name(), data_type, field.is_nullable()).with_metadata(metadata)
            })
            .collect();
        Self {
            schema: Arc::new(Schema::new_with_metadata(fields, schema.metadata().clone())),
            seconds,
        }
    }

    pub fn schema(&self) -> SchemaRef {
        Arc::clone(&self.schema)
    }

    pub fn apply(&self, batch: &RecordBatch) -> Result<RecordBatch> {
        let columns: Vec<ArrayRef> = batch
            .columns()
            .iter()
            .enumerate()
            .map(|(i, column)| self.column(i, column))
            .collect();
        Ok(RecordBatch::try_new(self.schema(), columns)?)
    }

    fn column(&self, i: usize, column: &ArrayRef) -> ArrayRef {
        let target = self.schema.field(i).data_type();
        if self.seconds.contains(&i) {
            let seconds = column.as_primitive::<UInt32Type>();
            let values: TimestampSecondArray = seconds.unary(i64::from);
            return Arc::new(values.with_timezone(UTC));
        }
        if column.data_type() == target {
            return Arc::clone(column);
        }
        // Only the time zone label differs: same buffers, new type.
        let data = column.to_data().into_builder().data_type(target.clone());
        arrow_array::make_array(data.build().unwrap_or_else(|_| column.to_data()))
    }
}

#[cfg(test)]
mod tests {
    use arrow_array::{TimestampMillisecondArray, UInt32Array};

    use super::*;

    #[test]
    fn describes_only_queries() {
        assert!(describable("  -- c\n select 1"));
        assert!(describable("WITH x AS (SELECT 1) SELECT * FROM x"));
        assert!(describable("(SELECT 1) UNION ALL (SELECT 2)"));
        assert!(!describable("INSERT INTO t SELECT 1"));
        assert!(!describable("SHOW TABLES"));
        assert_eq!(
            parse_describe("[\"id\",\"UInt8\",\"\",\"\"]\n[\"u\",\"Nullable(UUID)\"]\n"),
            [
                ("id".to_owned(), "UInt8".to_owned()),
                ("u".to_owned(), "Nullable(UUID)".to_owned())
            ]
        );
    }

    #[test]
    fn selects_types_arrow_lacks_as_text() {
        let columns = vec![
            ("id".to_owned(), "UInt8".to_owned()),
            ("my id".to_owned(), "Array(UUID)".to_owned()),
        ];
        assert_eq!(
            arrow_compatible("SELECT * FROM t", &columns).as_deref(),
            Some("SELECT id, toString(`my id`) AS `my id` FROM (SELECT * FROM t)")
        );
        assert_eq!(arrow_compatible("SELECT 1", &columns[..1]), None);
    }

    #[test]
    fn turns_datetime_seconds_into_timestamps() {
        let schema = Schema::new(vec![
            Field::new("at", DataType::UInt32, true),
            Field::new(
                "ms",
                DataType::Timestamp(TimeUnit::Millisecond, Some("Europe/Rome".into())),
                false,
            ),
            Field::new("n", DataType::UInt32, false),
        ]);
        let types = vec![
            ("at".to_owned(), "Nullable(DateTime('UTC'))".to_owned()),
            ("ms".to_owned(), "DateTime64(3, 'Europe/Rome')".to_owned()),
            ("n".to_owned(), "UInt32".to_owned()),
        ];
        let adapter = Adapter::new(&schema, &types);
        let batch = RecordBatch::try_new(
            Arc::new(schema),
            vec![
                Arc::new(UInt32Array::from(vec![Some(1_709_287_200), None])),
                Arc::new(TimestampMillisecondArray::from(vec![1, 2]).with_timezone("Europe/Rome")),
                Arc::new(UInt32Array::from(vec![5, 6])),
            ],
        )
        .unwrap();
        let out = adapter.apply(&batch).unwrap();
        let fields = out.schema();
        assert_eq!(
            fields.field(0).data_type(),
            &DataType::Timestamp(TimeUnit::Second, Some(UTC.into()))
        );
        assert_eq!(
            fields.field(1).data_type(),
            &DataType::Timestamp(TimeUnit::Millisecond, Some(UTC.into()))
        );
        assert_eq!(fields.field(2).data_type(), &DataType::UInt32);
        assert_eq!(
            fields
                .field(2)
                .metadata()
                .get(META_DB_TYPE)
                .map(String::as_str),
            Some("UInt32")
        );
        assert!(out.column(0).is_null(1));
        let at = arrow_cast::display::array_value_to_string(out.column(0), 0).unwrap();
        assert_eq!(at, "2024-03-01T10:00:00Z");
    }
}
