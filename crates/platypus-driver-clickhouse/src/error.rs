//! ClickHouse errors: `Code: 60. DB::Exception: <message>. (UNKNOWN_TABLE) (version …)`,
//! as the body of a failed response or, once rows were sent, inside the stream.

use platypus_core::{DbError, Error};

/// `QUERY_WAS_CANCELLED`: stopped by `KILL QUERY`.
pub const QUERY_WAS_CANCELLED: u32 = 394;
/// `SESSION_NOT_FOUND`: the HTTP session expired (with `session_check=1`).
pub const SESSION_NOT_FOUND: u32 = 372;
/// `SESSION_IS_LOCKED`: another request of the session is still running.
pub const SESSION_IS_LOCKED: u32 = 373;

/// A server error: its numeric code and message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerError {
    pub code: u32,
    pub message: String,
}

impl ServerError {
    /// The error in `text`, or `None` when it holds no ClickHouse exception. Streamed
    /// exceptions are framed by `__exception__` markers; the first one wins.
    pub fn parse(text: &str) -> Option<Self> {
        let framed = text
            .split_once("__exception__")
            .map_or(text, |(_, rest)| rest);
        let start = framed.find("Code: ")? + "Code: ".len();
        let rest = &framed[start..];
        let digits = rest.find(|c: char| !c.is_ascii_digit())?;
        let code = rest[..digits].parse().ok()?;
        let rest = rest[digits..].trim_start_matches('.').trim_start();
        let rest = rest.strip_prefix("DB::Exception:").unwrap_or(rest).trim();
        // Streamed exceptions end with `<size> <tag>` and the closing marker.
        let rest = rest.split("__exception__").next().unwrap_or(rest);
        let message = match rest.find(" (version ") {
            Some(end) => &rest[..end],
            None => rest.lines().next().unwrap_or(rest),
        };
        Some(Self {
            code,
            message: message.trim().to_owned(),
        })
    }

    /// The error for a statement `sql`: cancellation and expired sessions get their own
    /// variants, syntax errors their position.
    pub fn into_error(self, sql: &str) -> Error {
        match self.code {
            QUERY_WAS_CANCELLED => Error::Cancelled,
            SESSION_NOT_FOUND => Error::Closed,
            code => Error::Database(DbError {
                code: Some(code.to_string()),
                position: position(sql, &self.message),
                message: self.message,
                detail: None,
                hint: None,
            }),
        }
    }
}

/// `failed at position N` counts bytes from 1; editors want characters.
fn position(sql: &str, message: &str) -> Option<u32> {
    let start = message.find("failed at position ")? + "failed at position ".len();
    let rest = &message[start..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let byte = rest[..end].parse::<usize>().ok()?.checked_sub(1)?;
    let prefix = sql.get(..byte.min(sql.len()))?;
    u32::try_from(prefix.chars().count() + 1).ok()
}

/// Text from a failed response that is not a ClickHouse exception (a proxy, a wrong port).
pub fn http_error(status: reqwest::StatusCode, body: &str) -> Error {
    let body = body.trim();
    let body = if body.len() > 300 {
        let mut end = 300;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        &body[..end]
    } else {
        body
    };
    Error::Other(format!("HTTP {status}: {body}"))
}

/// Transport failures: the server is unreachable or the connection broke.
pub fn transport(e: reqwest::Error) -> Error {
    // The request's URL holds the session and query ids and the output settings: noise.
    let e = e.without_url();
    let mut message = e.to_string();
    let mut source = std::error::Error::source(&e);
    while let Some(inner) = source {
        message = format!("{message}: {inner}");
        source = inner.source();
    }
    Error::Connect(platypus_core::without_repeats(&message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_response_errors() {
        let body = "Code: 60. DB::Exception: Unknown table expression identifier 'no_such_table' \
                    in scope SELECT * FROM no_such_table. (UNKNOWN_TABLE) (version 26.3.25.2 \
                    (official build))\n";
        let e = ServerError::parse(body).unwrap();
        assert_eq!(e.code, 60);
        assert!(e.message.starts_with("Unknown table expression"), "{e:?}");
        assert!(e.message.ends_with("(UNKNOWN_TABLE)"), "{e:?}");
        match e.into_error("SELECT * FROM no_such_table") {
            Error::Database(db) => {
                assert_eq!(db.code.as_deref(), Some("60"));
                assert_eq!(db.position, None);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(ServerError::parse("Bad Gateway"), None);
    }

    #[test]
    fn parses_streamed_errors() {
        let text = "\u{1}\u{2}garbage__exception__\r\nabcd\r\nCode: 394. DB::Exception: Query \
                    was cancelled. (QUERY_WAS_CANCELLED) (version 26.3.25.2 (official build))\
                    \r\n120 abcd\r\n__exception__\r\n";
        let e = ServerError::parse(text).unwrap();
        assert_eq!(e.code, QUERY_WAS_CANCELLED);
        assert!(matches!(e.into_error(""), Error::Cancelled));
        let expired = ServerError {
            code: SESSION_NOT_FOUND,
            message: String::new(),
        };
        assert!(matches!(expired.into_error(""), Error::Closed));
    }

    #[test]
    fn locates_syntax_errors_in_characters() {
        let message = "Syntax error: failed at position 10 (FROMM): FROMM t. Expected one of: …";
        assert_eq!(position("SELECT * FROMM t", message), Some(10));
        assert_eq!(
            position("SELECT 'é' FROMM t", "failed at position 13 (x)"),
            Some(12)
        );
        assert_eq!(position("SELECT 1", "Unknown identifier"), None);
    }
}
