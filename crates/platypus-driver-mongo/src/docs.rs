//! Documents to Arrow: one column per top-level field of the first documents of a result.
//!
//! A field whose values share a type gets the matching Arrow type; nested documents and
//! arrays become relaxed Extended JSON text, and fields of mixed types text too. The
//! column's [`META_DB_TYPE`] says which BSON type it holds (`document` and `array` for the
//! JSON columns), so the UI can show those as a tree.

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::builder::{
    BinaryBuilder, BooleanBuilder, Float64Builder, Int32Builder, Int64Builder, StringBuilder,
    TimestampMillisecondBuilder,
};
use arrow_array::{ArrayRef, RecordBatch, RecordBatchOptions};
use arrow_schema::{DataType, Field, Schema, SchemaRef, TimeUnit};
use mongodb::bson::{Bson, Document, doc};
use platypus_core::{Error, META_DB_TYPE, Result};

/// The BSON type of a column, as far as the Arrow type goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bool,
    Int32,
    Int64,
    Double,
    String,
    ObjectId,
    Date,
    Decimal,
    Binary,
    Document,
    Array,
    /// Any other type, named by mongosh's `$type` alias.
    Other(&'static str),
    Mixed,
}

impl Kind {
    fn of(value: &Bson) -> Option<Self> {
        Some(match value {
            Bson::Null | Bson::Undefined => return None,
            Bson::Boolean(_) => Self::Bool,
            Bson::Int32(_) => Self::Int32,
            Bson::Int64(_) => Self::Int64,
            Bson::Double(_) => Self::Double,
            Bson::String(_) => Self::String,
            Bson::ObjectId(_) => Self::ObjectId,
            Bson::DateTime(_) => Self::Date,
            Bson::Decimal128(_) => Self::Decimal,
            Bson::Binary(_) => Self::Binary,
            Bson::Document(_) => Self::Document,
            Bson::Array(_) => Self::Array,
            Bson::RegularExpression(_) => Self::Other("regex"),
            Bson::Timestamp(_) => Self::Other("timestamp"),
            Bson::JavaScriptCode(_) | Bson::JavaScriptCodeWithScope(_) => Self::Other("javascript"),
            Bson::Symbol(_) => Self::Other("symbol"),
            Bson::MinKey => Self::Other("minKey"),
            Bson::MaxKey => Self::Other("maxKey"),
            Bson::DbPointer(_) => Self::Other("dbPointer"),
        })
    }

    /// The kind of a column that holds both kinds.
    fn merge(self, other: Self) -> Self {
        use Kind::{Double, Int32, Int64, Mixed};
        match (self, other) {
            (a, b) if a == b => a,
            (Int32 | Int64, Int32 | Int64) => Int64,
            (Int32 | Int64 | Double, Int32 | Int64 | Double) => Double,
            _ => Mixed,
        }
    }

    fn data_type(self) -> DataType {
        match self {
            Self::Bool => DataType::Boolean,
            Self::Int32 => DataType::Int32,
            Self::Int64 => DataType::Int64,
            Self::Double => DataType::Float64,
            Self::Date => DataType::Timestamp(TimeUnit::Millisecond, Some("+00:00".into())),
            Self::Binary => DataType::Binary,
            _ => DataType::Utf8,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int32 => "int",
            Self::Int64 => "long",
            Self::Double => "double",
            Self::String => "string",
            Self::ObjectId => "objectId",
            Self::Date => "date",
            Self::Decimal => "decimal",
            Self::Binary => "binData",
            Self::Document => "document",
            Self::Array => "array",
            Self::Other(name) => name,
            Self::Mixed => "mixed",
        }
    }

    /// The kind of a `$type` alias, as the server names value types; `None` for null.
    fn from_alias(alias: &str) -> Option<Self> {
        Some(match alias {
            "null" | "undefined" | "missing" => return None,
            "object" => Self::Document,
            "javascriptWithScope" => Self::Other("javascript"),
            "regex" => Self::Other("regex"),
            "timestamp" => Self::Other("timestamp"),
            "javascript" => Self::Other("javascript"),
            "symbol" => Self::Other("symbol"),
            "minKey" => Self::Other("minKey"),
            "maxKey" => Self::Other("maxKey"),
            "dbPointer" => Self::Other("dbPointer"),
            name => match Self::from_name(name) {
                Self::Mixed => Self::Other("unknown"),
                kind => kind,
            },
        })
    }

    /// The kind of that [`Self::name`]; [`Self::Mixed`] for the other types.
    fn from_name(name: &str) -> Self {
        [
            Self::Bool,
            Self::Int32,
            Self::Int64,
            Self::Double,
            Self::String,
            Self::ObjectId,
            Self::Date,
            Self::Decimal,
            Self::Binary,
            Self::Document,
            Self::Array,
        ]
        .into_iter()
        .find(|kind| kind.name() == name)
        .unwrap_or(Self::Mixed)
    }
}

/// Integers beyond this lose digits as doubles.
const EXACT_IN_DOUBLE: i64 = 1 << 53;

/// The values a column has to hold: their kinds merged, and whether a long among them is
/// beyond what a double holds exactly.
#[derive(Debug, Default, Clone, Copy)]
struct Seen {
    kind: Option<Kind>,
    big: bool,
}

impl Seen {
    fn add(&mut self, kind: Option<Kind>, big: bool) {
        self.kind = match (self.kind, kind) {
            (Some(a), Some(b)) => Some(a.merge(b)),
            (a, b) => a.or(b),
        };
        self.big |= big;
    }

    fn value(&mut self, value: &Bson) {
        let big =
            matches!(value, Bson::Int64(n) if n.unsigned_abs() > EXACT_IN_DOUBLE.unsigned_abs());
        self.add(Kind::of(value), big);
    }

    /// The column's kind: a double column with such a long is text, which keeps both exact.
    /// A field with only nulls is text too.
    fn kind(self) -> Kind {
        match self.kind.unwrap_or(Kind::String) {
            Kind::Double if self.big => Kind::Mixed,
            kind => kind,
        }
    }
}

/// The fields of `sample` in the order they first appear, with the values seen of each.
fn survey_sample(sample: &[Document]) -> (Vec<&str>, HashMap<&str, Seen>) {
    let mut order: Vec<&str> = Vec::new();
    let mut seen: HashMap<&str, Seen> = HashMap::new();
    for doc in sample {
        for (key, value) in doc {
            seen.entry(key.as_str())
                .or_insert_with(|| {
                    order.push(key);
                    Seen::default()
                })
                .value(value);
        }
    }
    (order, seen)
}

/// The schema of `order`'s fields, `_id` first.
fn schema_of(mut order: Vec<&str>, seen: &HashMap<&str, Seen>) -> SchemaRef {
    if let Some(i) = order.iter().position(|&k| k == "_id") {
        let id = order.remove(i);
        order.insert(0, id);
    }
    let fields: Vec<Field> = order
        .into_iter()
        .map(|name| field(name, seen.get(name).copied().unwrap_or_default().kind()))
        .collect();
    Arc::new(Schema::new(fields))
}

/// The schema of documents like `sample`: their top-level fields, `_id` first, then in
/// the order they first appear. A field with only nulls is text.
#[must_use]
pub fn infer_schema(sample: &[Document]) -> SchemaRef {
    let (order, seen) = survey_sample(sample);
    schema_of(order, &seen)
}

/// Aggregation stages that turn the documents of a result into one document per top-level
/// field: `{ _id: name, types: [$type aliases], big: <a long beyond 2^53> }`.
#[must_use]
pub fn survey_stages() -> Vec<Document> {
    let value = "$_platypus_field.v";
    let big = doc! { "$cond": [
        { "$and": [
            { "$eq": [{ "$type": value }, "long"] },
            { "$or": [
                { "$gt": [value, EXACT_IN_DOUBLE] },
                { "$lt": [value, -EXACT_IN_DOUBLE] },
            ] },
        ] },
        true,
        false,
    ] };
    vec![
        doc! { "$project": { "_id": 0, "_platypus_field": { "$objectToArray": "$$ROOT" } } },
        doc! { "$unwind": "$_platypus_field" },
        doc! { "$group": {
            "_id": "$_platypus_field.k",
            "types": { "$addToSet": { "$type": value } },
            "big": { "$max": big },
        } },
    ]
}

/// The schema of a whole result from the documents of [`survey_stages`]: the fields of the
/// `first` documents in their order, then the fields only later documents have, by name.
/// `None` when the survey is not one.
#[must_use]
pub fn surveyed_schema(first: &[Document], survey: &[Document]) -> Option<SchemaRef> {
    let (mut order, _) = survey_sample(first);
    let mut seen: HashMap<&str, Seen> = HashMap::new();
    let mut late: Vec<&str> = Vec::new();
    for field in survey {
        let name = field.get_str("_id").ok()?;
        let mut s = Seen::default();
        for alias in field.get_array("types").ok()? {
            s.add(Kind::from_alias(alias.as_str()?), false);
        }
        s.add(None, field.get_bool("big").unwrap_or(false));
        if !order.contains(&name) {
            late.push(name);
        }
        seen.insert(name, s);
    }
    late.sort_unstable();
    order.extend(late);
    Some(schema_of(order, &seen))
}

/// `schema` for a result whose later documents were not seen: `int` columns take any
/// integer, as longs, since counters often outgrow 32 bits.
#[must_use]
pub fn widen_for_later(schema: &SchemaRef) -> SchemaRef {
    let fields: Vec<Field> = schema
        .fields()
        .iter()
        .map(|f| match kind_of_field(f) {
            Kind::Int32 => field(f.name(), Kind::Int64),
            _ => f.as_ref().clone(),
        })
        .collect();
    Arc::new(Schema::new(fields))
}

fn field(name: &str, kind: Kind) -> Field {
    Field::new(name, kind.data_type(), true).with_metadata(HashMap::from([(
        META_DB_TYPE.to_owned(),
        kind.name().to_owned(),
    )]))
}

/// A one-column schema for values of any type, e.g. the result of `distinct`.
#[must_use]
pub fn value_schema(name: &str, values: &[Bson]) -> SchemaRef {
    let mut seen = Seen::default();
    values.iter().for_each(|v| seen.value(v));
    Arc::new(Schema::new(vec![field(name, seen.kind())]))
}

/// The documents as a batch of `schema`; fields outside the schema are left out. A value
/// is converted to its column's type when that keeps it exact (an int as a long, a long
/// up to 2^53 as a double, a whole double as an integer); text columns take any value.
///
/// # Errors
/// A value that its column cannot hold exactly is a decode error, never a NULL.
pub fn to_batch(schema: &SchemaRef, docs: &[Document]) -> Result<RecordBatch> {
    let columns = schema
        .fields()
        .iter()
        .map(|f| column(f, docs.iter().map(|d| d.get(f.name()))))
        .collect::<Result<Vec<_>>>()?;
    // No documents means no fields: a batch without columns needs its row count.
    let options = RecordBatchOptions::new().with_row_count(Some(docs.len()));
    Ok(RecordBatch::try_new_with_options(
        Arc::clone(schema),
        columns,
        &options,
    )?)
}

fn kind_of_field(field: &Field) -> Kind {
    field
        .metadata()
        .get(META_DB_TYPE)
        .map_or(Kind::Mixed, |n| Kind::from_name(n))
}

/// The error for a value its column cannot hold.
fn misfit(field: &Field, kind: Kind, value: &Bson) -> Error {
    let found = Kind::of(value).map_or("null", Kind::name);
    let mut shown = text(value).unwrap_or_default();
    if let Some((cut, _)) = shown.char_indices().nth(40) {
        shown.truncate(cut);
        shown.push('…');
    }
    Error::Decode {
        column: field.name().clone(),
        db_type: kind.name().to_owned(),
        message: format!(
            "a later document holds the {found} {shown}, which this column (typed from the \
             first documents) cannot hold exactly; read the field as text with \
             {{ $toString: \"${}\" }} in an aggregation, or filter those documents",
            field.name()
        ),
    }
}

/// Appends each value, or NULL for none, through `convert`; a value it rejects fails.
fn append_all<'a, T>(
    field: &Field,
    kind: Kind,
    values: impl Iterator<Item = Option<&'a Bson>>,
    convert: impl Fn(&Bson) -> Option<T>,
    mut append: impl FnMut(Option<T>),
) -> Result<()> {
    for v in values {
        match v {
            None | Some(Bson::Null | Bson::Undefined) => append(None),
            Some(v) => append(Some(convert(v).ok_or_else(|| misfit(field, kind, v))?)),
        }
    }
    Ok(())
}

fn column<'a>(field: &Field, values: impl Iterator<Item = Option<&'a Bson>>) -> Result<ArrayRef> {
    let kind = kind_of_field(field);
    Ok(match kind {
        Kind::Bool => {
            let mut b = BooleanBuilder::new();
            append_all(field, kind, values, Bson::as_bool, |v| b.append_option(v))?;
            Arc::new(b.finish())
        }
        Kind::Int32 => {
            let mut b = Int32Builder::new();
            let convert = |v: &Bson| as_i64(v).and_then(|n| i32::try_from(n).ok());
            append_all(field, kind, values, convert, |v| b.append_option(v))?;
            Arc::new(b.finish())
        }
        Kind::Int64 => {
            let mut b = Int64Builder::new();
            append_all(field, kind, values, as_i64, |v| b.append_option(v))?;
            Arc::new(b.finish())
        }
        Kind::Double => {
            let mut b = Float64Builder::new();
            append_all(field, kind, values, as_f64, |v| b.append_option(v))?;
            Arc::new(b.finish())
        }
        Kind::Date => {
            let mut b = TimestampMillisecondBuilder::new().with_timezone("+00:00");
            let convert = |v: &Bson| match v {
                Bson::DateTime(d) => Some(d.timestamp_millis()),
                _ => None,
            };
            append_all(field, kind, values, convert, |v| b.append_option(v))?;
            Arc::new(b.finish())
        }
        Kind::Binary => {
            let mut b = BinaryBuilder::new();
            let convert = |v: &Bson| match v {
                Bson::Binary(bin) => Some(bin.bytes.clone()),
                _ => None,
            };
            append_all(field, kind, values, convert, |v| b.append_option(v))?;
            Arc::new(b.finish())
        }
        _ => {
            let mut b = StringBuilder::new();
            values.for_each(|v| b.append_option(v.and_then(text)));
            Arc::new(b.finish())
        }
    })
}

/// An integer value, or a double with a whole value an `i64` holds.
#[expect(
    clippy::cast_possible_truncation,
    reason = "range and fraction are checked first"
)]
fn as_i64(value: &Bson) -> Option<i64> {
    // 2^63 is exactly representable; every double below it in magnitude fits an i64.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    match value {
        Bson::Int32(n) => Some(i64::from(*n)),
        Bson::Int64(n) => Some(*n),
        Bson::Double(d) if d.fract() == 0.0 && (-LIMIT..LIMIT).contains(d) => Some(*d as i64),
        _ => None,
    }
}

/// A double, or an integer a double holds exactly.
#[expect(clippy::cast_precision_loss, reason = "the range is checked first")]
fn as_f64(value: &Bson) -> Option<f64> {
    match value {
        Bson::Int32(n) => Some(f64::from(*n)),
        Bson::Int64(n) if n.unsigned_abs() <= EXACT_IN_DOUBLE.unsigned_abs() => Some(*n as f64),
        Bson::Double(n) => Some(*n),
        _ => None,
    }
}

/// A value as a text cell: strings as they are, `ObjectId`s as hex, dates in RFC 3339 and
/// anything else as relaxed Extended JSON. `None` for null.
#[must_use]
pub fn text(value: &Bson) -> Option<String> {
    Some(match value {
        Bson::Null | Bson::Undefined => return None,
        Bson::String(s) => s.clone(),
        Bson::ObjectId(id) => id.to_hex(),
        Bson::Decimal128(d) => d.to_string(),
        Bson::DateTime(d) => d
            .try_to_rfc3339_string()
            .unwrap_or_else(|_| d.timestamp_millis().to_string()),
        other => other.clone().into_relaxed_extjson().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use arrow_array::cast::AsArray;
    use arrow_array::types::{Int32Type, Int64Type, TimestampMillisecondType};
    use mongodb::bson::{DateTime, doc, oid::ObjectId};

    use super::*;

    fn db_type(schema: &Schema, i: usize) -> &str {
        &schema.field(i).metadata()[META_DB_TYPE]
    }

    #[test]
    fn infers_columns_from_the_sample() {
        let id = ObjectId::new();
        let docs = vec![
            doc! { "name": "a", "_id": id, "n": 1, "tags": ["x"], "when": DateTime::from_millis(5) },
            doc! { "_id": 2, "n": 3_000_000_000_i64, "extra": { "k": true }, "name": null },
            doc! { "_id": 3, "n": 1.5, "flag": true },
        ];
        let schema = infer_schema(&docs);
        let names: Vec<_> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        assert_eq!(names, ["_id", "name", "n", "tags", "when", "extra", "flag"]);
        assert_eq!(db_type(&schema, 0), "mixed");
        assert_eq!(schema.field(1).data_type(), &DataType::Utf8);
        assert_eq!(schema.field(2).data_type(), &DataType::Float64);
        assert_eq!(db_type(&schema, 3), "array");
        assert!(matches!(
            schema.field(4).data_type(),
            DataType::Timestamp(TimeUnit::Millisecond, _)
        ));
        assert_eq!(db_type(&schema, 5), "document");
        assert_eq!(schema.field(6).data_type(), &DataType::Boolean);

        let batch = to_batch(&schema, &docs).unwrap();
        assert_eq!(batch.num_rows(), 3);
        let ids = batch.column(0).as_string::<i32>();
        assert_eq!(ids.value(0), id.to_hex());
        assert_eq!(ids.value(1), "2");
        let n = batch
            .column(2)
            .as_primitive::<arrow_array::types::Float64Type>();
        assert!((n.value(1) - 3e9).abs() < 1.0);
        assert_eq!(batch.column(3).as_string::<i32>().value(0), r#"["x"]"#);
        assert_eq!(batch.column(5).as_string::<i32>().value(1), r#"{"k":true}"#);
        assert!(batch.column(5).is_null(0));
        assert_eq!(
            batch
                .column(4)
                .as_primitive::<TimestampMillisecondType>()
                .value(0),
            5
        );
    }

    #[test]
    fn converts_exactly_or_fails() {
        let schema = infer_schema(&[
            doc! { "a": 1, "b": 1_i64, "d": 0.5 },
            doc! { "a": 2, "b": 2 },
        ]);
        assert_eq!(schema.field(0).data_type(), &DataType::Int32);
        assert_eq!(schema.field(1).data_type(), &DataType::Int64);
        // A later batch with values of other numeric types that fit, and an unknown field.
        let batch = to_batch(
            &schema,
            &[doc! { "a": 7_i64, "b": 3.0, "d": 9_007_199_254_740_992_i64, "c": 1 }],
        )
        .unwrap();
        assert_eq!(batch.column(0).as_primitive::<Int32Type>().value(0), 7);
        assert_eq!(batch.column(1).as_primitive::<Int64Type>().value(0), 3);
        assert_eq!(batch.num_columns(), 3);
        // Values that do not fit are an error naming the column, never a NULL.
        for misfit in [
            doc! { "a": "x" },
            doc! { "a": 3_000_000_000_i64 },
            doc! { "a": 2.5 },
            doc! { "d": 9_007_199_254_740_993_i64 },
        ] {
            let err = to_batch(&schema, &[misfit.clone()])
                .unwrap_err()
                .to_string();
            assert!(err.contains("cannot hold exactly"), "{misfit}: {err}");
        }
        // Widened for later documents, an int column takes longs.
        let wide = widen_for_later(&schema);
        assert_eq!(wide.field(0).data_type(), &DataType::Int64);
        let batch = to_batch(&wide, &[doc! { "a": 3_000_000_000_i64 }]).unwrap();
        assert_eq!(
            batch.column(0).as_primitive::<Int64Type>().value(0),
            3_000_000_000
        );
    }

    #[test]
    fn longs_beyond_doubles_make_text_columns() {
        let schema = infer_schema(&[
            doc! { "n": 0.5, "m": 0.5 },
            doc! { "n": 9_007_199_254_740_993_i64, "m": 9_007_199_254_740_992_i64 },
        ]);
        assert_eq!(schema.field(0).data_type(), &DataType::Utf8);
        assert_eq!(schema.field(1).data_type(), &DataType::Float64);
        let batch = to_batch(&schema, &[doc! { "n": 9_007_199_254_740_993_i64 }]).unwrap();
        assert_eq!(
            batch.column(0).as_string::<i32>().value(0),
            "9007199254740993"
        );
    }

    #[test]
    fn schema_from_a_survey_of_the_whole_result() {
        let first = [doc! { "_id": 1, "qty": 1, "name": "a" }];
        let survey = [
            doc! { "_id": "late", "types": ["string"], "big": false },
            doc! { "_id": "qty", "types": ["int", "double"], "big": false },
            doc! { "_id": "name", "types": ["string", "null"], "big": false },
            doc! { "_id": "_id", "types": ["int"], "big": false },
            doc! { "_id": "big", "types": ["long", "double"], "big": true },
            doc! { "_id": "sub", "types": ["object"], "big": false },
        ];
        let schema = surveyed_schema(&first, &survey).unwrap();
        let names: Vec<_> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        assert_eq!(names, ["_id", "qty", "name", "big", "late", "sub"]);
        assert_eq!(schema.field(1).data_type(), &DataType::Float64);
        assert_eq!(schema.field(3).data_type(), &DataType::Utf8);
        assert_eq!(db_type(&schema, 5), "document");
        assert!(surveyed_schema(&first, &[doc! { "x": 1 }]).is_none());
        assert_eq!(survey_stages().len(), 3);
    }

    #[test]
    fn makes_batches_of_no_documents() {
        let schema = infer_schema(&[]);
        assert!(schema.fields().is_empty());
        assert_eq!(to_batch(&schema, &[]).unwrap().num_rows(), 0);
    }

    #[test]
    fn types_value_columns() {
        let values = [Bson::Int32(1), Bson::Null, Bson::Int32(3)];
        let schema = value_schema("n", &values);
        assert_eq!(schema.field(0).data_type(), &DataType::Int32);
        assert_eq!(value_schema("x", &[]).field(0).data_type(), &DataType::Utf8);
    }
}
