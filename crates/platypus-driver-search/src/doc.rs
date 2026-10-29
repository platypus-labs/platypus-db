//! Documents to Arrow, as the MongoDB driver shows them: one column per top-level field of
//! the first documents of a result, in the order the server wrote them. A field whose
//! values are all booleans, integers or numbers gets the matching Arrow type; nested objects
//! and arrays stay JSON text, and fields of mixed types text too. The column's
//! [`META_DB_TYPE`] says what it holds (`boolean`, `long`, `double`, `string`, `document`,
//! `array`, `mixed`), so the UI can show objects and arrays as a tree.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use arrow_array::{ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use futures::stream;
use futures::{FutureExt, StreamExt};
use platypus_core::{Error, ExecResult, META_DB_TYPE, RecordBatchStream, Result};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::value::RawValue;

/// The fields of a JSON object, in order, each value as the text it was written in.
pub type Doc = Vec<(String, Box<RawValue>)>;

/// A JSON object read in order (`serde_json`'s own maps sort their keys).
#[derive(Debug)]
pub struct Ordered(pub Doc);

impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = Ordered;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Ordered, A::Error> {
                let mut fields = Vec::new();
                while let Some(entry) = map.next_entry::<String, Box<RawValue>>()? {
                    fields.push(entry);
                }
                Ok(Ordered(fields))
            }
        }
        d.deserialize_map(Fields)
    }
}

/// `text` as a JSON string value.
pub fn raw_string(text: &str) -> Result<Box<RawValue>> {
    serde_json::value::to_raw_value(text).map_err(|e| Error::Other(e.to_string()))
}

/// The type of a column, as far as the Arrow type goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bool,
    Int,
    Double,
    Text,
    Doc,
    Array,
    Mixed,
}

impl Kind {
    /// The kind of a value; `None` for null.
    fn of(raw: &RawValue) -> Option<Self> {
        let text = raw.get();
        Some(match text.as_bytes().first()? {
            b'n' => return None,
            b'{' => Self::Doc,
            b'[' => Self::Array,
            b'"' => Self::Text,
            b't' | b'f' => Self::Bool,
            _ if text.parse::<i64>().is_ok() => Self::Int,
            // An integer past 64 bits stays text, whole.
            _ if text.bytes().all(|b| b.is_ascii_digit() || b == b'-') => Self::Mixed,
            _ => Self::Double,
        })
    }

    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (a, b) if a == b => a,
            (Self::Int, Self::Double) | (Self::Double, Self::Int) => Self::Double,
            _ => Self::Mixed,
        }
    }

    const fn data_type(self) -> DataType {
        match self {
            Self::Bool => DataType::Boolean,
            Self::Int => DataType::Int64,
            Self::Double => DataType::Float64,
            _ => DataType::Utf8,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Bool => "boolean",
            Self::Int => "long",
            Self::Double => "double",
            Self::Text => "string",
            Self::Doc => "document",
            Self::Array => "array",
            Self::Mixed => "mixed",
        }
    }
}

/// The text of a cell: a string as it is, anything else as the JSON it was; `None` for null.
fn text_of(raw: &RawValue) -> Option<String> {
    let text = raw.get();
    match text.as_bytes().first()? {
        b'n' => None,
        b'"' => serde_json::from_str(text).ok(),
        _ => Some(text.to_owned()),
    }
}

/// The columns of a result, fixed by its first documents.
#[derive(Debug, Clone)]
pub struct Layout {
    names: Vec<String>,
    kinds: Vec<Kind>,
    schema: SchemaRef,
}

impl Layout {
    /// The columns of `docs`; those of `lead` come first, whether the documents have them or
    /// not.
    #[must_use]
    pub fn infer(docs: &[Doc], lead: &[&str]) -> Self {
        let mut names: Vec<String> = lead.iter().map(|&n| n.to_owned()).collect();
        let mut kinds: Vec<Option<Kind>> = vec![Some(Kind::Text); names.len()];
        let mut index: HashMap<String, usize> = names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), i))
            .collect();
        for doc in docs {
            for (name, value) in doc {
                let at = *index.entry(name.clone()).or_insert_with(|| {
                    names.push(name.clone());
                    kinds.push(None);
                    names.len() - 1
                });
                if let Some(kind) = Kind::of(value) {
                    kinds[at] = Some(kinds[at].map_or(kind, |k| k.merge(kind)));
                }
            }
        }
        let kinds: Vec<Kind> = kinds.into_iter().map(|k| k.unwrap_or(Kind::Text)).collect();
        let schema = Arc::new(Schema::new(
            names
                .iter()
                .zip(&kinds)
                .map(|(name, kind)| {
                    Field::new(name, kind.data_type(), true).with_metadata(HashMap::from([(
                        META_DB_TYPE.to_owned(),
                        kind.name().to_owned(),
                    )]))
                })
                .collect::<Vec<_>>(),
        ));
        Self {
            names,
            kinds,
            schema,
        }
    }

    #[must_use]
    pub fn schema(&self) -> SchemaRef {
        Arc::clone(&self.schema)
    }

    /// The rows of `docs`; fields the columns do not have are left out.
    pub fn batch(&self, docs: &[Doc]) -> Result<RecordBatch> {
        let columns = (0..self.names.len())
            .map(|c| self.column(c, docs))
            .collect::<Result<Vec<_>>>()?;
        Ok(RecordBatch::try_new_with_options(
            self.schema(),
            columns,
            &arrow_array::RecordBatchOptions::new().with_row_count(Some(docs.len())),
        )?)
    }

    fn column(&self, c: usize, docs: &[Doc]) -> Result<ArrayRef> {
        let name = &self.names[c];
        let cells = docs.iter().map(|doc| {
            // Documents mostly list their fields in the same order.
            match doc.get(c) {
                Some((n, v)) if n == name => Some(v),
                _ => doc.iter().find(|(n, _)| n == name).map(|(_, v)| v),
            }
            .filter(|v| v.get() != "null")
        });
        let kind = self.kinds[c];
        let bad = |value: &str| Error::Decode {
            column: name.clone(),
            db_type: kind.name().to_owned(),
            message: format!(
                "`{}` does not fit the column",
                value.chars().take(40).collect::<String>()
            ),
        };
        Ok(match kind {
            Kind::Bool => Arc::new(
                cells
                    .map(|v| {
                        v.map(|v| v.get().parse::<bool>().map_err(|_| bad(v.get())))
                            .transpose()
                    })
                    .collect::<Result<BooleanArray>>()?,
            ),
            Kind::Int => Arc::new(
                cells
                    .map(|v| {
                        v.map(|v| v.get().parse::<i64>().map_err(|_| bad(v.get())))
                            .transpose()
                    })
                    .collect::<Result<Int64Array>>()?,
            ),
            Kind::Double => Arc::new(
                cells
                    .map(|v| {
                        v.map(|v| v.get().parse::<f64>().map_err(|_| bad(v.get())))
                            .transpose()
                    })
                    .collect::<Result<Float64Array>>()?,
            ),
            _ => Arc::new(
                cells
                    .map(|v| v.and_then(|v| text_of(v)))
                    .collect::<StringArray>(),
            ),
        })
    }
}

/// A result held in memory.
#[derive(Debug)]
pub struct Table {
    pub schema: SchemaRef,
    pub batches: Vec<RecordBatch>,
}

impl Table {
    /// The documents as rows, in batches of `size`.
    pub fn of_docs(docs: &[Doc], lead: &[&str], size: usize) -> Result<Self> {
        let layout = Layout::infer(docs, lead);
        let batches = docs
            .chunks(size.max(1))
            .map(|chunk| layout.batch(chunk))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            schema: layout.schema(),
            batches,
        })
    }

    /// One row of one `value` column.
    pub fn scalar(text: &str) -> Result<Self> {
        let value = raw_string(text)?;
        Self::of_docs(&[vec![("value".to_owned(), value)]], &[], 1)
    }

    /// The rows as a result.
    #[must_use]
    pub fn into_result(self) -> ExecResult {
        let batches: Vec<Result<RecordBatch>> = self.batches.into_iter().map(Ok).collect();
        ExecResult::Rows(RecordBatchStream::new(
            self.schema,
            stream::iter(batches).boxed(),
        ))
    }
}

/// Several results as one, the later ones following the earlier as a statement's next
/// results.
///
/// `None` when there are no tables.
#[must_use]
pub fn chain(mut tables: Vec<Table>) -> Option<ExecResult> {
    if tables.is_empty() {
        return None;
    }
    let first = tables.remove(0);
    let ExecResult::Rows(stream) = first.into_result() else {
        return None;
    };
    if tables.is_empty() {
        return Some(ExecResult::Rows(stream));
    }
    let rest = futures::future::ready(Ok(chain(tables))).boxed();
    Some(ExecResult::Rows(stream.with_next(Some(rest))))
}

#[cfg(test)]
mod tests {
    use arrow_array::Array;

    use super::*;

    fn docs(text: &str) -> Vec<Doc> {
        let items: Vec<Ordered> = serde_json::from_str(text).unwrap();
        items.into_iter().map(|o| o.0).collect()
    }

    fn db_types(layout: &Layout) -> Vec<(String, String)> {
        layout
            .schema()
            .fields()
            .iter()
            .map(|f| (f.name().clone(), f.metadata()[META_DB_TYPE].clone()))
            .collect()
    }

    #[test]
    fn columns_follow_the_first_documents() {
        let docs = docs(
            r#"[{"_id":"1","n":1,"price":1.5,"ok":true,"tags":["a"],"owner":{"b":1,"a":2},"mix":1},
                {"_id":"2","n":2,"price":2,"ok":null,"extra":"x","mix":"s"}]"#,
        );
        let layout = Layout::infer(&docs, &["_id"]);
        let types: Vec<_> = db_types(&layout);
        let want = [
            ("_id", "string"),
            ("n", "long"),
            ("price", "double"),
            ("ok", "boolean"),
            ("tags", "array"),
            ("owner", "document"),
            ("mix", "mixed"),
            ("extra", "string"),
        ];
        assert_eq!(types, want.map(|(a, b)| (a.to_owned(), b.to_owned())));
        let batch = layout.batch(&docs).unwrap();
        assert_eq!(batch.num_rows(), 2);
        let owner = batch
            .column(5)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(
            owner.value(0),
            r#"{"b":1,"a":2}"#,
            "the order of the server"
        );
        assert!(owner.is_null(1));
        let price = batch
            .column(2)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert_eq!((price.value(0), price.value(1)), (1.5, 2.0));
        let mix = batch
            .column(6)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!((mix.value(0), mix.value(1)), ("1", "s"));
    }

    #[test]
    fn lead_columns_exist_without_documents() {
        let layout = Layout::infer(&[], &["_id", "_index"]);
        assert_eq!(layout.schema().fields().len(), 2);
        let table = Table::of_docs(&[], &["_id"], 10).unwrap();
        assert_eq!(table.batches.len(), 0);
        assert_eq!(table.schema.fields().len(), 1);
    }

    #[test]
    fn a_value_that_does_not_fit_is_an_error() {
        let first = docs(r#"[{"n":1}]"#);
        let layout = Layout::infer(&first, &[]);
        let later = docs(r#"[{"n":"x"}]"#);
        assert!(matches!(layout.batch(&later), Err(Error::Decode { .. })));
    }

    #[test]
    fn chains_results() {
        let a = Table::scalar("a").unwrap();
        let b = Table::scalar("b").unwrap();
        let Some(ExecResult::Rows(mut first)) = chain(vec![a, b]) else {
            panic!()
        };
        assert!(first.take_next().is_some());
        assert!(chain(Vec::new()).is_none());
    }
}
