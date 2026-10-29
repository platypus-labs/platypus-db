//! Searches that return more than a page. A `_search` of an index (or of all) that has no
//! `from`, `search_after`, `pit`, aggregations or `rescore` of its own, and that asks for
//! more hits than one page holds (or for no `size`, meaning all of them), is read page by
//! page, the next one only when the reader asks for it:
//!
//! - on Elasticsearch 7.12 and later, with a point in time and `search_after`, sorted by
//!   `_shard_doc` unless the request sorts;
//! - otherwise (older Elasticsearch, `OpenSearch`) with a scroll.
//!
//! A `size` is the most hits read. The context on the server is closed when the result is
//! dropped.

use std::sync::Arc;

use arrow_array::RecordBatch;
use futures::StreamExt;
use futures::stream;
use platypus_core::{Error, ExecResult, RecordBatchStream, Result, ServerVersion};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::command::{Body, Request};
use crate::doc::Layout;
use crate::http::Payload;
use crate::meta::{get, top_level_fields};
use crate::reply::{Hit, Hits, hit_docs, lead, several_indices};
use crate::session::{Flavor, Shared};
use crate::target::Target;

/// How long the server keeps a point in time or a scroll between two pages.
const KEEP_ALIVE: &str = "2m";
/// The most hits a page can hold (`index.max_result_window`).
const MAX_PAGE: usize = 10_000;
/// Query parameters that do not change the search.
const HARMLESS: &[&str] = &["pretty", "typed_keys", "human"];
/// Body fields that keep a search from being paged.
const UNPAGEABLE: &[&str] = &[
    "from",
    "search_after",
    "pit",
    "aggs",
    "aggregations",
    "rescore",
    "scroll",
];

/// A search to read page by page.
#[derive(Debug)]
pub struct Plan {
    body: Map<String, Value>,
    /// The index expression of the path; empty for all.
    index: String,
    /// The most hits to read.
    limit: Option<u64>,
    page: usize,
}

/// The plan for `request`, if it is a search to page.
pub fn plan(request: &Request, batch_size: usize) -> Option<Plan> {
    if !matches!(request.method.as_str(), "GET" | "POST") {
        return None;
    }
    if request
        .target
        .query()
        .split('&')
        .filter(|p| !p.is_empty())
        .any(|p| !HARMLESS.contains(&p.split('=').next().unwrap_or_default()))
    {
        return None;
    }
    let parts: Vec<&str> = request
        .path()
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();
    let index = match parts.as_slice() {
        ["_search"] => "",
        [index, "_search"] if !index.starts_with('_') || *index == "_all" => index,
        _ => return None,
    };
    let body = match &request.body {
        Body::None => Map::new(),
        Body::Json(text) => match serde_json::from_str(text) {
            Ok(Value::Object(map)) => map,
            _ => return None,
        },
        Body::Ndjson(_) => return None,
    };
    if UNPAGEABLE.iter().any(|k| body.contains_key(*k)) {
        return None;
    }
    let page = batch_size.clamp(1, MAX_PAGE);
    let limit = match body.get("size") {
        None => None,
        Some(size) => Some(size.as_u64()?),
    };
    if limit.is_some_and(|l| l <= page as u64) {
        return None;
    }
    Some(Plan {
        body,
        index: index.to_owned(),
        limit,
        page,
    })
}

enum Mode {
    /// The id of a point in time.
    Pit(String),
    /// The id of the scroll, once the first page has opened it.
    Scroll(Option<String>),
}

#[derive(Deserialize)]
struct Page {
    pit_id: Option<String>,
    #[serde(rename = "_scroll_id")]
    scroll_id: Option<String>,
    hits: Option<Hits>,
}

#[derive(Deserialize)]
struct Opened {
    id: Option<String>,
    pit_id: Option<String>,
}

struct Pager {
    shared: Arc<Shared>,
    plan: Plan,
    mode: Mode,
    /// The sort values of the last hit read.
    after: Option<Value>,
    left: Option<u64>,
    done: bool,
    layout: Layout,
    with_index: bool,
    /// The first page, not yet given to the reader.
    first: Option<RecordBatch>,
}

impl Drop for Pager {
    fn drop(&mut self) {
        match &self.mode {
            Mode::Pit(id) => {
                self.shared
                    .later("DELETE", "/_pit", json!({ "id": id }).to_string());
            }
            Mode::Scroll(Some(id)) => {
                self.shared.later(
                    "DELETE",
                    "/_search/scroll",
                    json!({ "scroll_id": id }).to_string(),
                );
            }
            Mode::Scroll(None) => {}
        }
    }
}

fn point_in_time(shared: &Shared) -> bool {
    shared.flavor == Flavor::Elasticsearch && shared.version >= ServerVersion::new(7, 12, 0)
}

impl Pager {
    /// Reads the next page of hits.
    async fn fetch(&mut self) -> Result<Vec<Hit>> {
        let size = self.left.map_or(self.plan.page, |left| {
            self.plan
                .page
                .min(usize::try_from(left).unwrap_or(usize::MAX))
        });
        let mut body = self.plan.body.clone();
        body.insert("size".to_owned(), json!(size));
        let (target, body) = match &self.mode {
            Mode::Pit(id) => {
                body.insert(
                    "pit".to_owned(),
                    json!({ "id": id, "keep_alive": KEEP_ALIVE }),
                );
                body.entry("track_total_hits").or_insert(json!(false));
                body.entry("sort")
                    .or_insert(json!([{ "_shard_doc": "asc" }]));
                if let Some(after) = &self.after {
                    body.insert("search_after".to_owned(), after.clone());
                }
                ("/_search".to_owned(), Value::Object(body))
            }
            Mode::Scroll(None) => {
                body.entry("sort").or_insert(json!(["_doc"]));
                let index = &self.plan.index;
                let slash = if index.is_empty() { "" } else { "/" };
                (
                    format!("{slash}{index}/_search?scroll={KEEP_ALIVE}"),
                    Value::Object(body),
                )
            }
            Mode::Scroll(Some(id)) => (
                "/_search/scroll".to_owned(),
                json!({ "scroll": KEEP_ALIVE, "scroll_id": id }),
            ),
        };
        let reply = self
            .shared
            .call(
                "POST",
                &Target::new(&target)?,
                Payload::Json(&body.to_string()),
                None,
            )
            .await?;
        let page: Page = serde_json::from_str(&reply.body)
            .map_err(|e| Error::Other(format!("unexpected search reply: {e}")))?;
        match &mut self.mode {
            Mode::Pit(id) => {
                if let Some(new) = page.pit_id {
                    *id = new;
                }
            }
            Mode::Scroll(id) => {
                if page.scroll_id.is_some() {
                    *id = page.scroll_id;
                }
            }
        }
        let mut hits = page.hits.and_then(|h| h.hits).unwrap_or_default();
        // A scroll goes on with the size it started with.
        hits.truncate(size);
        self.after = hits
            .last()
            .and_then(|h| h.sort.as_ref())
            .and_then(|s| serde_json::from_str(s.get()).ok());
        let n = hits.len() as u64;
        self.left = self.left.map(|left| left.saturating_sub(n));
        self.done = hits.len() < size
            || self.left == Some(0)
            || (self.after.is_none() && matches!(self.mode, Mode::Pit(_)));
        Ok(hits)
    }
}

/// Opens the context the pages are read in: a point in time where the server has them, else
/// a scroll (which the first page opens).
async fn open(shared: &Arc<Shared>, plan: &Plan) -> Result<Mode> {
    if !point_in_time(shared) {
        return Ok(Mode::Scroll(None));
    }
    let index = if plan.index.is_empty() {
        "*"
    } else {
        &plan.index
    };
    let target = format!("/{index}/_pit?keep_alive={KEEP_ALIVE}");
    match shared
        .call("POST", &Target::new(&target)?, Payload::None, None)
        .await
    {
        Ok(reply) => {
            let opened: Opened = serde_json::from_str(&reply.body)
                .map_err(|e| Error::Other(format!("unexpected reply opening a search: {e}")))?;
            match opened.id.or(opened.pit_id) {
                Some(id) => Ok(Mode::Pit(id)),
                None => Ok(Mode::Scroll(None)),
            }
        }
        // A server that has none, or an index that is not there: the scroll tells which.
        Err(Error::Database(_)) => Ok(Mode::Scroll(None)),
        Err(e) => Err(e),
    }
}

/// The top-level fields of the mappings of the indices `index` names, none when it names all
/// of them or the server will not tell.
async fn mapped_fields(shared: &Shared, index: &str) -> Vec<(String, String)> {
    if index.is_empty() || index == "_all" {
        return Vec::new();
    }
    let target = format!("/{index}/_mapping?expand_wildcards=open");
    get(shared, &target)
        .await
        .map(|v| top_level_fields(&v))
        .unwrap_or_default()
}

/// Runs the search and returns its first page; the others follow as the result is read.
pub async fn run(shared: &Arc<Shared>, plan: Plan) -> Result<ExecResult> {
    let mode = open(shared, &plan).await?;
    let multiple = plan.index.is_empty() || plan.index.contains([',', '*']) || plan.index == "_all";
    let mut pager = Pager {
        shared: Arc::clone(shared),
        left: plan.limit,
        plan,
        mode,
        after: None,
        done: false,
        layout: Layout::infer(&[], &[]),
        with_index: false,
        first: None,
    };
    let hits = pager.fetch().await?;
    pager.with_index = multiple || several_indices(&hits);
    let docs = hit_docs(hits, pager.with_index)?;
    // The columns stay the same on every page: the mapping's fields, those of the first
    // page, and `_other` for what only later pages have.
    let fields = mapped_fields(shared, &pager.plan.index).await;
    pager.layout = Layout::paged(&docs, lead(pager.with_index), &fields, !pager.done);
    let schema = pager.layout.schema();
    if !docs.is_empty() {
        pager.first = Some(pager.layout.batch(&docs)?);
    }
    let batches = stream::try_unfold(pager, next_batch).boxed();
    Ok(ExecResult::Rows(RecordBatchStream::new(schema, batches)))
}

async fn next_batch(mut pager: Pager) -> Result<Option<(RecordBatch, Pager)>> {
    if let Some(batch) = pager.first.take() {
        return Ok(Some((batch, pager)));
    }
    if pager.done {
        return Ok(None);
    }
    let hits = pager.fetch().await?;
    if hits.is_empty() {
        return Ok(None);
    }
    let docs = hit_docs(hits, pager.with_index)?;
    let batch = pager.layout.batch(&docs)?;
    Ok(Some((batch, pager)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, parse};

    fn plan_of(text: &str, batch: usize) -> Option<Plan> {
        let Command::Request(request) = parse(text).unwrap() else {
            panic!()
        };
        plan(&request, batch)
    }

    #[test]
    fn pages_searches_that_can_be() {
        let p = plan_of("GET /orders/_search\n{\"query\":{\"match_all\":{}}}", 100).unwrap();
        assert_eq!((p.index.as_str(), p.limit, p.page), ("orders", None, 100));
        assert!(plan_of("GET /_search", 100).is_some());
        assert!(plan_of("POST /a,b*/_search?pretty", 100).is_some());
        let p = plan_of("GET /i/_search\n{\"size\":5000}", 100).unwrap();
        assert_eq!(p.limit, Some(5000));
        assert_eq!(plan_of("GET /i/_search", 50_000).unwrap().page, 10_000);
    }

    #[test]
    fn leaves_the_rest_alone() {
        for text in [
            "GET /i/_search\n{\"size\":10}",
            "GET /i/_search\n{\"size\":0}",
            "GET /i/_search\n{\"from\":10}",
            "GET /i/_search\n{\"search_after\":[1]}",
            "GET /i/_search\n{\"aggs\":{}}",
            "GET /i/_search?size=3",
            "GET /i/_search?scroll=1m",
            "GET /i/_search/template",
            "GET /i/_count",
            "GET /_search/scroll",
            "PUT /i/_search",
            "POST /i/_msearch",
            "GET /i/_doc/_search",
        ] {
            assert!(plan_of(text, 100).is_none(), "{text}");
        }
    }
}
