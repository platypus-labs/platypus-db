//! The editor language: requests in the form of Kibana's Dev Tools console, and SQL.
//!
//! ```text
//! GET /orders/_search?size=5
//! { "query": { "match": { "status": "open" } } }
//!
//! POST /_bulk
//! { "index": { "_index": "orders", "_id": "1" } }
//! { "total": 12 }
//!
//! SELECT status, COUNT(*) FROM orders GROUP BY status;
//! ```
//!
//! A request is a line `METHOD /path?query` (`GET`, `POST`, `PUT`, `DELETE`, `HEAD`; the
//! slash is optional) and its body, the JSON that follows. A body of one value ends where
//! its braces close; the NDJSON bodies of `_bulk` and `_msearch` end at a blank line or the
//! next request. Anything else is SQL, which ends at a `;` or a blank line. Lines that
//! start with `#` or `//` (and `--` between statements) are comments.

use std::ops::Range;

use serde::de::IgnoredAny;

use crate::http::Payload;
pub use crate::kind::{Kind, Unfiltered, sql_kind};

/// The body of a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    None,
    /// One JSON value, as written.
    Json(String),
    /// A JSON value per line, ending with a newline.
    Ndjson(String),
}

impl Body {
    /// The body as the HTTP layer sends it.
    #[must_use]
    pub fn payload(&self) -> Payload<'_> {
        match self {
            Self::None => Payload::None,
            Self::Json(text) => Payload::Json(text),
            Self::Ndjson(text) => Payload::Ndjson(text),
        }
    }
}

/// A request to the REST API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// `GET`, `POST`, `PUT`, `DELETE` or `HEAD`.
    pub method: String,
    /// The path, with a leading slash and without the query string.
    pub path: String,
    /// The query string, without the `?`.
    pub query: String,
    pub body: Body,
}

impl Request {
    /// The path and query string, as the request line writes them.
    #[must_use]
    pub fn target(&self) -> String {
        if self.query.is_empty() {
            self.path.clone()
        } else {
            format!("{}?{}", self.path, self.query)
        }
    }

    /// The path's segments, in lower case.
    #[must_use]
    pub fn segments(&self) -> Vec<String> {
        segments(&self.path)
    }

    /// The value of the query parameter `name`, if the request has it.
    #[must_use]
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (key == name).then_some(value)
        })
    }
}

/// A statement of the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Request(Request),
    /// SQL, without its final `;`.
    Sql(String),
}

impl Command {
    /// What the statement is, for messages: `POST /orders/_search`, or `SQL`.
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            Self::Request(r) => format!("{} {}", r.method, r.path),
            Self::Sql(_) => "SQL".to_owned(),
        }
    }
}

const METHODS: [&str; 5] = ["GET", "POST", "PUT", "DELETE", "HEAD"];

fn segments(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

/// `(method, target, rest of the line)` when `line` is a request line.
fn method_line(line: &str) -> Option<(&'static str, &str, &str)> {
    let (word, rest) = line.trim_start().split_once(char::is_whitespace)?;
    let method = METHODS.iter().find(|m| m.eq_ignore_ascii_case(word))?;
    let rest = rest.trim_start();
    let (target, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    // `DELETE FROM` is SQL.
    if target.is_empty() || (*method == "DELETE" && target.eq_ignore_ascii_case("from")) {
        return None;
    }
    Some((method, target, after))
}

fn is_comment(line: &str) -> bool {
    line.starts_with('#') || line.starts_with("//") || line.starts_with("--")
}

/// Whether the body of a request to `target` is NDJSON.
fn ndjson_target(target: &str) -> bool {
    let path = target.split('?').next().unwrap_or(target);
    segments(path)
        .iter()
        .any(|s| matches!(s.as_str(), "_bulk" | "_msearch" | "_bulk_stream"))
}

/// The line at `pos`, without its line break, and where the next one starts.
fn line_at(src: &str, pos: usize) -> (&str, usize) {
    match src[pos..].find('\n') {
        Some(i) => (src[pos..pos + i].trim_end_matches('\r'), pos + i + 1),
        None => (&src[pos..], src.len()),
    }
}

/// Follows the nesting of JSON text, line by line.
#[derive(Default)]
struct Depth {
    level: i32,
    seen: bool,
    in_string: bool,
    escaped: bool,
}

impl Depth {
    fn feed(&mut self, text: &str) {
        for c in text.chars() {
            if self.in_string {
                if self.escaped {
                    self.escaped = false;
                } else if c == '\\' {
                    self.escaped = true;
                } else if c == '"' {
                    self.in_string = false;
                }
            } else {
                match c {
                    '"' => self.in_string = true,
                    '{' | '[' => {
                        self.level += 1;
                        self.seen = true;
                    }
                    '}' | ']' => self.level -= 1,
                    _ => {}
                }
            }
        }
        self.in_string = false;
        self.escaped = false;
    }

    const fn complete(&self) -> bool {
        self.seen && self.level <= 0
    }
}

/// Where the request that starts at `first` (a line start) ends.
fn request_end(src: &str, first: usize) -> usize {
    let (line, mut next) = line_at(src, first);
    let (_, target, after) = method_line(line).unwrap_or(("GET", "", ""));
    let ndjson = ndjson_target(target);
    let mut depth = Depth::default();
    depth.feed(after);
    let mut end = first + line.trim_end().len();
    while (ndjson || !depth.complete()) && next < src.len() {
        let (line, after_line) = line_at(src, next);
        let text = line.trim();
        if text.is_empty() || method_line(text).is_some() {
            break;
        }
        // Not a body: the request has none, and this is what comes next.
        if !ndjson && !depth.seen && !is_comment(text) && !text.starts_with(['{', '[']) {
            break;
        }
        if !is_comment(text) {
            depth.feed(text);
            end = next + line.trim_end().len();
        }
        next = after_line;
    }
    end
}

#[derive(Clone, Copy, PartialEq)]
enum Lex {
    Code,
    Quote(char),
    Block,
}

/// Where the SQL statement that starts at `first` ends: after its `;`, or at a blank line
/// or a request line.
fn sql_end(src: &str, first: usize) -> usize {
    let mut pos = first;
    let mut end = first;
    let mut lex = Lex::Code;
    let mut first_line = true;
    loop {
        let (line, next) = line_at(src, pos);
        let text = line.trim();
        if !first_line && lex == Lex::Code && (text.is_empty() || method_line(text).is_some()) {
            return end;
        }
        let mut chars = line.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            let ahead = chars.peek().map(|&(_, c)| c);
            match lex {
                Lex::Code => match c {
                    ';' => return pos + i + 1,
                    '\'' | '"' | '`' => lex = Lex::Quote(c),
                    '-' if ahead == Some('-') => break,
                    '/' if ahead == Some('*') => {
                        chars.next();
                        lex = Lex::Block;
                    }
                    _ => {}
                },
                Lex::Quote(q) => {
                    if c == q {
                        lex = Lex::Code;
                    }
                }
                Lex::Block => {
                    if c == '*' && ahead == Some('/') {
                        chars.next();
                        lex = Lex::Code;
                    }
                }
            }
        }
        if !text.is_empty() {
            end = pos + line.trim_end().len();
        }
        first_line = false;
        if next >= src.len() {
            return end;
        }
        pos = next;
    }
}

/// The statements of `src`, trimmed.
#[must_use]
pub fn split(src: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < src.len() {
        let (line, next) = line_at(src, pos);
        let text = line.trim();
        if text.is_empty() || is_comment(text) {
            pos = next;
            continue;
        }
        let start = pos + line.len() - line.trim_start().len();
        let end = if method_line(text).is_some() {
            request_end(src, pos)
        } else {
            sql_end(src, pos)
        };
        let end = end.max(start + 1);
        out.push(start..end);
        // What follows on the last line is nothing, or the next statement after a `;`.
        pos = end;
    }
    out
}

/// The statement on the line of byte offset `at`, or else the nearest one above it.
#[must_use]
pub fn statement_at(src: &str, at: usize) -> Option<Range<usize>> {
    let at = at.min(src.len());
    let line_start = src[..at].rfind('\n').map_or(0, |i| i + 1);
    split(src)
        .into_iter()
        .take_while(|r| r.start <= at.max(line_start))
        .last()
}

/// The statement that opens an index in the data viewer: all its documents.
#[must_use]
pub fn browse(index: &str) -> String {
    browse_limited(index, None)
}

/// [`browse`] that reads `limit` documents at most.
#[must_use]
pub fn browse_limited(index: &str, limit: Option<u64>) -> String {
    let size = limit.map(|n| format!("\"size\":{n},")).unwrap_or_default();
    format!(
        "GET /{}/_search
{{{size}\"query\":{{\"match_all\":{{}}}}}}",
        crate::meta::segment(index)
    )
}

/// The index a statement reads whole, when it is a plain search of one index (what
/// [`browse`] writes), so that the data viewer can load its columns from the mapping.
#[must_use]
pub fn source_index(text: &str) -> Option<String> {
    let Ok(Command::Request(request)) = parse(text) else {
        return None;
    };
    let parts: Vec<&str> = request.path.split('/').filter(|s| !s.is_empty()).collect();
    match parts.as_slice() {
        [index, "_search"] if !index.starts_with('_') && !index.contains([',', '*']) => {
            Some((*index).to_owned())
        }
        _ => None,
    }
}

/// Whether every statement of `src` parses and only reads.
#[must_use]
pub fn only_reads(src: &str) -> bool {
    let statements = split(src);
    !statements.is_empty()
        && statements
            .into_iter()
            .all(|r| parse(&src[r]).is_ok_and(|c| c.kind() == Kind::Query))
}

/// Reads one statement, as [`split`] cuts it out.
pub fn parse(text: &str) -> Result<Command, String> {
    let lines: Vec<&str> = text.lines().collect();
    let first = lines
        .iter()
        .position(|l| {
            let t = l.trim();
            !t.is_empty() && !is_comment(t)
        })
        .ok_or("no statement")?;
    if let Some((method, target, after)) = method_line(lines[first]) {
        return request(method, target, after, &lines[first + 1..]);
    }
    let sql = lines[first..].join("\n");
    let sql = sql.trim().trim_end_matches([';', ' ', '\t', '\n', '\r']);
    if sql.is_empty() {
        Err("no statement".to_owned())
    } else {
        Ok(Command::Sql(sql.to_owned()))
    }
}

fn request(method: &str, target: &str, after: &str, rest: &[&str]) -> Result<Command, String> {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let path = if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    };
    let body_lines: Vec<&str> = std::iter::once(after)
        .chain(rest.iter().copied())
        .map(str::trim)
        .filter(|l| !l.is_empty() && !is_comment(l))
        .collect();
    let body = if body_lines.is_empty() {
        Body::None
    } else if ndjson_target(&path) {
        for (n, line) in body_lines.iter().enumerate() {
            valid_json(line).map_err(|e| format!("invalid JSON in body line {}: {e}", n + 1))?;
        }
        Body::Ndjson(body_lines.join("\n") + "\n")
    } else {
        let text = body_lines.join("\n");
        valid_json(&text).map_err(|e| format!("invalid JSON body: {e}"))?;
        Body::Json(text)
    };
    Ok(Command::Request(Request {
        method: method.to_owned(),
        path,
        query: query.to_owned(),
        body,
    }))
}

fn valid_json(text: &str) -> Result<(), serde_json::Error> {
    serde_json::from_str::<IgnoredAny>(text).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn statements(src: &str) -> Vec<&str> {
        split(src).into_iter().map(|r| &src[r]).collect()
    }

    fn request_of(text: &str) -> Request {
        match parse(text).unwrap() {
            Command::Request(r) => r,
            other @ Command::Sql(_) => panic!("{other:?}"),
        }
    }

    #[test]
    fn splits_requests_and_sql() {
        let src = "# find orders\nGET /orders/_search?size=5\n{\n  \"query\": {\"match_all\": {}}\n}\nGET _cat/indices\n\nSELECT a,\n  b FROM t\n-- next\nSELECT 2; SELECT 3;\n\n\nPOST /_bulk\n{\"index\":{\"_index\":\"i\"}}\n{\"a\":1}\n\nDELETE /i\n";
        assert_eq!(
            statements(src),
            [
                "GET /orders/_search?size=5\n{\n  \"query\": {\"match_all\": {}}\n}",
                "GET _cat/indices",
                "SELECT a,\n  b FROM t\n-- next\nSELECT 2;",
                "SELECT 3;",
                "POST /_bulk\n{\"index\":{\"_index\":\"i\"}}\n{\"a\":1}",
                "DELETE /i",
            ]
        );
    }

    #[test]
    fn a_body_ends_where_its_braces_close() {
        let src = "GET /i/_search\n{\"a\": \"}\"}\nSELECT 1";
        assert_eq!(
            statements(src),
            ["GET /i/_search\n{\"a\": \"}\"}", "SELECT 1"]
        );
        // A method line ends SQL without a blank line; DELETE FROM is SQL.
        let src = "SELECT 1\nGET /x\nDELETE FROM t";
        assert_eq!(statements(src), ["SELECT 1", "GET /x", "DELETE FROM t"]);
    }

    #[test]
    fn semicolons_in_strings_and_comments_do_not_end_sql() {
        let src = "SELECT ';' AS a, \"b;c\" /* ; */ FROM t -- ;\nWHERE x = 1;\nSELECT 2";
        assert_eq!(
            statements(src),
            [
                "SELECT ';' AS a, \"b;c\" /* ; */ FROM t -- ;\nWHERE x = 1;",
                "SELECT 2"
            ]
        );
        let src = "SELECT 'a\n\nb';\n";
        assert_eq!(statements(src), ["SELECT 'a\n\nb';"]);
    }

    #[test]
    fn finds_the_statement_at_an_offset() {
        let src = "GET /a\n\nGET /b\n{\"x\":1}\n\nSELECT 1";
        let at = |offset| statement_at(src, offset).map(|r| &src[r]);
        assert_eq!(at(0), Some("GET /a"));
        assert_eq!(at(7), Some("GET /a"));
        assert_eq!(at(16), Some("GET /b\n{\"x\":1}"));
        assert_eq!(at(src.len()), Some("SELECT 1"));
        assert_eq!(statement_at("# nothing", 3), None);
    }

    #[test]
    fn parses_requests() {
        let r =
            request_of("get orders/_search?size=2&pretty\n{ \"query\":\n {\"match_all\": {}} }");
        assert_eq!(r.method, "GET");
        assert_eq!(r.path, "/orders/_search");
        assert_eq!(r.query, "size=2&pretty");
        assert_eq!(r.param("size"), Some("2"));
        assert_eq!(r.param("from"), None);
        assert_eq!(r.target(), "/orders/_search?size=2&pretty");
        assert!(matches!(r.body, Body::Json(b) if b.starts_with("{ \"query\":")));
        let r = request_of("PUT /idx");
        assert_eq!((r.path.as_str(), &r.body), ("/idx", &Body::None));
        let r = request_of("POST /_bulk\n{\"index\":{}}\n# note\n{\"a\":1}");
        assert_eq!(
            r.body,
            Body::Ndjson("{\"index\":{}}\n{\"a\":1}\n".to_owned())
        );
        let r = request_of("POST /i/_doc {\"a\": 1}");
        assert_eq!(r.body, Body::Json("{\"a\": 1}".to_owned()));
        let r = request_of("// c\nHEAD /i");
        assert_eq!(r.method, "HEAD");
    }

    #[test]
    fn writes_and_reads_the_browse_statement() {
        let text = browse("orders");
        assert_eq!(
            text,
            "GET /orders/_search
{\"query\":{\"match_all\":{}}}"
        );
        assert_eq!(source_index(&text).as_deref(), Some("orders"));
        assert_eq!(source_index("GET /a,b/_search"), None);
        assert_eq!(source_index("GET /_search"), None);
        assert_eq!(source_index("SELECT 1"), None);
    }

    #[test]
    fn parses_sql() {
        assert_eq!(
            parse("-- count\nSELECT COUNT(*) FROM t;\n").unwrap(),
            Command::Sql("SELECT COUNT(*) FROM t".to_owned())
        );
        assert_eq!(
            parse("DELETE FROM t").unwrap(),
            Command::Sql("DELETE FROM t".to_owned())
        );
        assert!(parse("  ;").is_err());
        assert!(parse("# only a comment").is_err());
    }

    #[test]
    fn refuses_bad_bodies() {
        let e = parse("POST /i/_doc\n{\"a\": }").unwrap_err();
        assert!(e.starts_with("invalid JSON body"), "{e}");
        let e = parse("POST /_bulk\n{\"index\":{}}\n{oops}").unwrap_err();
        assert!(e.contains("body line 2"), "{e}");
        assert!(parse("GET /i/_search\n{\"a\":1}\n{\"b\":2}").is_err());
    }
}
