//! The URIs of the MCP resources: `platypus://<connection>[/<schema>[/<table>]]`, each
//! segment percent-encoded so that names with `/`, spaces or non-ASCII letters round-trip.

use std::fmt::Write;

const SCHEME: &str = "platypus://";

/// What a resource URI points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Connection(String),
    Schema(String, String),
    Table(String, String, String),
}

/// The URI forms a client can read, for error messages.
pub const FORMS: &str = "platypus://<connection>, platypus://<connection>/<schema> or platypus://<connection>/<schema>/<table>";

/// Everything but the unreserved characters is escaped, `/` included.
fn encode(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for &b in segment.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            // Writing to a String cannot fail.
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

fn decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = segment.get(i + 1..i + 3)?;
            // `from_str_radix` accepts a sign: a bare hex check keeps `%+1` out.
            if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[must_use]
pub fn connection_uri(connection: &str) -> String {
    format!("{SCHEME}{}", encode(connection))
}

#[must_use]
pub fn schema_uri(connection: &str, schema: &str) -> String {
    format!("{}/{}", connection_uri(connection), encode(schema))
}

#[must_use]
pub fn table_uri(connection: &str, schema: &str, table: &str) -> String {
    format!("{}/{}", schema_uri(connection, schema), encode(table))
}

/// The target of `uri`, or `None` for another scheme, an empty segment, more than three
/// segments or a bad escape.
#[must_use]
pub fn parse(uri: &str) -> Option<Target> {
    let rest = uri.strip_prefix(SCHEME)?;
    let parts = rest
        .split('/')
        .map(|s| if s.is_empty() { None } else { decode(s) })
        .collect::<Option<Vec<_>>>()?;
    let mut parts = parts.into_iter();
    match (parts.next()?, parts.next(), parts.next(), parts.next()) {
        (c, None, ..) => Some(Target::Connection(c)),
        (c, Some(s), None, _) => Some(Target::Schema(c, s)),
        (c, Some(s), Some(t), None) => Some(Target::Table(c, s, t)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_awkward_names() {
        let (c, s, t) = ("App db", "my/schema", "tab%le é/日本");
        let uri = table_uri(c, s, t);
        assert_eq!(uri.matches('/').count(), 4, "{uri}");
        assert!(uri.is_ascii(), "{uri}");
        assert_eq!(
            parse(&uri),
            Some(Target::Table(c.into(), s.into(), t.into()))
        );
        assert_eq!(
            parse(&schema_uri(c, s)),
            Some(Target::Schema(c.into(), s.into()))
        );
        assert_eq!(
            parse(&connection_uri(c)),
            Some(Target::Connection(c.into()))
        );
        assert_eq!(connection_uri("App db"), "platypus://App%20db");
    }

    #[test]
    fn rejects_what_is_not_a_resource() {
        for bad in [
            "",
            "platypus://",
            "http://a/b",
            "platypus:/a",
            "platypus://a//t",
            "platypus://a/",
            "platypus:///s",
            "platypus://a/b/c/d",
            "platypus://a%",
            "platypus://a%2",
            "platypus://a%zz",
            "platypus://a%+1",
            "platypus://a%FF",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }
}
