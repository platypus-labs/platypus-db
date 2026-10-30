//! The target of a request: its path and query string, in the one form that both the checks
//! before it runs ([`crate::kind`]) and the HTTP call ([`crate::http`]) read.
//!
//! A target is a path on the server, never a URL: one with a scheme or a host (`http://…`,
//! `//host/…`, a `\`) is refused, and so is a path with an empty, `.` or `..` segment, however
//! it is written (`%2e`, `.%2E`…). What is left cannot be resolved into another path by the
//! HTTP layer, so the API the checks see is the one the server gets. Each segment is written
//! with the same percent-encoding whatever the request wrote (`%5Fbulk` is `_bulk`), and the
//! checks read the decoded segments and parameters, as the server does.

use std::fmt::{self, Write as _};

/// The path and query string of a request, checked and percent-encoded the one way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Absolute: `/` or `/<segment>/…`, without a trailing slash.
    path: String,
    /// Without the `?`; empty when there is none.
    query: String,
}

impl Target {
    /// Reads a target as a request line writes it: `/index/_search?size=5` (the leading slash
    /// is optional, a trailing one is dropped).
    pub fn parse(raw: &str) -> Result<Self, String> {
        let (path, query) = raw.split_once('?').unwrap_or((raw, ""));
        if path.contains('\\') {
            return Err(format!(
                "invalid request path `{path}`: a path cannot contain `\\`"
            ));
        }
        if path.starts_with("//") || has_scheme(path) {
            return Err(format!(
                "invalid request path `{path}`: a request takes a path on the server, not a URL"
            ));
        }
        let rest = path.strip_prefix('/').unwrap_or(path);
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        let mut canonical = String::with_capacity(path.len() + 1);
        if rest.is_empty() {
            canonical.push('/');
        } else {
            for segment in rest.split('/') {
                let decoded = decode(segment, false)
                    .map_err(|e| format!("invalid request path `{path}`: {e}"))?;
                let problem = match decoded.as_str() {
                    "" => "a path cannot have an empty segment",
                    "." | ".." => "a path cannot have a `.` or `..` segment",
                    _ => {
                        canonical.push('/');
                        encode_segment(&decoded, &mut canonical);
                        continue;
                    }
                };
                return Err(format!("invalid request path `{path}`: {problem}"));
            }
        }
        let mut target = Self {
            path: canonical,
            query: String::new(),
        };
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            decode(key, true)
                .and_then(|_| decode(value, true))
                .map_err(|e| format!("invalid query string `{query}`: {e}"))?;
            target.push_pair(pair);
        }
        Ok(target)
    }

    /// [`parse`](Self::parse), for a target the driver writes itself (from the names of
    /// indices, say), whose error is the driver's.
    pub fn new(raw: &str) -> platypus_core::Result<Self> {
        Self::parse(raw).map_err(platypus_core::Error::Other)
    }

    /// The path, percent-encoded: what the URL of the request has.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The query string, without the `?`.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The segments of the path, decoded.
    #[must_use]
    pub fn segments(&self) -> Vec<String> {
        self.path
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| decode(s, false).unwrap_or_default())
            .collect()
    }

    /// The query parameters, decoded (a `+` is a space), in order.
    pub fn params(&self) -> impl Iterator<Item = (String, String)> + '_ {
        self.query.split('&').filter(|p| !p.is_empty()).map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (
                decode(key, true).unwrap_or_default(),
                decode(value, true).unwrap_or_default(),
            )
        })
    }

    /// The value of the query parameter `name`, decoded: the last one when it is repeated,
    /// as the server reads it.
    #[must_use]
    pub fn param(&self, name: &str) -> Option<String> {
        self.params()
            .filter(|(key, _)| key == name)
            .map(|(_, value)| value)
            .last()
    }

    /// Adds `name=value` to the query string; neither needs encoding.
    pub fn push_param(&mut self, name: &str, value: &str) {
        self.push_pair(&format!("{name}={value}"));
    }

    fn push_pair(&mut self, pair: &str) {
        if !self.query.is_empty() {
            self.query.push('&');
        }
        encode_query(pair, &mut self.query);
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.path)?;
        if !self.query.is_empty() {
            write!(f, "?{}", self.query)?;
        }
        Ok(())
    }
}

/// Whether `path` starts with a URL scheme and an authority: `http://`, `HTTPS://`, `x+y://`.
fn has_scheme(path: &str) -> bool {
    let Some((scheme, _)) = path.split_once("://") else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// `text` with its `%XX` escapes decoded, and `+` as a space in a query string. A `%` that
/// does not start an escape, or bytes that are not UTF-8, are an error.
fn decode(text: &str, query: bool) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = bytes
                    .get(i + 1..i + 3)
                    .and_then(|h| std::str::from_utf8(h).ok())
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
                    .ok_or_else(|| format!("`{text}` has a `%` that is not an escape"))?;
                out.push(hex);
                i += 3;
            }
            b'+' if query => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| format!("`{text}` does not decode to UTF-8 text"))
}

/// Characters a path segment keeps as they are: the unreserved ones and the sub-delimiters,
/// which no HTTP layer changes. Anything else is escaped, `/`, `%`, `?` and `#` included.
fn plain_in_segment(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
        )
}

fn encode_segment(decoded: &str, out: &mut String) {
    for b in decoded.bytes() {
        if plain_in_segment(b) {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
}

/// Copies a query pair, escaping what the URL of the request would escape (controls, spaces,
/// quotes, `#`, `<`, `>`, anything not ASCII) so that it is sent as written here.
fn encode_query(pair: &str, out: &mut String) {
    for b in pair.bytes() {
        if b.is_ascii_graphic() && !matches!(b, b'"' | b'#' | b'\'' | b'<' | b'>' | b'`') {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(raw: &str) -> String {
        Target::parse(raw).unwrap().path().to_owned()
    }

    #[test]
    fn writes_paths_the_one_way() {
        assert_eq!(path("/"), "/");
        assert_eq!(path(""), "/");
        assert_eq!(path("_cat/indices"), "/_cat/indices");
        assert_eq!(path("/idx/_search/"), "/idx/_search");
        assert_eq!(path("/%5Fbulk"), "/_bulk");
        assert_eq!(path("/a%2Fb/_doc/x%20y"), "/a%2Fb/_doc/x%20y");
        assert_eq!(path("/a,b*/_search"), "/a,b*/_search");
        assert_eq!(path("/remote:idx/_search"), "/remote:idx/_search");
        assert_eq!(
            path("/%3Clogs-%7Bnow%2Fd%7D%3E"),
            "/%3Clogs-%7Bnow%2Fd%7D%3E"
        );
        assert_eq!(path("/caf\u{e9}"), "/caf%C3%A9");
        assert_eq!(path("/.kibana/_search"), "/.kibana/_search");
        assert_eq!(path("/.../_doc/..x"), "/.../_doc/..x");
        // Written the one way, a target reads back as itself.
        for raw in ["/a%2Fb/_doc/x%20y?q=a+b&x", "/%5fsearch?q=%2A"] {
            let once = Target::parse(raw).unwrap();
            assert_eq!(Target::parse(&once.to_string()).unwrap(), once);
        }
    }

    #[test]
    fn refuses_dot_and_empty_segments_in_any_encoding() {
        for raw in [
            "/idx/_search/../_bulk",
            "/idx/_search/./x",
            "..",
            "/idx/%2e%2e/_bulk",
            "/idx/%2E%2e/_bulk",
            "/idx/.%2E/_bulk",
            "/idx/%2e./_bulk",
            "/idx/%2e",
            "/idx//_bulk",
            "//idx",
        ] {
            let e = Target::parse(raw).unwrap_err();
            assert!(e.starts_with("invalid request path"), "{raw}: {e}");
        }
    }

    #[test]
    fn refuses_urls() {
        for raw in [
            "http://attacker.example/x",
            "HTTPS://attacker.example/x",
            "//attacker.example/x",
            "/\\attacker.example/x",
            "\\\\attacker.example\\x",
            "/idx\\..\\_bulk",
        ] {
            assert!(Target::parse(raw).is_err(), "{raw}");
        }
        assert!(
            Target::parse("http://attacker.example/x")
                .unwrap_err()
                .contains("not a URL")
        );
    }

    #[test]
    fn refuses_broken_escapes() {
        assert!(Target::parse("/idx%zz").is_err());
        assert!(Target::parse("/idx%2").is_err());
        assert!(Target::parse("/idx%FF").is_err());
        assert!(Target::parse("/idx?q=%").is_err());
    }

    #[test]
    fn reads_decoded_parameters() {
        let t = Target::parse("/i/_delete_by_query?q=%2A&%71=*%3A*&size=2&pretty").unwrap();
        assert_eq!(t.param("q").as_deref(), Some("*:*"));
        assert_eq!(t.param("pretty").as_deref(), Some(""));
        assert_eq!(t.param("from"), None);
        assert_eq!(
            Target::parse("/i?q=a+b").unwrap().param("q").as_deref(),
            Some("a b")
        );
        let mut t = Target::parse("/_cat/indices").unwrap();
        t.push_param("format", "json");
        assert_eq!(t.to_string(), "/_cat/indices?format=json");
        assert_eq!(
            Target::parse("/i?q=\"a b\"#x").unwrap().query(),
            "q=%22a%20b%22%23x"
        );
    }
}
