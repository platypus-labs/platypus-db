//! Replies of the REST API as results.
//!
//! - A search (`hits.hits`), a `_mget` or a document `GET` gives documents: the columns are
//!   `_id` (and `_index` when the hits come from several indices), then the top-level fields
//!   of `_source` (and of `fields`, one-item arrays unwrapped), as [`crate::doc`] lays them
//!   out. Aggregations follow as results of their own: a table of the buckets of each
//!   bucket aggregation, and one row for the metrics. An `_msearch` gives one result per
//!   search.
//! - Writes (`_doc`, `_update`, `_bulk`, `*_by_query`) give the number of documents they
//!   changed; a `_bulk` that had errors lists its items.
//! - Anything else is a document: a JSON object is one row of its fields, an array of
//!   objects a row each, other text one row of a `value` column.

use std::sync::Arc;

use platypus_core::{Error, ExecResult, Result};
use serde::Deserialize;
use serde_json::value::RawValue;

use crate::command::{Body, Kind, Request};
use crate::doc::{Doc, Ordered, Table, chain, raw_string};
use crate::http::Payload;
use crate::session::Shared;

#[derive(Deserialize)]
pub struct Hit {
    #[serde(rename = "_index")]
    index: Option<String>,
    #[serde(rename = "_id")]
    id: Option<String>,
    #[serde(rename = "_source")]
    source: Option<Ordered>,
    fields: Option<Ordered>,
    pub sort: Option<Box<RawValue>>,
    found: Option<bool>,
}

#[derive(Deserialize, Default)]
pub struct Hits {
    pub hits: Option<Vec<Hit>>,
}

/// Every shape of reply the driver knows, each field optional.
#[derive(Deserialize, Default)]
struct Probe {
    hits: Option<Hits>,
    #[serde(rename = "_id")]
    id: Option<String>,
    #[serde(rename = "_source")]
    source: Option<Ordered>,
    found: Option<bool>,
    fields: Option<Ordered>,
    #[serde(rename = "_index")]
    index: Option<String>,
    result: Option<String>,
    items: Option<Vec<Box<RawValue>>>,
    errors: Option<bool>,
    responses: Option<Vec<Box<RawValue>>>,
    docs: Option<Vec<Hit>>,
    aggregations: Option<Ordered>,
    created: Option<u64>,
    updated: Option<u64>,
    deleted: Option<u64>,
    failures: Option<Vec<Box<RawValue>>>,
}

/// `fields` values come as arrays of one value.
fn unwrap_single(raw: Box<RawValue>) -> Box<RawValue> {
    match serde_json::from_str::<Vec<Box<RawValue>>>(raw.get()) {
        Ok(mut items) if items.len() == 1 => items.remove(0),
        _ => raw,
    }
}

/// The documents of `hits`; `_index` is a column when `with_index`.
pub fn hit_docs(hits: Vec<Hit>, with_index: bool) -> Result<Vec<Doc>> {
    hits.into_iter()
        .map(|hit| {
            let mut doc: Doc = Vec::new();
            if let Some(id) = &hit.id {
                doc.push(("_id".to_owned(), raw_string(id)?));
            }
            if with_index && let Some(index) = &hit.index {
                doc.push(("_index".to_owned(), raw_string(index)?));
            }
            if let Some(Ordered(source)) = hit.source {
                doc.extend(source);
            }
            if let Some(Ordered(fields)) = hit.fields {
                for (name, value) in fields {
                    if !doc.iter().any(|(n, _)| *n == name) {
                        doc.push((name, unwrap_single(value)));
                    }
                }
            }
            Ok(doc)
        })
        .collect()
}

/// Whether the documents of `hits` need an `_index` column: they come from several indices.
pub fn several_indices(hits: &[Hit]) -> bool {
    let mut indices = hits.iter().filter_map(|h| h.index.as_deref());
    indices
        .next()
        .is_some_and(|first| indices.any(|other| other != first))
}

/// The lead columns of documents.
pub const fn lead(with_index: bool) -> &'static [&'static str] {
    if with_index {
        &["_id", "_index"]
    } else {
        &["_id"]
    }
}

fn hit_table(hits: Vec<Hit>, size: usize) -> Result<Table> {
    let with_index = several_indices(&hits);
    Table::of_docs(&hit_docs(hits, with_index)?, lead(with_index), size)
}

/// Runs a request that is not paged, and shapes its reply.
pub async fn run(shared: &Arc<Shared>, request: &Request, size: usize) -> Result<ExecResult> {
    let mut target = request.target();
    // `_cat` replies are text tables unless asked otherwise.
    if request.method == "GET"
        && request.segments().first().is_some_and(|s| s == "_cat")
        && request.param("format").is_none()
    {
        target.push(if request.query.is_empty() { '?' } else { '&' });
        target.push_str("format=json");
    }
    let payload = match &request.body {
        Body::None => Payload::None,
        Body::Json(text) => Payload::Json(text),
        Body::Ndjson(text) => Payload::Ndjson(text),
    };
    if request.method == "HEAD" {
        let reply = shared.send("HEAD", &target, payload).await?;
        let exists = (200..300).contains(&reply.status);
        let text = format!("{{\"exists\":{exists},\"status\":{}}}", reply.status);
        return generic(&text, size);
    }
    let reply = shared.call(&request.method, &target, payload, None).await?;
    shape(&reply.body, request.kind(), size)
}

/// The result for the body of a reply.
pub fn shape(body: &str, kind: Kind, size: usize) -> Result<ExecResult> {
    let text = body.trim();
    if text.is_empty() {
        return Ok(ExecResult::Affected { rows: 0 });
    }
    if text.starts_with('{') {
        let tables = match serde_json::from_str::<Probe>(text) {
            Ok(probe) => {
                if kind == Kind::Dml
                    && let Some(rows) = written(&probe)?
                {
                    return Ok(ExecResult::Affected { rows });
                }
                tables(probe, size)?
            }
            Err(_) => Vec::new(),
        };
        if let Some(result) = chain(tables) {
            return Ok(result);
        }
    }
    generic(text, size)
}

/// The tables a reply holds; none when it is not a search or a document.
fn tables(probe: Probe, size: usize) -> Result<Vec<Table>> {
    let mut tables = Vec::new();
    let has_aggregations = probe.aggregations.is_some();
    if let Some(Hits { hits: Some(hits) }) = probe.hits {
        if !hits.is_empty() || !has_aggregations {
            tables.push(hit_table(hits, size)?);
        }
    } else if let Some(docs) = probe.docs {
        let found = docs
            .into_iter()
            .filter(|d| d.id.is_some() && d.found != Some(false))
            .collect();
        tables.push(hit_table(found, size)?);
    } else if let Some(items) = probe.items {
        tables.push(item_table(&items, size)?);
    } else if let Some(responses) = probe.responses {
        for response in responses {
            let reply = serde_json::from_str::<Probe>(response.get()).unwrap_or_default();
            let found = reply.hits.is_some() || reply.aggregations.is_some();
            if found {
                tables.extend(self::tables(reply, size)?);
            } else {
                tables.push(object_table(response.get(), size)?);
            }
        }
    } else if probe.id.is_some()
        && (probe.source.is_some() || probe.fields.is_some() || probe.found.is_some())
    {
        let hit = Hit {
            index: probe.index,
            id: probe.id,
            source: probe.source,
            fields: probe.fields,
            sort: None,
            found: None,
        };
        tables.push(hit_table(vec![hit], size)?);
    }
    if let Some(aggregations) = probe.aggregations {
        tables.extend(aggregation_tables(aggregations.0, size)?);
    }
    Ok(tables)
}

/// The items of a `_bulk` reply, a row each: the action, then what the server says of it.
fn item_table(items: &[Box<RawValue>], size: usize) -> Result<Table> {
    let docs = items
        .iter()
        .map(|item| {
            let Ordered(fields) = serde_json::from_str(item.get())
                .map_err(|e| Error::Other(format!("unexpected bulk item: {e}")))?;
            let mut doc: Doc = Vec::new();
            for (action, outcome) in fields {
                doc.push(("action".to_owned(), raw_string(&action)?));
                if let Ok(Ordered(inner)) = serde_json::from_str(outcome.get()) {
                    doc.extend(inner);
                }
            }
            Ok(doc)
        })
        .collect::<Result<Vec<Doc>>>()?;
    Table::of_docs(&docs, &["action"], size)
}

/// How many documents a write reply says it changed; `None` when it is not one.
fn written(probe: &Probe) -> Result<Option<u64>> {
    if let Some(items) = &probe.items {
        if probe.errors == Some(true) {
            return Ok(None);
        }
        return Ok(Some(items.len() as u64));
    }
    if let Some(result) = &probe.result {
        return Ok(Some(u64::from(result != "not_found")));
    }
    let counts = [probe.created, probe.updated, probe.deleted];
    if counts.iter().all(Option::is_none) {
        return Ok(None);
    }
    if let Some(first) = probe.failures.as_ref().and_then(|f| f.first()) {
        let n = probe.failures.as_ref().map_or(0, Vec::len);
        return Err(Error::Other(format!(
            "{n} documents failed: {}",
            first.get()
        )));
    }
    Ok(Some(counts.iter().flatten().sum()))
}

/// The buckets of each bucket aggregation as a table, and the metrics as one row first.
fn aggregation_tables(aggregations: Doc, size: usize) -> Result<Vec<Table>> {
    let mut metrics: Doc = Vec::new();
    let mut tables = Vec::new();
    for (name, value) in aggregations {
        match bucket_docs(&value) {
            Some(docs) => tables.push(Table::of_docs(&docs, &[], size)?),
            None => metrics.push((name, metric_value(value))),
        }
    }
    if !metrics.is_empty() {
        tables.insert(0, Table::of_docs(&[metrics], &[], size)?);
    }
    Ok(tables)
}

/// The buckets of an aggregation, one document each (a keyed object of buckets gets a `key`
/// field); `None` when it has none.
fn bucket_docs(aggregation: &RawValue) -> Option<Vec<Doc>> {
    let Ordered(fields) = serde_json::from_str(aggregation.get()).ok()?;
    let (_, buckets) = fields.into_iter().find(|(k, _)| k == "buckets")?;
    if buckets.get().starts_with('[') {
        let list: Vec<Ordered> = serde_json::from_str(buckets.get()).ok()?;
        return Some(list.into_iter().map(|o| o.0).collect());
    }
    let Ordered(keyed) = serde_json::from_str(buckets.get()).ok()?;
    keyed
        .into_iter()
        .map(|(key, bucket)| {
            let Ordered(mut fields) = serde_json::from_str(bucket.get()).ok()?;
            fields.insert(0, ("key".to_owned(), raw_string(&key).ok()?));
            Some(fields)
        })
        .collect()
}

/// A metric aggregation's value: `{"value": 3}` is 3.
fn metric_value(aggregation: Box<RawValue>) -> Box<RawValue> {
    let fields: Option<Ordered> = serde_json::from_str(aggregation.get()).ok();
    match fields {
        Some(Ordered(fields))
            if fields
                .iter()
                .all(|(k, _)| k == "value" || k == "value_as_string") =>
        {
            fields
                .into_iter()
                .find(|(k, _)| k == "value")
                .map_or(aggregation, |(_, v)| v)
        }
        _ => aggregation,
    }
}

/// A JSON object as one row of its fields.
fn object_table(text: &str, size: usize) -> Result<Table> {
    let Ordered(fields) = serde_json::from_str(text).map_err(|e| Error::Other(e.to_string()))?;
    Table::of_docs(&[fields], &[], size)
}

/// Any other reply: an object is a row, an array of objects a row each, else text.
fn generic(text: &str, size: usize) -> Result<ExecResult> {
    let text = text.trim();
    if text.starts_with('{')
        && let Ok(table) = object_table(text, size)
    {
        return Ok(table.into_result());
    }
    if text.starts_with('[') {
        if let Ok(list) = serde_json::from_str::<Vec<Ordered>>(text) {
            let docs: Vec<Doc> = list.into_iter().map(|o| o.0).collect();
            return Ok(Table::of_docs(&docs, &[], size)?.into_result());
        }
        if let Ok(items) = serde_json::from_str::<Vec<Box<RawValue>>>(text) {
            let docs: Vec<Doc> = items
                .into_iter()
                .map(|item| vec![("value".to_owned(), item)])
                .collect();
            return Ok(Table::of_docs(&docs, &["value"], size)?.into_result());
        }
    }
    let value = match serde_json::from_str::<String>(text) {
        Ok(unquoted) => unquoted,
        Err(_) => text.to_owned(),
    };
    Ok(Table::scalar(&value)?.into_result())
}

#[cfg(test)]
mod tests {
    use arrow_array::{Array, Int64Array, StringArray};
    use futures::TryStreamExt;
    use platypus_core::arrow_array::RecordBatch;

    use super::*;

    async fn results(body: &str, kind: Kind) -> Vec<Vec<RecordBatch>> {
        let mut out = Vec::new();
        let mut next = Some(shape(body, kind, 100).unwrap());
        while let Some(ExecResult::Rows(mut stream)) = next.take() {
            let after = stream.take_next();
            let schema = stream.schema();
            let mut batches: Vec<RecordBatch> = (&mut stream).try_collect().await.unwrap();
            if batches.is_empty() {
                batches.push(RecordBatch::new_empty(schema));
            }
            out.push(batches);
            if let Some(after) = after {
                next = after.await.unwrap();
            }
        }
        out
    }

    fn columns(batches: &[RecordBatch]) -> Vec<String> {
        batches[0]
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().clone())
            .collect()
    }

    #[tokio::test]
    async fn searches_give_documents() {
        let body = r#"{"took":1,"hits":{"total":{"value":2},"hits":[
            {"_index":"a","_id":"1","_score":1.0,"_source":{"name":"x","n":1}},
            {"_index":"a","_id":"2","_source":{"name":"y","n":2,"tags":["t"]}}]}}"#;
        let r = results(body, Kind::Query).await;
        assert_eq!(columns(&r[0]), ["_id", "name", "n", "tags"]);
        assert_eq!(r[0][0].num_rows(), 2);
        let several = body.replace(r#""_index":"a","_id":"2""#, r#""_index":"b","_id":"2""#);
        let r = results(&several, Kind::Query).await;
        assert_eq!(columns(&r[0]), ["_id", "_index", "name", "n", "tags"]);
        let empty = r#"{"hits":{"hits":[]}}"#;
        assert_eq!(columns(&results(empty, Kind::Query).await[0]), ["_id"]);
    }

    #[tokio::test]
    async fn a_document_and_fields() {
        let body = r#"{"_index":"a","_id":"7","_version":1,"found":true,"_source":{"k":"v"}}"#;
        let r = results(body, Kind::Query).await;
        assert_eq!(columns(&r[0]), ["_id", "k"]);
        let fields = r#"{"hits":{"hits":[{"_id":"1","fields":{"a":[1],"b":["x","y"]}}]}}"#;
        let r = results(fields, Kind::Query).await;
        let b = r[0][0]
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(b.value(0), r#"["x","y"]"#);
    }

    #[tokio::test]
    async fn aggregations_follow_the_hits() {
        let body = r#"{"hits":{"hits":[]},"aggregations":{
            "avg_n":{"value":2.5},
            "by_tag":{"buckets":[{"key":"a","doc_count":2},{"key":"b","doc_count":1}]},
            "by_filter":{"buckets":{"x":{"doc_count":3}}}}}"#;
        let r = results(body, Kind::Query).await;
        assert_eq!(r.len(), 3, "metrics, and one table per bucket aggregation");
        assert_eq!(columns(&r[0]), ["avg_n"]);
        assert_eq!(columns(&r[1]), ["key", "doc_count"]);
        let counts = r[1][0]
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(counts.values(), &[2, 1]);
        assert_eq!(columns(&r[2]), ["key", "doc_count"]);
    }

    #[tokio::test]
    async fn msearch_gives_a_result_per_search() {
        let body = r#"{"responses":[{"hits":{"hits":[{"_id":"1","_source":{"a":1}}]}},
            {"error":{"type":"x","reason":"y"},"status":400}]}"#;
        let r = results(body, Kind::Query).await;
        assert_eq!(r.len(), 2);
        assert_eq!(columns(&r[1]), ["error", "status"]);
    }

    #[test]
    fn counts_written_documents() {
        let rows = |body: &str| match shape(body, Kind::Dml, 10).unwrap() {
            ExecResult::Affected { rows } => rows,
            other @ ExecResult::Rows(_) => panic!("{other:?}"),
        };
        assert_eq!(rows(r#"{"_id":"1","result":"created"}"#), 1);
        assert_eq!(rows(r#"{"_id":"1","result":"deleted"}"#), 1);
        assert_eq!(rows(r#"{"_id":"1","result":"not_found"}"#), 0);
        assert_eq!(
            rows(r#"{"errors":false,"items":[{"index":{}},{"index":{}}]}"#),
            2
        );
        assert_eq!(rows(r#"{"updated":3,"deleted":0,"failures":[]}"#), 3);
        assert_eq!(rows(r#"{"deleted":4,"total":4}"#), 4);
        let failed = shape(r#"{"updated":1,"failures":[{"cause":"x"}]}"#, Kind::Dml, 10);
        assert!(matches!(failed, Err(Error::Other(m)) if m.contains("1 documents failed")));
    }

    #[tokio::test]
    async fn a_bulk_with_errors_lists_its_items_and_other_replies_are_documents() {
        let body = r#"{"errors":true,"items":[{"index":{"_id":"1","status":201}},{"index":{"_id":"2","status":400,"error":{"type":"e"}}}]}"#;
        let r = results(body, Kind::Dml).await;
        assert_eq!(columns(&r[0]), ["action", "_id", "status", "error"]);
        assert_eq!(r[0][0].num_rows(), 2);
        let r = results(r#"{"acknowledged":true,"index":"i"}"#, Kind::Ddl).await;
        assert_eq!(columns(&r[0]), ["acknowledged", "index"]);
        let r = results(
            r#"[{"index":"a","docs.count":"3"},{"index":"b"}]"#,
            Kind::Query,
        )
        .await;
        assert_eq!(r[0][0].num_rows(), 2);
        let r = results("plain text", Kind::Query).await;
        assert_eq!(columns(&r[0]), ["value"]);
        let r = results(r#"["a","b"]"#, Kind::Query).await;
        assert_eq!(r[0][0].num_rows(), 2);
    }
}
