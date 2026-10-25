//! Fake rows for a table: each column filled by a generator (names, emails, numbers, dates,
//! values of an enum), foreign keys with values the referenced table has, so that the rows
//! fit their constraints. The rows are written by [`crate::import::import_rows`].

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use arrow_array::builder::{
    BooleanBuilder, Date32Builder, Float64Builder, Int64Builder, StringBuilder,
    TimestampMicrosecondBuilder,
};
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{ArrowError, DataType, Field, Schema, SchemaRef, TimeUnit};
use futures::StreamExt;
use platypus_core::{Connection, ExecOptions, ExecResult, RecordBatchStream};
use platypus_sql::Dialect;
use serde::Deserialize;

use crate::import::{ImportColumn, ImportError, ImportMode, ImportPlan, import_rows, rows_text};

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
    Uuid,
    /// One of `values`: an enum's, or the keys a foreign key may point at.
    Choice {
        values: Vec<String>,
    },
    Null,
    /// The integers after the column's largest: resolved to a [`Generator::Sequence`].
    NextKey,
    /// Values `column` of `table` has, for a foreign key: resolved to a [`Generator::Choice`].
    Reference {
        schema: Option<String>,
        table: String,
        column: String,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FakeColumn {
    pub name: String,
    pub generator: Generator,
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
            let words: Vec<&str> = (0..n).map(|_| *rng.pick(WORDS)).collect();
            words.join(" ")
        }
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
        _ => return None,
    })
}

/// `rows` rows of `columns`, the first numbered `first` (for sequences and unique values).
pub fn batch(
    rng: &mut Rng,
    columns: &[FakeColumn],
    first: u64,
    rows: usize,
) -> Result<RecordBatch, ArrowError> {
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
                    let mut b = StringBuilder::with_capacity(rows, rows * 16);
                    numbers.for_each(|n| b.append_option(text(rng, other, n)));
                    Arc::new(b.finish())
                }
            }
        })
        .collect();
    RecordBatch::try_new(schema(columns), arrays)
}

/// The values of the one-column query `sql`, as text.
async fn values(conn: &dyn Connection, sql: &str) -> Result<Vec<Option<String>>, ImportError> {
    let ExecResult::Rows(mut rows) = conn.execute(sql, &ExecOptions::default()).await? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    while let Some(batch) = rows.next().await {
        out.extend(
            rows_text(&batch?)?
                .into_iter()
                .map(|mut r| r.swap_remove(0)),
        );
    }
    Ok(out)
}

/// The generator `generator` stands for, read from the table's database where it says so.
async fn resolve(
    conn: &dyn Connection,
    dialect: Dialect,
    table: &str,
    (column, generator): (&str, Generator),
) -> Result<Generator, ImportError> {
    Ok(match generator {
        Generator::NextKey => {
            let sql = format!("SELECT MAX({}) FROM {table}", dialect.quote_ident(column));
            let max = values(conn, &sql).await?.into_iter().flatten().next();
            let start = max.and_then(|m| m.parse::<i64>().ok()).unwrap_or(0) + 1;
            Generator::Sequence { start }
        }
        Generator::Reference {
            schema,
            table: referenced,
            column: key,
        } => {
            let q = dialect.quote_ident(&key);
            let select = format!(
                "SELECT DISTINCT {q} FROM {} WHERE {q} IS NOT NULL",
                dialect.qualified(schema.as_deref(), &referenced)
            );
            let values: Vec<String> = values(conn, &dialect.limited(&select, REFERENCE_VALUES))
                .await?
                .into_iter()
                .flatten()
                .collect();
            if values.is_empty() {
                return Err(ImportError::Plan(format!(
                    "{column} refers to {referenced}, which has no rows yet: fill it first"
                )));
            }
            Generator::Choice { values }
        }
        other => other,
    })
}

/// Adds `rows` fake rows to `table` of `schema` on `target`, a connection of its own, in one
/// transaction; `progress` gets the rows written so far and returns false to stop.
pub async fn fill(
    (target, dialect): (&dyn Connection, Dialect),
    (schema, table): (Option<&str>, &str),
    columns: Vec<FakeColumn>,
    rows: u64,
    progress: impl FnMut(u64) -> bool,
) -> Result<u64, ImportError> {
    let qualified = dialect.qualified(schema, table);
    let mut resolved = Vec::with_capacity(columns.len());
    for c in columns {
        let generator = resolve(target, dialect, &qualified, (&c.name, c.generator)).await?;
        resolved.push(FakeColumn {
            name: c.name,
            generator,
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
        columns: resolved
            .iter()
            .map(|c| ImportColumn {
                source: c.name.clone(),
                target: c.name.clone(),
                data_type: None,
                not_null: false,
            })
            .collect(),
    };
    let schema = self::schema(&resolved);
    let mut rng = Rng::from_clock();
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

#[expect(clippy::cast_possible_wrap, reason = "days since 1970 fit in i64")]
fn days_since_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| (d.as_secs() / 86_400) as i64)
}

#[cfg(test)]
mod tests {
    use arrow_array::cast::AsArray;
    use arrow_array::types::Int64Type;

    use super::*;

    fn column(name: &str, generator: Generator) -> FakeColumn {
        FakeColumn {
            name: name.to_owned(),
            generator,
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
}
