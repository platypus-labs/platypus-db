//! Server errors: `{"error": {"type": …, "reason": …, "root_cause": […]}, "status": 400}`
//! (`OpenSearch`'s SQL plugin adds `details`), and transport failures.

use platypus_core::{DbError, Error};
use serde_json::Value;

/// The error type the servers answer a cancelled task with.
const CANCELLED: &str = "task_cancelled_exception";

/// The error for an unsuccessful reply. `sql` is the statement it answers, when there is
/// one: parsing errors name a line and column in it.
pub fn from_reply(status: u16, body: &str, sql: Option<&str>) -> Error {
    let Ok(Value::Object(root)) = serde_json::from_str::<Value>(body) else {
        return http_error(status, body);
    };
    let Some(error) = root.get("error") else {
        return http_error(status, body);
    };
    let text = |v: Option<&Value>, key: &str| {
        v.and_then(|v| v.get(key))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let (code, mut message, mut detail) = match error {
        Value::String(reason) => (None, reason.clone(), None),
        error => (
            text(Some(error), "type"),
            text(Some(error), "reason").unwrap_or_else(|| error.to_string()),
            text(Some(error), "details"),
        ),
    };
    if code.as_deref() == Some(CANCELLED) {
        return Error::Cancelled;
    }
    // The first root cause says more than "all shards failed".
    if let Some(root_cause) = text(error.get("root_cause").and_then(|c| c.get(0)), "reason")
        && root_cause != message
    {
        detail = Some(std::mem::replace(&mut message, root_cause));
    }
    if detail.is_none() {
        detail = text(error.get("caused_by"), "reason").filter(|d| *d != message);
    }
    Error::Database(DbError {
        position: sql.and_then(|sql| position(sql, &message)),
        code,
        message,
        detail,
        hint: None,
    })
}

/// `line 1:8: Unknown column [foo]` as the 1-based character offset in `sql`.
fn position(sql: &str, message: &str) -> Option<u32> {
    let rest = message.lines().last()?.strip_prefix("line ")?;
    let (line, rest) = rest.split_once(':')?;
    let column: usize = rest.split(':').next()?.trim().parse().ok()?;
    let line: usize = line.trim().parse().ok()?;
    let before: usize = sql
        .split('\n')
        .take(line.checked_sub(1)?)
        .map(|l| l.chars().count() + 1)
        .sum();
    u32::try_from(before + column).ok()
}

/// A statement of the editor that does not parse.
pub fn parse_error(statement: &str, message: &str) -> Error {
    let statement = statement.trim();
    let first = statement.lines().next().unwrap_or_default();
    Error::Database(DbError {
        code: None,
        message: format!("{message}: {first}"),
        detail: None,
        hint: None,
        position: None,
    })
}

/// Text from a failed reply that is not a server error (a proxy, a wrong port).
pub fn http_error(status: u16, body: &str) -> Error {
    let body = body.trim();
    let mut end = body.len().min(300);
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    Error::Other(format!("HTTP {status}: {}", &body[..end]))
}

/// Transport failures: the server is unreachable or the connection broke.
pub fn transport(e: reqwest::Error) -> Error {
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
    fn reads_the_reason_and_the_root_cause() {
        let body = r#"{"error":{"root_cause":[{"type":"index_not_found_exception","reason":"no such index [x]"}],
            "type":"index_not_found_exception","reason":"no such index [x]"},"status":404}"#;
        let Error::Database(e) = from_reply(404, body, None) else {
            panic!()
        };
        assert_eq!(e.code.as_deref(), Some("index_not_found_exception"));
        assert_eq!(e.message, "no such index [x]");
        assert_eq!(e.detail, None);
        let body = r#"{"error":{"root_cause":[{"type":"parsing_exception","reason":"bad query"}],
            "type":"search_phase_execution_exception","reason":"all shards failed"},"status":400}"#;
        let Error::Database(e) = from_reply(400, body, None) else {
            panic!()
        };
        assert_eq!(e.message, "bad query");
        assert_eq!(e.detail.as_deref(), Some("all shards failed"));
    }

    #[test]
    fn locates_sql_errors_in_characters() {
        assert_eq!(
            position("SELECT 1\nSELECT foo", "line 2:8: Unknown"),
            Some(17)
        );
        assert_eq!(position("SELECT foo FROM t", "line 1:8: Unknown"), Some(8));
        assert_eq!(
            position("SELECT foo", "Found 1 problem\nline 1:8: Unknown"),
            Some(8)
        );
        assert_eq!(position("SELECT", "no position"), None);
    }

    #[test]
    fn keeps_other_replies_and_cancels() {
        assert!(
            matches!(from_reply(502, "Bad Gateway", None), Error::Other(m) if m.starts_with("HTTP 502"))
        );
        let body =
            r#"{"error":{"type":"task_cancelled_exception","reason":"cancelled"},"status":400}"#;
        assert!(matches!(from_reply(400, body, None), Error::Cancelled));
        let body = r#"{"error":"plain reason","status":500}"#;
        assert!(
            matches!(from_reply(500, body, None), Error::Database(e) if e.message == "plain reason")
        );
    }
}
