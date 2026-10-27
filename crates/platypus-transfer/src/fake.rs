//! Fake rows for a table: each column filled by a generator (names, emails, numbers, dates,
//! values of an enum), foreign keys with values the referenced table has, so that the rows
//! fit their constraints. The rows are written by [`crate::import::import_rows`], or as
//! mongosh `insertMany` statements into a MongoDB collection by [`fill_collection`].

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use arrow_array::builder::{
    BooleanBuilder, Date32Builder, Float64Builder, Int64Builder, StringBuilder,
    TimestampMicrosecondBuilder,
};
use arrow_array::cast::AsArray;
use arrow_array::types::{Date32Type, Float64Type, Int64Type, TimestampMicrosecondType};
use arrow_array::{Array, ArrayRef, RecordBatch};
use arrow_schema::{ArrowError, DataType, Field, Schema, SchemaRef, TimeUnit};
use futures::StreamExt;
use platypus_core::{Connection, ExecOptions, RecordBatchStream};
use platypus_sql::Dialect;
use serde::Deserialize;

use crate::import::{
    ImportColumn, ImportError, ImportMode, ImportPlan, import_rows, query_rows, query_value,
    text_length,
};

/// Rows per generated batch.
const BATCH_ROWS: usize = 500;

/// Distinct values read from a referenced table, at most.
const REFERENCE_VALUES: u64 = 10_000;

/// How a column is filled.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Generator {
    /// Whole numbers in `min..=max`.
    Integer {
        min: i64,
        max: i64,
    },
    /// Numbers in `min..max` with `scale` digits after the point.
    Decimal {
        min: f64,
        max: f64,
        scale: u32,
    },
    /// `start`, `start + 1`, ...: a key no row has yet.
    Sequence {
        start: i64,
    },
    Bool,
    FirstName,
    LastName,
    FullName,
    Email,
    City,
    Phone,
    /// A few words of filler text.
    Text,
    /// Days within the last ten years.
    Date,
    /// Times within the last ten years.
    Timestamp,
    /// A time of day, `HH:MM:SS`.
    Time,
    /// An IPv4 address of a private network.
    Ip,
    Uuid,
    /// A new MongoDB `ObjectId`, as its 24 hex digits.
    ObjectId,
    /// A JSON array of a few words: for an array field.
    WordList,
    /// A JSON object with a name, a city and a phone number: for a document field.
    Document,
    /// One of `values`: an enum's, or the keys a foreign key may point at.
    Choice {
        values: Vec<String>,
    },
    Null,
    /// The integers after the column's largest: resolved to a [`Generator::Sequence`].
    NextKey,
    /// Values `column` of `table` has, for a foreign key: resolved to a [`Generator::Choice`].
    /// The columns of a composite foreign key share its `key` (the name), and take their
    /// values from one referenced row.
    Reference {
        schema: Option<String>,
        table: String,
        column: String,
        #[serde(default)]
        key: Option<String>,
    },
    /// Column `part` of one of `rows`, the same row for each column of a composite foreign
    /// key: picked by row number and `seed`, which they share.
    #[serde(skip)]
    Tuple {
        rows: Arc<Vec<Vec<String>>>,
        part: usize,
        seed: u64,
    },
    /// `values` in turn, each once: a unique foreign key's keys no row uses yet.
    #[serde(skip)]
    Distinct {
        values: Vec<String>,
    },
    /// The text of `inner` made unique with `token` (this run's) and the row number.
    #[serde(skip)]
    Unique {
        inner: Box<Generator>,
        token: String,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FakeColumn {
    pub name: String,
    pub generator: Generator,
    /// The column takes no value twice (a unique index or constraint of its own).
    #[serde(default)]
    pub unique: bool,
    /// The column's type as the database names it: for a MongoDB field, the BSON type of the
    /// sampled documents (`long`, `double`, `decimal`…), which the written numbers take.
    #[serde(default)]
    pub data_type: Option<String>,
}

const FIRST: &[&str] = &[
    "Ada",
    "Alan",
    "Grace",
    "Linus",
    "Margaret",
    "Dennis",
    "Barbara",
    "Ken",
    "Frances",
    "Edsger",
    "Radia",
    "Donald",
    "Hedy",
    "Tim",
    "Katherine",
    "Guido",
    "Lucia",
    "Marco",
    "Giulia",
    "Paolo",
];
const LAST: &[&str] = &[
    "Lovelace",
    "Turing",
    "Hopper",
    "Torvalds",
    "Hamilton",
    "Ritchie",
    "Liskov",
    "Thompson",
    "Allen",
    "Dijkstra",
    "Perlman",
    "Knuth",
    "Lamarr",
    "Berners-Lee",
    "Johnson",
    "van Rossum",
    "Rossi",
    "Bianchi",
    "Ferrari",
    "Esposito",
];
const CITIES: &[&str] = &[
    "Milano", "Roma", "Torino", "Napoli", "Bologna", "London", "Paris", "Berlin", "Madrid",
    "Lisboa", "Wien", "Zürich", "New York", "Toronto", "Tokyo", "Sydney",
];
const WORDS: &[&str] = &[
    "lorem",
    "ipsum",
    "dolor",
    "sit",
    "amet",
    "consectetur",
    "adipiscing",
    "elit",
    "sed",
    "do",
    "eiusmod",
    "tempor",
    "incididunt",
    "ut",
    "labore",
    "et",
    "dolore",
    "magna",
    "aliqua",
];
const DOMAINS: &[&str] = &["example.com", "example.org", "example.net"];

/// Days in ten years, the span of generated dates.
const SPAN_DAYS: i64 = 3653;

/// xorshift64*: plenty for filler data, no dependency.
#[derive(Debug)]
pub struct Rng(u64);

impl Rng {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    /// Seeded from the clock: each run differs.
    #[must_use]
    pub fn from_clock() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        #[expect(clippy::cast_possible_truncation, reason = "any bits make a seed")]
        Self::new(nanos as u64)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A number in `0..n`; `n` must not be 0.
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let i = usize::try_from(self.below(items.len() as u64)).unwrap_or(0);
        &items[i]
    }

    #[expect(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "the span fits, and wraps back into range"
    )]
    fn between(&mut self, min: i64, max: i64) -> i64 {
        let span = max.saturating_sub(min) as u64 + 1;
        if span == 0 {
            return self.next() as i64;
        }
        min.wrapping_add(self.below(span) as i64)
    }

    #[expect(clippy::cast_precision_loss, reason = "a fraction of the range")]
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Six letters and digits, to tell this run's unique values from earlier runs'.
    fn token(&mut self) -> String {
        let mut n = self.next();
        (0..6)
            .map(|_| {
                let digit = u32::try_from(n % 36).unwrap_or(0);
                n /= 36;
                char::from_digit(digit, 36).unwrap_or('0')
            })
            .collect()
    }
}

/// splitmix64's finalizer: the same well-spread number for the same input.
fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

impl Generator {
    /// The Arrow type of the generated values; the target converts text to its column type.
    fn data_type(&self) -> DataType {
        match self {
            Self::Integer { .. } | Self::Sequence { .. } => DataType::Int64,
            Self::Decimal { .. } => DataType::Float64,
            Self::Bool => DataType::Boolean,
            Self::Date => DataType::Date32,
            Self::Timestamp => DataType::Timestamp(TimeUnit::Microsecond, None),
            _ => DataType::Utf8,
        }
    }
}

/// The schema of the generated batches.
#[must_use]
pub fn schema(columns: &[FakeColumn]) -> SchemaRef {
    Arc::new(Schema::new(
        columns
            .iter()
            .map(|c| Field::new(&c.name, c.generator.data_type(), true))
            .collect::<Vec<_>>(),
    ))
}

fn words(rng: &mut Rng, n: u64) -> Vec<&'static str> {
    (0..n).map(|_| *rng.pick(WORDS)).collect()
}

/// An `ObjectId` as MongoDB makes one: the time in seconds, five random bytes and a counter,
/// here the row number, which keeps the run's ids apart.
fn object_id(rng: &mut Rng, row: u64) -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
        & 0xFFFF_FFFF;
    let random = rng.next() & 0xFF_FFFF_FFFF;
    format!("{seconds:08x}{random:010x}{:06x}", row & 0xFF_FFFF)
}

fn text(rng: &mut Rng, generator: &Generator, row: u64) -> Option<String> {
    Some(match generator {
        Generator::FirstName => (*rng.pick(FIRST)).to_owned(),
        Generator::LastName => (*rng.pick(LAST)).to_owned(),
        Generator::FullName => format!("{} {}", rng.pick(FIRST), rng.pick(LAST)),
        Generator::Email => format!(
            "{}.{}{}@{}",
            rng.pick(FIRST).to_lowercase(),
            rng.pick(LAST).to_lowercase().replace(' ', ""),
            // The row number keeps addresses apart, for unique columns.
            row,
            rng.pick(DOMAINS)
        ),
        Generator::City => (*rng.pick(CITIES)).to_owned(),
        Generator::Phone => format!("+39 3{:02} {:07}", rng.below(100), rng.below(10_000_000)),
        Generator::Text => {
            let n = 3 + rng.below(6);
            words(rng, n).join(" ")
        }
        Generator::WordList => {
            let n = 1 + rng.below(4);
            serde_json::Value::from(words(rng, n)).to_string()
        }
        Generator::Document => serde_json::json!({
            "name": text(rng, &Generator::FullName, row)?,
            "city": text(rng, &Generator::City, row)?,
            "phone": text(rng, &Generator::Phone, row)?,
        })
        .to_string(),
        Generator::ObjectId => object_id(rng, row),
        Generator::Time => format!(
            "{:02}:{:02}:{:02}",
            rng.below(24),
            rng.below(60),
            rng.below(60)
        ),
        Generator::Ip => format!(
            "10.{}.{}.{}",
            rng.below(256),
            rng.below(256),
            1 + rng.below(254)
        ),
        Generator::Uuid => {
            let (a, b) = (rng.next(), rng.next());
            // Version 4, variant 1.
            let a = (a & !0xF000) | 0x4000;
            let b = (b & !(0b11 << 62)) | (0b10 << 62);
            format!(
                "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
                a >> 32,
                (a >> 16) & 0xFFFF,
                a & 0xFFFF,
                b >> 48,
                b & 0xFFFF_FFFF_FFFF
            )
        }
        Generator::Choice { values } if !values.is_empty() => rng.pick(values).clone(),
        Generator::Tuple { rows, part, seed } if !rows.is_empty() => {
            let i = usize::try_from(mix(seed ^ row) % rows.len() as u64).unwrap_or(0);
            rows[i][*part].clone()
        }
        Generator::Distinct { values } => values.get(usize::try_from(row).ok()?)?.clone(),
        Generator::Unique { inner, token } => {
            let v = text(rng, inner, row)?;
            match v.split_once('@') {
                Some((name, domain)) => format!("{name}.{token}{row}@{domain}"),
                None => format!("{v} {token}{row}"),
            }
        }
        _ => return None,
    })
}

/// `value` in at most `limit` characters: its start, or with `end` its end; no space left
/// at the edge.
fn cut(value: String, limit: usize, end: bool) -> String {
    let count = value.chars().count();
    if count <= limit {
        return value;
    }
    let kept: String = if end {
        value.chars().skip(count - limit).collect()
    } else {
        value.chars().take(limit).collect()
    };
    kept.trim().to_owned()
}

/// `rows` rows of `columns`, the first numbered `first` (for sequences and unique values).
pub fn batch(
    rng: &mut Rng,
    columns: &[FakeColumn],
    first: u64,
    rows: usize,
) -> Result<RecordBatch, ArrowError> {
    // The number of the last row, added to a sequence's start.
    let last = i64::try_from((first + rows as u64).saturating_sub(1)).ok();
    let beyond = |c: &FakeColumn| {
        matches!(c.generator, Generator::Sequence { start }
            if last.and_then(|l| start.checked_add(l)).is_none())
    };
    if let Some(c) = columns.iter().find(|c| beyond(c)) {
        return Err(ArrowError::ComputeError(format!(
            "the keys of {} go beyond the largest integer",
            c.name
        )));
    }
    let today = days_since_epoch();
    let arrays: Vec<ArrayRef> = columns
        .iter()
        .map(|c| -> ArrayRef {
            let numbers = first..first + rows as u64;
            match &c.generator {
                Generator::Integer { min, max } => {
                    let mut b = Int64Builder::with_capacity(rows);
                    numbers.for_each(|_| b.append_value(rng.between(*min, (*max).max(*min))));
                    Arc::new(b.finish())
                }
                Generator::Sequence { start } => {
                    let mut b = Int64Builder::with_capacity(rows);
                    #[expect(clippy::cast_possible_wrap, reason = "row numbers fit in i64")]
                    numbers.for_each(|n| b.append_value(start.wrapping_add(n as i64)));
                    Arc::new(b.finish())
                }
                Generator::Decimal { min, max, scale } => {
                    let mut b = Float64Builder::with_capacity(rows);
                    let factor = 10f64.powi(i32::try_from(*scale).unwrap_or(2).min(6));
                    numbers.for_each(|_| {
                        let v = min + (max - min) * rng.unit();
                        b.append_value((v * factor).round() / factor);
                    });
                    Arc::new(b.finish())
                }
                Generator::Bool => {
                    let mut b = BooleanBuilder::with_capacity(rows);
                    numbers.for_each(|_| b.append_value(rng.below(2) == 1));
                    Arc::new(b.finish())
                }
                Generator::Date => {
                    let mut b = Date32Builder::with_capacity(rows);
                    numbers.for_each(|_| {
                        let day = today - rng.between(0, SPAN_DAYS);
                        b.append_value(i32::try_from(day).unwrap_or(0));
                    });
                    Arc::new(b.finish())
                }
                Generator::Timestamp => {
                    let mut b = TimestampMicrosecondBuilder::with_capacity(rows);
                    numbers.for_each(|_| {
                        let seconds =
                            (today - rng.between(0, SPAN_DAYS)) * 86_400 + rng.between(0, 86_399);
                        b.append_value(seconds * 1_000_000);
                    });
                    Arc::new(b.finish())
                }
                other => {
                    // A text of a bounded length (`varchar(5)`) is cut to it; a unique one
                    // keeps its end, which tells it apart.
                    let limit = c
                        .data_type
                        .as_deref()
                        .and_then(text_length)
                        .and_then(|n| usize::try_from(n).ok());
                    let unique = matches!(other, Generator::Unique { .. });
                    let mut b = StringBuilder::with_capacity(rows, rows * 16);
                    numbers.for_each(|n| {
                        let value = text(rng, other, n);
                        b.append_option(match limit {
                            Some(limit) => value.map(|v| cut(v, limit, unique)),
                            None => value,
                        });
                    });
                    Arc::new(b.finish())
                }
            }
        })
        .collect();
    RecordBatch::try_new(schema(columns), arrays)
}

/// The error of foreign key column `name`, whose referenced `table` has no rows.
fn no_keys(name: &str, table: &str) -> ImportError {
    ImportError::Plan(format!(
        "{name} refers to {table}, which has no rows to point at: fill it first"
    ))
}

/// Whether the table a foreign key references, `(schema, table)`, is `own`, the one filled.
/// A schema left out is the session's, whichever it names.
fn is_own(referenced: (Option<&str>, &str), own: (Option<&str>, &str)) -> bool {
    fn named(s: Option<&str>) -> Option<&str> {
        s.filter(|s| !s.is_empty())
    }
    referenced.1 == own.1
        && match (named(referenced.0), named(own.0)) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
}

/// The keys a foreign key can take, at most [`REFERENCE_VALUES`]: the rows of `columns` of
/// `table` without NULL, `filter` added to their `WHERE`. None when it has no rows.
async fn keys(
    conn: &dyn Connection,
    dialect: Dialect,
    (schema, table): (Option<&str>, &str),
    columns: &[&str],
    filter: &str,
) -> Result<Vec<Vec<String>>, ImportError> {
    let names: Vec<_> = columns.iter().map(|c| dialect.quote_ident(c)).collect();
    let not_null: Vec<_> = names.iter().map(|q| format!("{q} IS NOT NULL")).collect();
    let select = format!(
        "SELECT DISTINCT {} FROM {} WHERE {}{filter}",
        names.join(", "),
        dialect.qualified(schema, table),
        not_null.join(" AND ")
    );
    let rows = query_rows(conn, &dialect.limited(&select, REFERENCE_VALUES)).await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.into_iter().collect())
        .collect())
}

/// Turns the columns of each composite foreign key (the references with one `key`) into
/// [`Generator::Tuple`]s over rows of the referenced table.
async fn composite_keys(
    (conn, dialect): (&dyn Connection, Dialect),
    own: (Option<&str>, &str),
    columns: &mut [FakeColumn],
    rng: &mut Rng,
) -> Result<(), ImportError> {
    let key_of = |c: &FakeColumn| match &c.generator {
        Generator::Reference { key: Some(k), .. } => Some(k.clone()),
        _ => None,
    };
    let mut names: Vec<String> = columns.iter().filter_map(key_of).collect();
    names.sort();
    names.dedup();
    for name in names {
        let parts: Vec<usize> = (0..columns.len())
            .filter(|&i| key_of(&columns[i]).as_ref() == Some(&name))
            .collect();
        if parts.len() < 2 {
            continue;
        }
        let mut target = None;
        let mut referenced = Vec::new();
        for &i in &parts {
            if let Generator::Reference {
                schema,
                table,
                column,
                ..
            } = &columns[i].generator
            {
                target = Some((schema.clone(), table.clone()));
                referenced.push(column.clone());
            }
        }
        let Some((schema, table)) = target else {
            continue;
        };
        let referenced: Vec<&str> = referenced.iter().map(String::as_str).collect();
        let at = (schema.as_deref(), table.as_str());
        let rows = keys(conn, dialect, at, &referenced, "").await?;
        if rows.is_empty() {
            // The first rows of a table that refers to itself point at none.
            if !is_own(at, own) {
                let names: Vec<&str> = parts.iter().map(|&i| columns[i].name.as_str()).collect();
                return Err(no_keys(&names.join(", "), &table));
            }
            for &i in &parts {
                columns[i].generator = Generator::Null;
            }
            continue;
        }
        let rows = Arc::new(rows);
        let seed = rng.next();
        for (part, i) in parts.into_iter().enumerate() {
            columns[i].generator = Generator::Tuple {
                rows: Arc::clone(&rows),
                part,
                seed,
            };
        }
    }
    Ok(())
}

/// 2^63, the first `f64` beyond `i64`.
const I64_LIMIT: f64 = 9_223_372_036_854_775_808.0;

/// The largest whole number at most `m`, a number as the database writes it: `None` for text
/// that is not a number, an error for a number beyond `i64`.
fn whole(m: &str) -> Result<Option<i64>, ()> {
    let m = m.trim();
    if let Ok(n) = m.parse::<i64>() {
        return Ok(Some(n));
    }
    // `123.45` exactly, beyond what an f64 holds.
    if let Some((int, fraction)) = m.split_once('.')
        && !fraction.is_empty()
        && fraction.bytes().all(|b| b.is_ascii_digit())
        && let Ok(n) = int.parse::<i64>()
    {
        let below = m.starts_with('-') && fraction.bytes().any(|b| b != b'0');
        return if below {
            n.checked_sub(1).ok_or(()).map(Some)
        } else {
            Ok(Some(n))
        };
    }
    let Ok(f) = m.parse::<f64>() else {
        return Ok(None);
    };
    if !f.is_finite() || f.floor() < -I64_LIMIT || f.floor() >= I64_LIMIT {
        return Err(());
    }
    #[expect(clippy::cast_possible_truncation, reason = "within i64, checked above")]
    Ok(Some(f.floor() as i64))
}

/// The keys after `max`, the largest of column `name`, for `rows` rows; from 1 without one.
fn next_key(name: &str, max: Option<&str>, rows: u64) -> Result<Generator, ImportError> {
    let beyond = || {
        ImportError::Plan(format!(
            "{name}: {rows} keys after the largest go beyond the largest integer"
        ))
    };
    let largest = match max.map(whole) {
        Some(Err(())) => return Err(beyond()),
        Some(Ok(n)) => n.unwrap_or(0),
        None => 0,
    };
    let last = i64::try_from(rows)
        .ok()
        .and_then(|r| largest.checked_add(r));
    match (largest.checked_add(1), last) {
        (Some(start), Some(_)) => Ok(Generator::Sequence { start }),
        _ => Err(beyond()),
    }
}

/// Refuses the settings no value comes from: numbers between bounds beyond `f64`.
fn check(column: &FakeColumn) -> Result<(), ImportError> {
    match column.generator {
        Generator::Decimal { min, max, .. }
            if !(min.is_finite() && max.is_finite() && (max - min).is_finite()) =>
        {
            Err(ImportError::Plan(format!(
                "{}: the bounds of its numbers are too large",
                column.name
            )))
        }
        _ => Ok(()),
    }
}

/// The generator `column` stands for, read from the table's database where it says so;
/// `rows` and `token` (this run's) keep the values of a unique column apart.
async fn resolve(
    (conn, dialect): (&dyn Connection, Dialect),
    (own, table, rows): ((Option<&str>, &str), &str, u64),
    column: FakeColumn,
    token: &str,
) -> Result<Generator, ImportError> {
    check(&column)?;
    let name = column.name.as_str();
    let generator = match column.generator {
        Generator::NextKey => {
            let sql = format!("SELECT MAX({}) FROM {table}", dialect.quote_ident(name));
            next_key(name, query_value(conn, &sql).await?.as_deref(), rows)?
        }
        Generator::Reference {
            schema,
            table: referenced,
            column: key,
            ..
        } => {
            let at = (schema.as_deref(), referenced.as_str());
            // A unique foreign key takes each key no row has yet, once.
            let unused = format!(
                " AND {} NOT IN (SELECT {} FROM {table} WHERE {1} IS NOT NULL)",
                dialect.quote_ident(&key),
                dialect.quote_ident(name)
            );
            let filter = if column.unique { unused.as_str() } else { "" };
            let values: Vec<String> = keys(conn, dialect, at, &[&key], filter)
                .await?
                .into_iter()
                .map(|mut r| r.swap_remove(0))
                .collect();
            if values.is_empty() {
                // The first rows of a table that refers to itself point at none.
                if !is_own(at, own) {
                    return Err(no_keys(name, &referenced));
                }
                Generator::Null
            } else if !column.unique {
                return Ok(Generator::Choice { values });
            } else if (values.len() as u64) < rows {
                return Err(ImportError::Plan(format!(
                    "{name} is unique, and only {} keys of {referenced} are not used yet",
                    values.len()
                )));
            } else {
                Generator::Distinct { values }
            }
        }
        other => other,
    };
    if !column.unique {
        return Ok(generator);
    }
    // SQL Server's unique indexes take one NULL.
    let null_once = (dialect == Dialect::SqlServer).then_some("SQL Server");
    distinct(generator, (name, rows), token, null_once)
}

/// `generator` made to take no value twice in `rows` rows of the unique column `name`: text
/// gets `token` (this run's) and the row number. `null_once` names the database whose unique
/// indexes take NULL once.
fn distinct(
    generator: Generator,
    (name, rows): (&str, u64),
    token: &str,
    null_once: Option<&str>,
) -> Result<Generator, ImportError> {
    Ok(match generator {
        Generator::Null if rows > 1 && null_once.is_some() => {
            return Err(ImportError::Plan(format!(
                "{name} is unique, and {} takes NULL in it only once",
                null_once.unwrap_or_default()
            )));
        }
        g @ (Generator::Sequence { .. }
        | Generator::Uuid
        | Generator::ObjectId
        | Generator::Null
        | Generator::Distinct { .. }) => g,
        g @ (Generator::FirstName
        | Generator::LastName
        | Generator::FullName
        | Generator::Email
        | Generator::City
        | Generator::Phone
        | Generator::Text) => Generator::Unique {
            inner: Box::new(g),
            token: token.to_owned(),
        },
        _ => {
            return Err(ImportError::Plan(format!(
                "{name} is unique: fill it with keys after the largest, or with text"
            )));
        }
    })
}

/// Adds `rows` fake rows to `table` of `schema` on `target`, a connection of its own, in one
/// transaction; `progress` gets the rows written so far and returns false to stop.
pub async fn fill(
    (target, dialect): (&dyn Connection, Dialect),
    (schema, table): (Option<&str>, &str),
    mut columns: Vec<FakeColumn>,
    rows: u64,
    progress: impl FnMut(u64) -> bool,
) -> Result<u64, ImportError> {
    let qualified = dialect.qualified(schema, table);
    let mut rng = Rng::from_clock();
    let token = rng.token();
    composite_keys((target, dialect), (schema, table), &mut columns, &mut rng).await?;
    let mut resolved = Vec::with_capacity(columns.len());
    for c in columns {
        let at = ((schema, table), qualified.as_str(), rows);
        let generator = resolve((target, dialect), at, c.clone(), &token).await?;
        resolved.push(FakeColumn {
            generator,
            unique: false,
            ..c
        });
    }
    let plan = ImportPlan {
        source_table: String::new(),
        source_schema: None,
        schema: schema.map(str::to_owned),
        table: table.to_owned(),
        create: false,
        mode: ImportMode::Append,
        keys: Vec::new(),
        primary_key: Vec::new(),
        identity: None,
        constraints: Vec::new(),
        columns: resolved
            .iter()
            .map(|c| ImportColumn {
                source: c.name.clone(),
                target: c.name.clone(),
                // The rows are not a new table's: the declared type types text values where
                // the database converts none (see `import::typed_text`).
                data_type: c.data_type.clone(),
                not_null: false,
                default: None,
            })
            .collect(),
    };
    let schema = self::schema(&resolved);
    let batches = futures::stream::unfold(0u64, move |done| {
        let n = usize::try_from((rows - done).min(BATCH_ROWS as u64)).unwrap_or(BATCH_ROWS);
        let next = (n > 0).then(|| {
            (
                batch(&mut rng, &resolved, done, n).map_err(Into::into),
                done + n as u64,
            )
        });
        async move { next }
    });
    let stream = RecordBatchStream::new(schema, batches.boxed());
    import_rows(stream, (target, dialect), &plan, progress).await
}

/// A string as mongosh reads it: JSON's.
fn quote(s: &str) -> String {
    serde_json::Value::from(s).to_string()
}

/// A collection in mongosh, of `db` or of the session's database.
pub(crate) fn collection_ref(db: Option<&str>, collection: &str) -> String {
    let base = db.map_or_else(
        || "db".to_owned(),
        |d| format!("db.getSiblingDB({})", quote(d)),
    );
    format!("{base}.getCollection({})", quote(collection))
}

/// The mongosh query of the largest number in `field` of `collection`: other values sort
/// after numbers, and are left out.
fn largest_number(collection: &str, field: &str) -> String {
    let f = quote(field);
    let projection = if field == "_id" {
        format!("{{ {f}: 1 }}")
    } else {
        format!("{{ {f}: 1, \"_id\": 0 }}")
    };
    format!(
        "{collection}.find({{ {f}: {{ \"$type\": [\"int\", \"long\", \"double\", \"decimal\"] }} }}, \
         {projection}).sort({{ {f}: -1 }}).limit(1)"
    )
}

/// The generator `column` of `collection` stands for, as [`resolve`] for a table. `_id` is
/// unique in every collection.
async fn resolve_field(
    conn: &dyn Connection,
    (collection, rows): (&str, u64),
    column: FakeColumn,
    token: &str,
) -> Result<Generator, ImportError> {
    check(&column)?;
    let name = column.name.as_str();
    let generator = match column.generator {
        // A query reads `a.b` as a path and `$a` as an operator, not as the field's name.
        Generator::NextKey if name.contains('.') || name.starts_with('$') => {
            return Err(ImportError::Plan(format!(
                "{name}: no keys after the largest for a field named with `.` or `$`"
            )));
        }
        Generator::NextKey => {
            let max = query_value(conn, &largest_number(collection, name)).await?;
            next_key(name, max.as_deref(), rows)?
        }
        Generator::Reference { table, .. } => {
            return Err(ImportError::Plan(format!(
                "MongoDB has no foreign keys: {name} cannot take the keys of {table}"
            )));
        }
        other => other,
    };
    if !column.unique && name != "_id" {
        return Ok(generator);
    }
    // A unique index takes one null, and a missing field counts as null.
    distinct(generator, (name, rows), token, Some("MongoDB"))
}

/// Whole number `n` as mongosh reads it into a field of BSON type `data_type`: a 32-bit int
/// when it fits and the field is no other number. `NumberLong` takes the digits as text,
/// which no double rounds.
fn shell_integer(n: i64, data_type: Option<&str>) -> String {
    match data_type {
        Some("double") => format!("{n}.0"),
        Some("decimal") => format!("NumberDecimal(\"{n}\")"),
        Some("long") => format!("NumberLong(\"{n}\")"),
        _ if i32::try_from(n).is_ok() => n.to_string(),
        _ => format!("NumberLong(\"{n}\")"),
    }
}

/// Number `v` as mongosh reads it, a `NumberDecimal` into a decimal field.
fn shell_number(v: f64, data_type: Option<&str>) -> String {
    match data_type {
        _ if v.is_nan() => "NaN".to_owned(),
        _ if v.is_infinite() => if v > 0.0 { "Infinity" } else { "-Infinity" }.to_owned(),
        Some("decimal") => format!("NumberDecimal(\"{v}\")"),
        _ => format!("{v:?}"),
    }
}

/// Value `row` of `array`, filled as `column` says, as mongosh reads it: dates as `ISODate`,
/// numbers of the field's type, arrays and documents as their JSON.
fn shell_value(column: &FakeColumn, array: &dyn Array, row: usize) -> String {
    if array.is_null(row) {
        return "null".to_owned();
    }
    let data_type = column.data_type.as_deref();
    let date = |seconds: Option<i64>| {
        seconds
            .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
            .map_or_else(
                || "null".to_owned(),
                |d| format!("ISODate(\"{}\")", d.format("%Y-%m-%dT%H:%M:%SZ")),
            )
    };
    match array.data_type() {
        DataType::Int64 => shell_integer(array.as_primitive::<Int64Type>().value(row), data_type),
        DataType::Float64 => {
            shell_number(array.as_primitive::<Float64Type>().value(row), data_type)
        }
        DataType::Boolean => array.as_boolean().value(row).to_string(),
        DataType::Date32 => {
            let days = i64::from(array.as_primitive::<Date32Type>().value(row));
            date(days.checked_mul(86_400))
        }
        DataType::Timestamp(..) => {
            let micros = array.as_primitive::<TimestampMicrosecondType>().value(row);
            date(Some(micros.div_euclid(1_000_000)))
        }
        _ => {
            let text = array.as_string::<i32>().value(row);
            match column.generator {
                Generator::ObjectId => format!("ObjectId({})", quote(text)),
                Generator::WordList | Generator::Document => text.to_owned(),
                _ => quote(text),
            }
        }
    }
}

/// `collection.insertMany([…])` with a document per row of `batch`, whose columns are
/// `columns`.
fn insert_many(collection: &str, columns: &[FakeColumn], batch: &RecordBatch) -> String {
    let docs: Vec<String> = (0..batch.num_rows())
        .map(|row| {
            let fields: Vec<String> = columns
                .iter()
                .zip(batch.columns())
                .map(|(c, a)| {
                    let value = shell_value(c, a.as_ref(), row);
                    format!("{}: {value}", quote(&c.name))
                })
                .collect();
            format!("{{ {} }}", fields.join(", "))
        })
        .collect();
    format!("{collection}.insertMany([{}])", docs.join(", "))
}

/// Adds `rows` fake documents to `collection` of the MongoDB database `db` (the session's
/// without one) on `target`, a connection of its own, a statement per [`BATCH_ROWS`]
/// documents. There are no transactions: the documents of the statements before a failure
/// stay. `progress` gets the documents written so far and returns false to stop before the
/// next statement.
pub async fn fill_collection(
    target: &dyn Connection,
    (db, collection): (Option<&str>, &str),
    columns: Vec<FakeColumn>,
    rows: u64,
    mut progress: impl FnMut(u64) -> bool,
) -> Result<u64, ImportError> {
    if columns.is_empty() {
        return Err(ImportError::Plan("no field to fill".to_owned()));
    }
    let at = collection_ref(db, collection);
    let mut rng = Rng::from_clock();
    let token = rng.token();
    let mut resolved = Vec::with_capacity(columns.len());
    for c in columns {
        let generator = resolve_field(target, (&at, rows), c.clone(), &token).await?;
        resolved.push(FakeColumn {
            generator,
            unique: false,
            ..c
        });
    }
    let mut written = 0;
    while written < rows {
        let n = usize::try_from(rows - written).map_or(BATCH_ROWS, |left| left.min(BATCH_ROWS));
        let batch = batch(&mut rng, &resolved, written, n)?;
        let sql = insert_many(&at, &resolved, &batch);
        target.execute(&sql, &ExecOptions::default()).await?;
        written += n as u64;
        // Stopped during the last statement, every document is written all the same.
        if !progress(written) && written < rows {
            return Err(ImportError::Cancelled);
        }
    }
    Ok(written)
}

#[expect(clippy::cast_possible_wrap, reason = "days since 1970 fit in i64")]
fn days_since_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| (d.as_secs() / 86_400) as i64)
}

#[cfg(test)]
mod tests {
    use platypus_driver_mongo::literal::to_shell;
    use platypus_driver_mongo::shell::{self, CollectionOp, Command};

    use super::*;

    fn column(name: &str, generator: Generator) -> FakeColumn {
        FakeColumn {
            name: name.to_owned(),
            generator,
            unique: false,
            data_type: None,
        }
    }

    fn typed(name: &str, generator: Generator, data_type: &str) -> FakeColumn {
        FakeColumn {
            data_type: Some(data_type.to_owned()),
            ..column(name, generator)
        }
    }

    #[test]
    fn fills_each_column_by_its_generator() {
        let columns = [
            column("id", Generator::Sequence { start: 100 }),
            column("age", Generator::Integer { min: 18, max: 20 }),
            column("email", Generator::Email),
            column(
                "status",
                Generator::Choice {
                    values: vec!["new".into(), "done".into()],
                },
            ),
            column("id_text", Generator::Uuid),
            column("note", Generator::Null),
        ];
        let mut rng = Rng::new(7);
        let batch = batch(&mut rng, &columns, 0, 50).unwrap();
        assert_eq!(batch.num_rows(), 50);
        let ids = batch.column(0).as_primitive::<Int64Type>();
        assert_eq!((ids.value(0), ids.value(49)), (100, 149));
        let ages = batch.column(1).as_primitive::<Int64Type>();
        assert!(ages.values().iter().all(|a| (18..=20).contains(a)));
        let emails = batch.column(2).as_string::<i32>();
        assert!(emails.value(3).contains("@example."), "{}", emails.value(3));
        assert_ne!(emails.value(1), emails.value(2));
        let status = batch.column(3).as_string::<i32>();
        assert!(["new", "done"].contains(&status.value(0)));
        let uuid = batch.column(4).as_string::<i32>().value(0).to_owned();
        assert_eq!((uuid.len(), &uuid[14..15]), (36, "4"));
        assert_eq!(batch.column(5).null_count(), 50);
    }

    #[test]
    fn text_fits_the_length_of_its_column() {
        let columns = [
            typed("code", Generator::Text, "character varying(5)"),
            typed("cc", Generator::FullName, "character(2)"),
            typed(
                "u",
                Generator::Unique {
                    inner: Box::new(Generator::FullName),
                    token: "t0k3n0".to_owned(),
                },
                "varchar(8)",
            ),
            typed("at", Generator::Time, "time"),
            typed("ip", Generator::Ip, "inet"),
            typed("fx", Generator::Text, "Nullable(FixedString(4))"),
            typed("o", Generator::City, "VARCHAR2(3 BYTE)"),
        ];
        let mut rng = Rng::new(7);
        let batch = batch(&mut rng, &columns, 0, 50).unwrap();
        let text = |c: usize, r: usize| batch.column(c).as_string::<i32>().value(r).to_owned();
        for r in 0..50 {
            assert!(text(0, r).chars().count() <= 5, "{}", text(0, r));
            assert!(text(1, r).chars().count() <= 2, "{}", text(1, r));
            // A unique value keeps the end that tells it apart.
            assert!(
                text(2, r).ends_with(&format!("t0k3n0{r}")),
                "{}",
                text(2, r)
            );
            assert_eq!(text(3, r).len(), 8);
            assert!(text(4, r).starts_with("10."), "{}", text(4, r));
            assert!(text(5, r).len() <= 4, "{}", text(5, r));
            assert!(text(6, r).chars().count() <= 3, "{}", text(6, r));
        }
        assert_eq!(cut("lorem ipsum".to_owned(), 6, false), "lorem");
    }

    #[test]
    fn a_table_refers_to_itself() {
        assert!(is_own((Some("public"), "tree"), (None, "tree")));
        assert!(is_own((Some(""), "tree"), (Some("app"), "tree")));
        assert!(!is_own((Some("public"), "tree"), (Some("app"), "tree")));
        assert!(!is_own((None, "parent"), (None, "tree")));
    }

    #[test]
    fn composite_keys_and_unique_values() {
        let rows = Arc::new(vec![
            vec!["1".to_owned(), "a".to_owned()],
            vec!["2".to_owned(), "b".to_owned()],
            vec!["3".to_owned(), "c".to_owned()],
        ]);
        let part = |part| Generator::Tuple {
            rows: Arc::clone(&rows),
            part,
            seed: 42,
        };
        let unique = |inner| Generator::Unique {
            inner: Box::new(inner),
            token: "t0k3n0".to_owned(),
        };
        let columns = [
            column("id", part(0)),
            column("code", part(1)),
            column("city", unique(Generator::City)),
            column("email", unique(Generator::Email)),
        ];
        let mut rng = Rng::new(7);
        let batch = batch(&mut rng, &columns, 0, 200).unwrap();
        let text = |c: usize, r: usize| batch.column(c).as_string::<i32>().value(r).to_owned();
        let mut pairs = std::collections::HashSet::new();
        let mut cities = std::collections::HashSet::new();
        for r in 0..200 {
            let pair = vec![text(0, r), text(1, r)];
            assert!(rows.contains(&pair), "{pair:?}");
            pairs.insert(pair);
            cities.insert(text(2, r));
        }
        assert_eq!(pairs.len(), 3, "every referenced row is used");
        assert_eq!(cities.len(), 200, "unique values");
        let email = text(3, 5);
        assert!(email.contains(".t0k3n05@example."), "{email}");
    }

    #[test]
    fn mongo_documents_take_the_types_of_their_fields() {
        let columns = [
            column("_id", Generator::Sequence { start: 41 }),
            column(
                "big",
                Generator::Sequence {
                    start: 3_000_000_000,
                },
            ),
            column("tags", Generator::WordList),
            column("address", Generator::Document),
            column("ref", Generator::ObjectId),
            column("created", Generator::Date),
            column("seen", Generator::Timestamp),
            column(
                "price",
                Generator::Decimal {
                    min: 1.0,
                    max: 2.0,
                    scale: 0,
                },
            ),
            column("note", Generator::Null),
            column("name", Generator::FullName),
        ];
        let mut rng = Rng::new(7);
        let batch = batch(&mut rng, &columns, 0, 3).unwrap();
        let src = insert_many(&collection_ref(Some("shop"), "my items"), &columns, &batch);
        let Command::Collection {
            db,
            collection,
            op: CollectionOp::InsertMany(docs),
        } = shell::parse(&src).unwrap()
        else {
            panic!("{src}");
        };
        assert_eq!(
            (db.as_deref(), collection.as_str()),
            (Some("shop"), "my items")
        );
        assert_eq!(docs.len(), 3);
        let field = |row: usize, name: &str| to_shell(docs[row].get(name).unwrap());
        assert_eq!(
            (field(0, "_id"), field(2, "_id")),
            ("41".into(), "43".into())
        );
        assert_eq!(field(0, "big"), "NumberLong(3000000000)");
        assert!(field(0, "tags").starts_with("[\""), "{}", field(0, "tags"));
        let address = field(0, "address");
        assert!(
            address.starts_with("{ ") && address.contains("city: "),
            "{address}"
        );
        assert!(field(0, "ref").starts_with("ObjectId(\""));
        assert_ne!(field(0, "ref"), field(1, "ref"));
        assert!(
            field(0, "created").ends_with("T00:00:00Z\")"),
            "{}",
            field(0, "created")
        );
        assert!(field(0, "seen").starts_with("ISODate(\""));
        assert!(
            ["1.0", "2.0"].contains(&field(0, "price").as_str()),
            "a double"
        );
        assert_eq!(field(0, "note"), "null");
        assert!(field(0, "name").starts_with('"'));
    }

    #[test]
    fn next_key_after_the_largest_number() {
        let start = |max: Option<&str>, rows| match next_key("k", max, rows) {
            Ok(Generator::Sequence { start }) => Some(start),
            _ => None,
        };
        assert_eq!(start(None, 10), Some(1));
        assert_eq!(start(Some("41"), 10), Some(42));
        assert_eq!(start(Some("41.5"), 10), Some(42));
        assert_eq!(start(Some("41.00"), 10), Some(42));
        assert_eq!(start(Some("-3"), 10), Some(-2));
        assert_eq!(start(Some("-3.5"), 10), Some(-3));
        assert_eq!(
            start(Some("1.5E+3"), 10),
            Some(1501),
            "Decimal128's exponent"
        );
        // Beyond what an f64 holds exactly: a NUMERIC(20, 2) maximum.
        assert_eq!(
            start(Some("9007199254740997.00"), 10),
            Some(9_007_199_254_740_998)
        );
        assert_eq!(start(Some("abc"), 10), Some(1), "text keys");
        for beyond in ["inf", "NaN", "1e30", "9223372036854775807"] {
            assert_eq!(start(Some(beyond), 10), None, "{beyond}");
        }
        assert_eq!(start(Some("9223372036854775797"), 10), Some(i64::MAX - 9));
        assert_eq!(start(Some("9223372036854775797"), 11), None);
        let src = largest_number(&collection_ref(None, "customers"), "_id");
        let Command::Collection {
            db: None,
            op: CollectionOp::Find(find),
            ..
        } = shell::parse(&src).unwrap()
        else {
            panic!("{src}");
        };
        assert_eq!(find.limit, Some(1));
        assert_eq!(to_shell(&find.sort.unwrap().into()), "{ _id: -1 }");
        assert_eq!(
            to_shell(&find.filter.into()),
            r#"{ _id: { $type: ["int", "long", "double", "decimal"] } }"#
        );
        let src = largest_number(&collection_ref(None, "customers"), "n");
        let Command::Collection {
            op: CollectionOp::Find(find),
            ..
        } = shell::parse(&src).unwrap()
        else {
            panic!("{src}");
        };
        assert_eq!(
            to_shell(&find.projection.unwrap().into()),
            "{ n: 1, _id: 0 }"
        );
    }

    /// The values of `columns` in the first document mongosh reads from their statement.
    fn first_document(columns: &[FakeColumn]) -> Vec<String> {
        let mut rng = Rng::new(7);
        let batch = batch(&mut rng, columns, 0, 1).unwrap();
        let src = insert_many(&collection_ref(None, "c"), columns, &batch);
        let Command::Collection {
            op: CollectionOp::InsertMany(docs),
            ..
        } = shell::parse(&src).unwrap()
        else {
            panic!("{src}");
        };
        columns
            .iter()
            .map(|c| to_shell(docs[0].get(&c.name).unwrap()))
            .collect()
    }

    #[test]
    fn numbers_take_the_type_of_their_field() {
        let seq = |start| Generator::Sequence { start };
        let decimal = Generator::Decimal {
            min: 2.5,
            max: 2.5,
            scale: 1,
        };
        let values = first_document(&[
            // Beyond 2^53: digits a double would round.
            column("big", seq(9_007_199_254_740_993)),
            column("max", seq(i64::MAX)),
            column("small", seq(5)),
            typed("long", seq(5), "long"),
            typed("double", seq(5), "double"),
            typed("decimal", seq(5), "decimal"),
            typed("price", decimal.clone(), "decimal"),
            column("ratio", decimal),
        ]);
        assert_eq!(
            values,
            [
                "NumberLong(9007199254740993)",
                "NumberLong(9223372036854775807)",
                "5",
                "NumberLong(5)",
                "5.0",
                "NumberDecimal(\"5\")",
                "NumberDecimal(\"2.5\")",
                "2.5"
            ]
        );
        assert_eq!(shell_number(f64::INFINITY, None), "Infinity");
    }

    #[test]
    fn refuses_values_beyond_their_type() {
        let decimal = |min, max| column("n", Generator::Decimal { min, max, scale: 2 });
        assert!(check(&decimal(0.0, 1e308)).is_ok());
        assert!(
            check(&decimal(-1e308, 1e308)).is_err(),
            "the span overflows"
        );
        assert!(check(&decimal(0.0, f64::INFINITY)).is_err());
        let mut rng = Rng::new(7);
        let last = [column("id", Generator::Sequence { start: i64::MAX })];
        assert!(batch(&mut rng, &last, 0, 1).is_ok());
        assert!(
            batch(&mut rng, &last, 0, 2).is_err(),
            "no key after i64::MAX"
        );
    }

    #[test]
    fn unique_fields_take_distinct_values() {
        let unique = |g, rows, null_once| distinct(g, ("f", rows), "t0k3n0", null_once);
        assert!(matches!(
            unique(Generator::Null, 2, Some("MongoDB")),
            Err(ImportError::Plan(m)) if m.contains("MongoDB takes NULL")
        ));
        assert_eq!(
            unique(Generator::Null, 1, Some("MongoDB")).unwrap(),
            Generator::Null
        );
        assert_eq!(unique(Generator::Null, 2, None).unwrap(), Generator::Null);
        assert_eq!(
            unique(Generator::ObjectId, 2, None).unwrap(),
            Generator::ObjectId
        );
        assert!(unique(Generator::WordList, 2, None).is_err());
        assert!(unique(Generator::Document, 2, None).is_err());
        assert!(matches!(
            unique(Generator::City, 2, None).unwrap(),
            Generator::Unique { .. }
        ));
    }
}
