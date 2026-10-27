//! SQLite values into Arrow columns.
//!
//! SQLite is dynamically typed: a column's declared type is only a hint and any row may
//! hold any storage class. The Arrow type of each column is inferred from the values of
//! the whole result when the driver can read them ahead in time, else from the first batch
//! (falling back to the declared type); later values are coerced when lossless.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use arrow_array::ArrayRef;
use arrow_array::builder::{BinaryBuilder, Float64Builder, Int64Builder, StringBuilder};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use rusqlite::types::ValueRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Int,
    Float,
    Text,
    Blob,
}

/// SQLite's type affinity rules (section 3.1 of "Datatypes In SQLite"), restricted to the
/// cases where the declared type predicts the stored values well.
fn affinity(decl: &str) -> Option<Kind> {
    let decl = decl.to_ascii_uppercase();
    if decl.contains("INT") {
        Some(Kind::Int)
    } else if decl.contains("CHAR") || decl.contains("CLOB") || decl.contains("TEXT") {
        Some(Kind::Text)
    } else if decl.contains("BLOB") {
        Some(Kind::Blob)
    } else if decl.contains("REAL") || decl.contains("FLOA") || decl.contains("DOUB") {
        Some(Kind::Float)
    } else {
        None
    }
}

/// Integers beyond this lose digits as reals.
const EXACT_IN_REAL: u64 = 1 << 53;

/// The storage classes of a column's values so far.
#[derive(Debug, Clone, Copy, Default)]
pub struct Seen {
    kind: Option<Kind>,
    /// An integer beyond what a real holds exactly.
    big: bool,
}

impl Seen {
    pub fn add(&mut self, value: ValueRef<'_>) {
        let kind = match value {
            ValueRef::Null => return,
            ValueRef::Integer(i) => {
                self.big |= i.unsigned_abs() > EXACT_IN_REAL;
                Kind::Int
            }
            ValueRef::Real(_) => Kind::Float,
            ValueRef::Text(_) => Kind::Text,
            ValueRef::Blob(_) => Kind::Blob,
        };
        self.kind = Some(match (self.kind, kind) {
            (None, k) => k,
            (Some(a), b) if a == b => a,
            (Some(Kind::Int | Kind::Float), Kind::Int | Kind::Float) => Kind::Float,
            _ => Kind::Text,
        });
    }

    /// The column kind, or that of the declared type when all values were `NULL`. Mixed
    /// storage classes, and reals with integers beyond 2^53, are text, which keeps them.
    #[must_use]
    pub fn kind(self, decl: Option<&str>) -> Kind {
        match self.kind {
            Some(Kind::Float) if self.big => Kind::Text,
            Some(kind) => kind,
            None => decl.and_then(affinity).unwrap_or(Kind::Text),
        }
    }
}

/// The column kind for `values` (see [`Seen::kind`]).
#[must_use]
pub fn infer<'a>(decl: Option<&str>, values: impl Iterator<Item = ValueRef<'a>>) -> Kind {
    let mut seen = Seen::default();
    values.for_each(|v| seen.add(v));
    seen.kind(decl)
}

#[must_use]
pub fn field(name: &str, decl: Option<&str>, kind: Kind) -> Field {
    let data_type = match kind {
        Kind::Int => DataType::Int64,
        Kind::Float => DataType::Float64,
        Kind::Text => DataType::Utf8,
        Kind::Blob => DataType::Binary,
    };
    let field = Field::new(name, data_type, true);
    match decl {
        Some(decl) if !decl.is_empty() => field.with_metadata(HashMap::from([(
            platypus_core::META_DB_TYPE.to_owned(),
            decl.to_owned(),
        )])),
        _ => field,
    }
}

/// A copy of a value, for the rows read ahead to infer column types.
#[derive(Debug, Clone, PartialEq)]
pub enum Owned {
    Null,
    Integer(i64),
    Real(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

impl Owned {
    #[must_use]
    pub fn new(value: ValueRef<'_>) -> Self {
        match value {
            ValueRef::Null => Self::Null,
            ValueRef::Integer(i) => Self::Integer(i),
            ValueRef::Real(r) => Self::Real(r),
            ValueRef::Text(t) => Self::Text(t.to_vec()),
            ValueRef::Blob(b) => Self::Blob(b.to_vec()),
        }
    }

    #[must_use]
    pub fn value(&self) -> ValueRef<'_> {
        match self {
            Self::Null => ValueRef::Null,
            Self::Integer(i) => ValueRef::Integer(*i),
            Self::Real(r) => ValueRef::Real(*r),
            Self::Text(t) => ValueRef::Text(t),
            Self::Blob(b) => ValueRef::Blob(b),
        }
    }
}

#[derive(Debug)]
pub enum ColumnBuilder {
    Int(Int64Builder),
    Float(Float64Builder),
    Text(StringBuilder),
    Blob(BinaryBuilder),
}

/// The schema of a result with `columns` (name, declared type), each column's kind from
/// the values `seen` of it.
#[must_use]
pub fn schema(columns: &[(String, Option<String>)], seen: &[Seen]) -> (SchemaRef, Vec<Kind>) {
    let kinds: Vec<Kind> = columns
        .iter()
        .zip(seen)
        .map(|((_, decl), seen)| seen.kind(decl.as_deref()))
        .collect();
    let schema = Arc::new(Schema::new(
        columns
            .iter()
            .zip(&kinds)
            .map(|((name, decl), kind)| field(name, decl.as_deref(), *kind))
            .collect::<Vec<_>>(),
    ));
    (schema, kinds)
}

/// A value that does not fit the column's inferred type.
#[derive(Debug)]
pub struct Mismatch {
    pub found: &'static str,
    pub expected: &'static str,
}

impl Mismatch {
    /// The decoding error for a value of `field` that does not fit.
    #[must_use]
    pub fn error(&self, field: &Field) -> platypus_core::Error {
        platypus_core::Error::Decode {
            column: field.name().clone(),
            db_type: field
                .metadata()
                .get(platypus_core::META_DB_TYPE)
                .cloned()
                .unwrap_or_default(),
            message: format!(
                "a {} value follows {} values in the first rows; use CAST(... AS TEXT) \
                 for columns with mixed types",
                self.found, self.expected
            ),
        }
    }
}

fn storage_class(value: ValueRef<'_>) -> &'static str {
    match value {
        ValueRef::Null => "null",
        ValueRef::Integer(_) => "integer",
        ValueRef::Real(_) => "real",
        ValueRef::Text(_) => "text",
        ValueRef::Blob(_) => "blob",
    }
}

/// Formats a real the way SQLite's `CAST(x AS TEXT)` does for common values.
fn real_text(r: f64) -> String {
    if r.is_finite() && r.fract() == 0.0 && r.abs() < 1e15 {
        format!("{r:.1}")
    } else {
        r.to_string()
    }
}

fn blob_text(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(3 + 2 * bytes.len());
    s.push_str("X'");
    for b in bytes {
        let _ = write!(s, "{b:02X}");
    }
    s.push('\'');
    s
}

/// The `i64` an integral real converts to without loss.
#[expect(
    clippy::cast_possible_truncation,
    reason = "range and fraction are checked first"
)]
fn exact_int(r: f64) -> Option<i64> {
    // 2^63 is exactly representable; every float below it in magnitude fits an i64.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    (r.fract() == 0.0 && (-LIMIT..LIMIT).contains(&r)).then_some(r as i64)
}

impl ColumnBuilder {
    #[must_use]
    pub fn new(kind: Kind, capacity: usize) -> Self {
        match kind {
            Kind::Int => Self::Int(Int64Builder::with_capacity(capacity)),
            Kind::Float => Self::Float(Float64Builder::with_capacity(capacity)),
            Kind::Text => Self::Text(StringBuilder::with_capacity(capacity, capacity * 16)),
            Kind::Blob => Self::Blob(BinaryBuilder::with_capacity(capacity, capacity * 16)),
        }
    }

    #[expect(clippy::cast_precision_loss, reason = "the range is checked first")]
    pub fn append(&mut self, value: ValueRef<'_>) -> Result<(), Mismatch> {
        let mismatch = |expected| Mismatch {
            found: storage_class(value),
            expected,
        };
        match (self, value) {
            (Self::Int(b), ValueRef::Null) => b.append_null(),
            (Self::Float(b), ValueRef::Null) => b.append_null(),
            (Self::Text(b), ValueRef::Null) => b.append_null(),
            (Self::Blob(b), ValueRef::Null) => b.append_null(),

            (Self::Int(b), ValueRef::Integer(i)) => b.append_value(i),
            (Self::Int(b), ValueRef::Real(r)) => {
                b.append_value(exact_int(r).ok_or_else(|| mismatch("integer"))?);
            }
            (Self::Int(_), _) => return Err(mismatch("integer")),

            (Self::Float(b), ValueRef::Integer(i)) if i.unsigned_abs() <= EXACT_IN_REAL => {
                b.append_value(i as f64);
            }
            (Self::Float(b), ValueRef::Real(r)) => b.append_value(r),
            (Self::Float(_), _) => return Err(mismatch("real")),

            (Self::Text(b), ValueRef::Integer(i)) => b.append_value(i.to_string()),
            (Self::Text(b), ValueRef::Real(r)) => b.append_value(real_text(r)),
            (Self::Text(b), ValueRef::Text(t)) => b.append_value(String::from_utf8_lossy(t)),
            (Self::Text(b), ValueRef::Blob(bytes)) => b.append_value(blob_text(bytes)),

            (Self::Blob(b), ValueRef::Blob(bytes) | ValueRef::Text(bytes)) => {
                b.append_value(bytes);
            }
            (Self::Blob(_), _) => return Err(mismatch("blob")),
        }
        Ok(())
    }

    pub fn finish(&mut self) -> ArrayRef {
        match self {
            Self::Int(b) => Arc::new(b.finish()),
            Self::Float(b) => Arc::new(b.finish()),
            Self::Text(b) => Arc::new(b.finish()),
            Self::Blob(b) => Arc::new(b.finish()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ValueRef::{Blob, Integer, Null, Real, Text};

    #[test]
    fn infers_from_values_then_declared_type() {
        assert_eq!(
            infer(Some("TEXT"), [Integer(1), Null].into_iter()),
            Kind::Int
        );
        assert_eq!(
            infer(None, [Integer(1), Real(0.5)].into_iter()),
            Kind::Float
        );
        assert_eq!(
            infer(None, [Integer(1), Text(b"x")].into_iter()),
            Kind::Text
        );
        assert_eq!(infer(None, [Blob(b"x")].into_iter()), Kind::Blob);
        assert_eq!(infer(Some("BIGINT"), [Null].into_iter()), Kind::Int);
        assert_eq!(infer(Some("VARCHAR(10)"), std::iter::empty()), Kind::Text);
        assert_eq!(infer(Some("DECIMAL(10,2)"), std::iter::empty()), Kind::Text);
        assert_eq!(infer(None, std::iter::empty()), Kind::Text);
    }

    #[test]
    fn integers_beyond_reals_keep_their_digits() {
        let big = 9_007_199_254_740_993;
        assert_eq!(
            infer(None, [Real(0.5), Integer(big)].into_iter()),
            Kind::Text
        );
        assert_eq!(
            infer(None, [Real(0.5), Integer(1 << 53)].into_iter()),
            Kind::Float
        );
        let mut real = ColumnBuilder::new(Kind::Float, 2);
        real.append(Integer(1 << 53)).unwrap();
        assert!(real.append(Integer(big)).is_err(), "never rounded");
        let mut text = ColumnBuilder::new(Kind::Text, 1);
        text.append(Integer(big)).unwrap();
        let array = text.finish();
        let strings = array
            .as_any()
            .downcast_ref::<arrow_array::StringArray>()
            .unwrap();
        assert_eq!(strings.value(0), "9007199254740993");
    }

    #[test]
    fn coerces_only_without_loss() {
        let mut int = ColumnBuilder::new(Kind::Int, 4);
        int.append(Real(3.0)).unwrap();
        assert!(int.append(Real(3.5)).is_err());
        assert!(int.append(Text(b"3")).is_err());

        let mut text = ColumnBuilder::new(Kind::Text, 4);
        for v in [Integer(7), Real(2.0), Real(0.25), Blob(&[0xab, 0x01]), Null] {
            text.append(v).unwrap();
        }
        let array = text.finish();
        let strings = array
            .as_any()
            .downcast_ref::<arrow_array::StringArray>()
            .unwrap();
        assert_eq!(
            strings.iter().collect::<Vec<_>>(),
            [Some("7"), Some("2.0"), Some("0.25"), Some("X'AB01'"), None]
        );
    }
}
