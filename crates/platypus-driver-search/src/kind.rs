//! What a statement does, for the checks before it runs (read-only connections and agents).

use serde_json::Value;

use crate::command::{Body, Command, Request};

/// What a statement does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Reads only: what a read-only connection runs.
    Query,
    /// Writes documents.
    Dml,
    /// Creates, changes or deletes indices, mappings, aliases, templates, data streams.
    Ddl,
    /// Anything else that changes the server: cluster settings, snapshots, users, tasks,
    /// maintenance; and what is not known to be a read.
    Other,
}

/// APIs that read, whichever method they are called with (the ones with a body can be
/// `POST`ed).
const SEARCH_APIS: &[&str] = &[
    "_search",
    "_msearch",
    "_count",
    "_mget",
    "_explain",
    "_validate",
    "_field_caps",
    "_search_shards",
    "_termvectors",
    "_mtermvectors",
    "_rank_eval",
    "_terms_enum",
    "_knn_search",
    "_eql",
    "_render",
    "_analyze",
    "_async_search",
    "_pit",
    "_resolve",
];

const WRITE_APIS: &[&str] = &[
    "_doc",
    "_create",
    "_update",
    "_bulk",
    "_update_by_query",
    "_delete_by_query",
    "_reindex",
];

const SCHEMA_APIS: &[&str] = &[
    "_mapping",
    "_mappings",
    "_settings",
    "_alias",
    "_aliases",
    "_template",
    "_index_template",
    "_component_template",
    "_data_stream",
    "_close",
    "_open",
    "_rollover",
    "_shrink",
    "_split",
    "_clone",
    "_ingest",
    "_scripts",
    "_freeze",
    "_unfreeze",
];

impl Command {
    /// What the statement does.
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Self::Sql(sql) => sql_kind(sql),
            Self::Request(request) => request.kind(),
        }
    }
}

impl Request {
    /// What the request does, from its method and the API it calls.
    #[must_use]
    pub fn kind(&self) -> Kind {
        let segments = self.segments();
        let segments: Vec<&str> = segments.iter().map(String::as_str).collect();
        let read = matches!(self.method.as_str(), "GET" | "HEAD");
        // OpenSearch's plugins live under `_plugins/` (`_opendistro/` before 1.0).
        if let [_, plugin, rest @ ..] = segments.as_slice()
            && matches!(segments[0], "_plugins" | "_opendistro")
        {
            return match *plugin {
                "_sql" => sql_endpoint(rest, &self.body),
                "_ppl" => Kind::Query,
                "_security" | "_securityadmin" => Kind::Other,
                _ if read => Kind::Query,
                _ => Kind::Other,
            };
        }
        // `/index/_api/…` or `/_api/…`; no API is `/` or `/index`, which is the index itself.
        let Some(at) = segments.iter().take(2).position(|s| s.starts_with('_')) else {
            return match (self.method.as_str(), segments.len()) {
                ("GET" | "HEAD", _) => Kind::Query,
                ("PUT" | "DELETE", 1) => Kind::Ddl,
                _ => Kind::Other,
            };
        };
        let (api, sub) = (segments[at], segments.get(at + 1).copied());
        if api == "_sql" {
            return sql_endpoint(&segments[at + 1..], &self.body);
        }
        if api == "_security" {
            return Kind::Other;
        }
        if read {
            return Kind::Query;
        }
        if SEARCH_APIS.contains(&api) {
            let closing = self.method == "DELETE"
                && matches!(api, "_pit" | "_async_search" | "_search")
                && (api != "_search" || sub == Some("scroll"));
            return if self.method == "POST" || closing {
                Kind::Query
            } else {
                Kind::Other
            };
        }
        if WRITE_APIS.contains(&api) {
            Kind::Dml
        } else if SCHEMA_APIS.contains(&api) {
            Kind::Ddl
        } else {
            Kind::Other
        }
    }
}

/// A request that changes every document of the indices it names, or every index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unfiltered {
    /// `DELETE BY QUERY`, `UPDATE BY QUERY` or `DELETE INDEX`.
    pub verb: &'static str,
    /// The index expression it names.
    pub index: String,
}

impl Command {
    /// What the statement changes without a filter, for the confirmation before it runs.
    #[must_use]
    pub fn unfiltered(&self) -> Option<Unfiltered> {
        match self {
            Self::Request(request) => request.unfiltered(),
            Self::Sql(_) => None,
        }
    }
}

impl Request {
    /// A `_delete_by_query` or `_update_by_query` with no query (or a `match_all` one), or a
    /// `DELETE` of every index (`_all`, `*`): the counterparts of a `DELETE` without `WHERE`.
    #[must_use]
    pub fn unfiltered(&self) -> Option<Unfiltered> {
        let parts: Vec<&str> = self.path.split('/').filter(|s| !s.is_empty()).collect();
        let (verb, index) = match (self.method.as_str(), parts.as_slice()) {
            ("POST", [index, api]) if api.eq_ignore_ascii_case("_delete_by_query") => {
                ("DELETE BY QUERY", index)
            }
            ("POST", [index, api]) if api.eq_ignore_ascii_case("_update_by_query") => {
                ("UPDATE BY QUERY", index)
            }
            ("DELETE", [index]) if matches!(*index, "_all" | "*") => ("DELETE INDEX", index),
            _ => return None,
        };
        if verb != "DELETE INDEX" && self.filters() {
            return None;
        }
        Some(Unfiltered {
            verb,
            index: (*index).to_owned(),
        })
    }

    /// Whether the request narrows the documents it reaches, by `q` or by a query that is
    /// not `match_all`.
    fn filters(&self) -> bool {
        if self
            .param("q")
            .is_some_and(|q| !matches!(q, "" | "*" | "*:*"))
        {
            return true;
        }
        let Body::Json(text) = &self.body else {
            return false;
        };
        let Ok(body) = serde_json::from_str::<Value>(text) else {
            return false;
        };
        body.get("query").is_some_and(|query| {
            !query
                .as_object()
                .is_some_and(|q| q.is_empty() || (q.len() == 1 && q.contains_key("match_all")))
        })
    }
}

/// The SQL endpoint (`_sql`, `_plugins/_sql`), which `rest` follows: its helpers read, and a
/// statement is what its body says.
fn sql_endpoint(rest: &[&str], body: &Body) -> Kind {
    if matches!(
        rest.first().copied(),
        Some("translate" | "close" | "stats" | "_explain" | "_close")
    ) {
        return Kind::Query;
    }
    let Body::Json(text) = body else {
        return Kind::Other;
    };
    match serde_json::from_str::<Value>(text) {
        // The next page of a cursor of a statement that already ran.
        Ok(v) if v.get("cursor").is_some() => Kind::Query,
        Ok(v) => v
            .get("query")
            .and_then(Value::as_str)
            .map_or(Kind::Other, sql_kind),
        Err(_) => Kind::Other,
    }
}

/// What an SQL statement does: `SELECT`, `SHOW`, `DESCRIBE` and `EXPLAIN` read; anything else
/// is not known to.
#[must_use]
pub fn sql_kind(sql: &str) -> Kind {
    let mut rest = sql.trim_start();
    loop {
        if let Some(r) = rest.strip_prefix("--") {
            rest = r
                .split_once('\n')
                .map_or("", |(_, after)| after)
                .trim_start();
        } else if let Some(r) = rest.strip_prefix("/*") {
            rest = r
                .split_once("*/")
                .map_or("", |(_, after)| after)
                .trim_start();
        } else if let Some(r) = rest.strip_prefix('(') {
            rest = r.trim_start();
        } else {
            break;
        }
    }
    let word: String = rest
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .collect::<String>()
        .to_ascii_uppercase();
    match word.as_str() {
        "SELECT" | "WITH" | "SHOW" | "DESCRIBE" | "DESC" | "EXPLAIN" => Kind::Query,
        _ => Kind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::parse;

    fn kind(text: &str) -> Kind {
        parse(text).unwrap().kind()
    }

    fn unfiltered(text: &str) -> Option<(&'static str, String)> {
        parse(text).unwrap().unfiltered().map(|u| (u.verb, u.index))
    }

    #[test]
    fn changes_without_a_filter_are_found() {
        let every = |verb, index: &str| Some((verb, index.to_owned()));
        assert_eq!(
            unfiltered("POST /orders/_delete_by_query"),
            every("DELETE BY QUERY", "orders")
        );
        assert_eq!(
            unfiltered(
                "POST /orders/_delete_by_query
{\"query\":{\"match_all\":{}}}"
            ),
            every("DELETE BY QUERY", "orders")
        );
        assert_eq!(
            unfiltered(
                "POST /o*/_update_by_query?q=*
{\"script\":\"x\"}"
            ),
            every("UPDATE BY QUERY", "o*")
        );
        assert_eq!(unfiltered("DELETE /_all"), every("DELETE INDEX", "_all"));
        assert_eq!(unfiltered("DELETE /*"), every("DELETE INDEX", "*"));
        for filtered in [
            "POST /o/_delete_by_query
{\"query\":{\"term\":{\"a\":1}}}",
            "POST /o/_delete_by_query?q=a:1",
            "POST /o/_update_by_query
{\"query\":{\"match_none\":{}}}",
            "DELETE /orders",
            "DELETE /orders/_doc/1",
            "POST /orders/_search",
            "SELECT 1",
        ] {
            assert_eq!(unfiltered(filtered), None, "{filtered}");
        }
    }

    #[test]
    fn reads_are_queries() {
        for text in [
            "GET /",
            "GET /idx",
            "HEAD /idx",
            "GET /idx/_search",
            "POST /idx/_search\n{\"query\":{\"match_all\":{}}}",
            "GET /_search",
            "POST /_msearch\n{}\n{\"query\":{\"match_all\":{}}}",
            "POST /idx/_count",
            "POST /idx/_mget\n{\"ids\":[\"1\"]}",
            "POST /idx/_explain/1\n{\"query\":{\"match_all\":{}}}",
            "POST /idx/_validate/query\n{\"query\":{\"match_all\":{}}}",
            "POST /idx/_field_caps?fields=*",
            "POST /idx/_search/template\n{\"id\":\"t\"}",
            "POST /idx/_pit?keep_alive=1m",
            "DELETE /_pit\n{\"id\":\"x\"}",
            "POST /_search/scroll\n{\"scroll_id\":\"x\"}",
            "DELETE /_search/scroll\n{\"scroll_id\":\"x\"}",
            "GET /idx/_doc/1",
            "GET /idx/_source/1",
            "GET /_cat/indices",
            "GET /_cluster/health",
            "GET /_nodes/stats",
            "GET /idx/_mapping",
            "GET /idx/_settings",
            "GET /_alias",
            "GET /_tasks",
            "POST /_analyze\n{\"text\":\"a\"}",
            "POST /_sql\n{\"query\":\"SELECT 1\"}",
            "POST /_sql?format=json\n{\"query\":\"  show tables\"}",
            "POST /_sql\n{\"query\":\"DESCRIBE t\"}",
            "POST /_sql\n{\"cursor\":\"abc\"}",
            "POST /_sql/translate\n{\"query\":\"DROP TABLE t\"}",
            "POST /_sql/close\n{\"cursor\":\"abc\"}",
            "POST /_plugins/_sql\n{\"query\":\"SELECT * FROM t\"}",
            "POST /_plugins/_ppl\n{\"query\":\"source=t\"}",
            "GET /_plugins/_ism/explain",
            "SELECT * FROM t",
            "  (SELECT 1)",
            "/* c */ SHOW TABLES",
            "DESCRIBE t",
            "WITH a AS (SELECT 1) SELECT * FROM a",
        ] {
            assert_eq!(kind(text), Kind::Query, "{text}");
        }
    }

    #[test]
    fn document_writes_are_dml() {
        for text in [
            "PUT /idx/_doc/1\n{\"a\":1}",
            "POST /idx/_doc\n{\"a\":1}",
            "PUT /idx/_create/1\n{\"a\":1}",
            "POST /idx/_update/1\n{\"doc\":{\"a\":2}}",
            "DELETE /idx/_doc/1",
            "POST /_bulk\n{\"index\":{}}\n{}",
            "POST /idx/_bulk\n{\"index\":{}}\n{}",
            "POST /idx/_update_by_query",
            "POST /idx/_delete_by_query\n{\"query\":{\"match_all\":{}}}",
            "POST /_reindex\n{\"source\":{\"index\":\"a\"},\"dest\":{\"index\":\"b\"}}",
        ] {
            assert_eq!(kind(text), Kind::Dml, "{text}");
        }
    }

    #[test]
    fn schema_changes_are_ddl() {
        for text in [
            "PUT /idx",
            "DELETE /idx",
            "DELETE /idx1,idx2",
            "DELETE /*",
            "PUT /idx/_mapping\n{\"properties\":{}}",
            "PUT /idx/_settings\n{\"number_of_replicas\":0}",
            "POST /_aliases\n{\"actions\":[]}",
            "PUT /idx/_alias/a",
            "PUT /_index_template/t\n{}",
            "DELETE /_template/t",
            "PUT /_data_stream/logs",
            "POST /idx/_close",
            "POST /idx/_rollover",
            "PUT /_ingest/pipeline/p\n{}",
            "PUT /_component_template/c\n{}",
        ] {
            assert_eq!(kind(text), Kind::Ddl, "{text}");
        }
    }

    #[test]
    fn the_rest_changes_the_server_or_is_unknown() {
        for text in [
            "PUT /_cluster/settings\n{}",
            "POST /_cluster/reroute\n{}",
            "PUT /_snapshot/repo\n{}",
            "POST /_snapshot/repo/s/_restore",
            "GET /_security/user",
            "PUT /_security/user/u\n{}",
            "POST /_nodes/_all/reload_secure_settings",
            "POST /_cache/clear",
            "POST /idx/_cache/clear",
            "POST /idx/_refresh",
            "POST /idx/_forcemerge",
            "POST /_tasks/abc/_cancel",
            "POST /_sql\n{\"query\":\"DROP TABLE t\"}",
            "POST /_sql",
            "POST /_plugins/_sql\n{\"query\":\"DELETE FROM t\"}",
            "PUT /_plugins/_ism/policies/p\n{}",
            "POST /_plugins/_security/api/internalusers/u",
            "PUT /_ilm/policy/p\n{}",
            "POST /",
            "PUT /idx/_search",
            "UPDATE t SET a = 1",
            "DELETE FROM t",
            "INSERT INTO t VALUES (1)",
            "DROP TABLE t",
        ] {
            assert_eq!(kind(text), Kind::Other, "{text}");
        }
    }
}
