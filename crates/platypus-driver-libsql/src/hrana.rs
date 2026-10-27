//! The Hrana 3 protocol over HTTP: `POST /v3/pipeline` runs requests on a server-side
//! stream, which the `baton` of each response keeps open for the next request.

use base64::Engine as _;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use platypus_core::{DbError, Error};
use platypus_driver_sqlite::columns::Owned;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct PipelineRequest<'a> {
    pub baton: Option<&'a str>,
    pub requests: Vec<StreamRequest<'a>>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamRequest<'a> {
    Execute { stmt: Stmt<'a> },
    GetAutocommit,
}

#[derive(Debug, Serialize)]
pub struct Stmt<'a> {
    pub sql: &'a str,
    pub want_rows: bool,
}

/// `POST /v3/cursor`: the steps of `batch` run on the stream, their rows streamed back one
/// entry per line, with no limit on the size of a result (a pipeline's response has one).
#[derive(Debug, Serialize)]
pub struct CursorRequest<'a> {
    pub baton: Option<&'a str>,
    pub batch: Batch<'a>,
}

#[derive(Debug, Serialize)]
pub struct Batch<'a> {
    pub steps: Vec<BatchStep<'a>>,
}

#[derive(Debug, Serialize)]
pub struct BatchStep<'a> {
    pub stmt: Stmt<'a>,
}

/// The first line of a cursor's response.
#[derive(Debug, Deserialize)]
pub struct CursorResponse {
    pub baton: Option<String>,
    pub base_url: Option<String>,
}

/// A line of a cursor's response after the first.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CursorEntry {
    StepBegin {
        cols: Vec<Col>,
    },
    StepEnd {
        affected_row_count: u64,
    },
    StepError {
        error: HranaError,
    },
    Row {
        row: Vec<Value>,
    },
    Error {
        error: HranaError,
    },
    #[serde(other)]
    Other,
}

/// What a cursor of one statement answered.
pub type CursorOutcome = (CursorResponse, Result<StmtResult, HranaError>);

/// The first line (the stream's baton) of the cursor response `body`, and the result or
/// error of its one statement.
///
/// # Errors
/// When a line is not one of the protocol.
pub fn cursor_result(body: &str) -> Result<CursorOutcome, Error> {
    let invalid = |e: serde_json::Error| Error::Other(format!("unexpected libSQL response: {e}"));
    let mut lines = body.lines().filter(|l| !l.trim().is_empty());
    let first = lines
        .next()
        .ok_or_else(|| Error::Other("empty libSQL response".to_owned()))?;
    let head: CursorResponse = serde_json::from_str(first).map_err(invalid)?;
    let mut result = StmtResult {
        cols: Vec::new(),
        rows: Vec::new(),
        affected_row_count: 0,
    };
    for line in lines {
        match serde_json::from_str(line).map_err(invalid)? {
            CursorEntry::StepBegin { cols } => result.cols = cols,
            CursorEntry::Row { row } => result.rows.push(row),
            CursorEntry::StepEnd { affected_row_count } => {
                result.affected_row_count = affected_row_count;
            }
            CursorEntry::StepError { error } | CursorEntry::Error { error } => {
                return Ok((head, Err(error)));
            }
            CursorEntry::Other => {}
        }
    }
    Ok((head, Ok(result)))
}

#[derive(Debug, Deserialize)]
pub struct PipelineResponse {
    pub baton: Option<String>,
    /// Where to send the next request of the stream, when not to the same server.
    pub base_url: Option<String>,
    pub results: Vec<StreamResult>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamResult {
    Ok { response: StreamResponse },
    Error { error: HranaError },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamResponse {
    Execute {
        result: StmtResult,
    },
    GetAutocommit {
        is_autocommit: bool,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct HranaError {
    pub message: String,
    pub code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StmtResult {
    pub cols: Vec<Col>,
    pub rows: Vec<Vec<Value>>,
    pub affected_row_count: u64,
}

#[derive(Debug, Deserialize)]
pub struct Col {
    pub name: Option<String>,
    pub decltype: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Value {
    Null,
    /// A 64-bit integer, as a string: JSON numbers lose precision beyond 2^53.
    Integer {
        value: String,
    },
    Float {
        value: f64,
    },
    Text {
        value: String,
    },
    Blob {
        base64: String,
    },
}

/// Base64 with or without the trailing `=`: servers send it without.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

impl Value {
    pub fn into_owned(self) -> Result<Owned, Error> {
        Ok(match self {
            Self::Null => Owned::Null,
            Self::Integer { value } => Owned::Integer(
                value
                    .parse()
                    .map_err(|_| Error::Other(format!("invalid integer `{value}` from libSQL")))?,
            ),
            Self::Float { value } => Owned::Real(value),
            Self::Text { value } => Owned::Text(value.into_bytes()),
            Self::Blob { base64 } => Owned::Blob(
                BASE64
                    .decode(base64)
                    .map_err(|e| Error::Other(format!("invalid blob from libSQL: {e}")))?,
            ),
        })
    }
}

impl HranaError {
    /// The error of a statement, with its position in `sql` when the message gives one.
    pub fn into_error(self, sql: &str) -> Error {
        if self.message.contains("interrupted") {
            return Error::Cancelled;
        }
        Error::Database(DbError {
            position: position(&self.message, sql),
            code: self.code,
            message: self.message,
            ..DbError::default()
        })
    }
}

/// The 1-based character position of an error in `sql`. SQLite's errors end with
/// `(at offset N)` (bytes); the server's own parser gives the line and column after the
/// token, ``around L1:15: `FROMM` ``, or `at (line, column)` at the end of the input.
fn position(message: &str, sql: &str) -> Option<u32> {
    let after = |marker: &str| Some(&message[message.find(marker)? + marker.len()..]);
    let number = |text: &str| -> Option<(usize, usize)> {
        let end = text
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.len());
        Some((text[..end].parse().ok()?, end))
    };
    if let Some((offset, _)) = after("(at offset ").and_then(number) {
        let prefix = sql.get(..offset)?;
        return u32::try_from(prefix.chars().count() + 1).ok();
    }
    let (line, column) = if let Some(rest) = after("around L") {
        let (line, end) = number(rest)?;
        let rest = rest[end..].strip_prefix(':')?;
        let (column, end) = number(rest)?;
        let token = rest[end..]
            .strip_prefix(": `")
            .and_then(|t| t.split('`').next())
            .map_or(0, |t| t.chars().count());
        (line, column.checked_sub(token)?)
    } else {
        let rest = after(" at (")?;
        let (line, end) = number(rest)?;
        let (column, _) = number(rest[end..].strip_prefix(", ")?)?;
        (line, column)
    };
    let before: usize = sql
        .split_inclusive('\n')
        .take(line.checked_sub(1)?)
        .map(|l| l.chars().count())
        .sum();
    u32::try_from(before + column).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_values() {
        let row: Vec<Value> = serde_json::from_str(
            r#"[{"type":"null"},{"type":"integer","value":"9223372036854775807"},
                {"type":"float","value":2.5},{"type":"text","value":"é"},
                {"type":"blob","base64":"AQI"}]"#,
        )
        .unwrap();
        let owned: Vec<Owned> = row.into_iter().map(|v| v.into_owned().unwrap()).collect();
        assert_eq!(
            owned,
            [
                Owned::Null,
                Owned::Integer(i64::MAX),
                Owned::Real(2.5),
                Owned::Text("é".as_bytes().to_vec()),
                Owned::Blob(vec![1, 2]),
            ]
        );
    }

    #[test]
    fn reads_error_positions() {
        let sql = "SELECT é, * FROMM t";
        let at = |message: &str| position(message, sql);
        assert_eq!(at("near \"FROMM\": syntax error (at offset 13)"), Some(13));
        assert_eq!(at("syntax error around L1:18: `FROMM`"), Some(13));
        assert_eq!(
            position("around L2:8: `FROMM`", "SELECT\n* FROMM"),
            Some(10)
        );
        assert_eq!(
            position("end of input at (2, 6)", "SELECT\n  1 +"),
            Some(13)
        );
        assert_eq!(at("no such table: t"), None);
        assert_eq!(at("(at offset 999)"), None);
    }

    #[test]
    fn reads_cursor_responses() {
        let body = r#"{"baton":"b","base_url":null}
{"type":"step_begin","step":0,"cols":[{"name":"x","decltype":null}]}
{"type":"row","row":[{"type":"integer","value":"1"}]}
{"type":"row","row":[{"type":"text","value":"a"}]}
{"type":"step_end","affected_row_count":0,"last_insert_rowid":null}
{"type":"replication_index","replication_index":3}
"#;
        let (head, result) = cursor_result(body).unwrap();
        assert_eq!(head.baton.as_deref(), Some("b"));
        let result = result.unwrap();
        assert_eq!(result.cols.len(), 1);
        assert_eq!(result.rows.len(), 2);
        let failed = r#"{"baton":null,"base_url":null}
{"type":"step_error","step":0,"error":{"message":"no such table: t","code":"SQLITE_ERROR"}}
"#;
        assert!(cursor_result(failed).unwrap().1.is_err());
    }

    #[test]
    fn parses_pipeline_responses() {
        let response: PipelineResponse = serde_json::from_str(
            r#"{"baton":"b","base_url":null,"results":[
                {"type":"ok","response":{"type":"execute","result":{"cols":[{"name":"a","decltype":"INTEGER"}],
                 "rows":[[{"type":"integer","value":"1"}]],"affected_row_count":0,"last_insert_rowid":null}}},
                {"type":"ok","response":{"type":"get_autocommit","is_autocommit":false}},
                {"type":"error","error":{"message":"no such table: x","code":"SQLITE_ERROR"}}]}"#,
        )
        .unwrap();
        assert_eq!(response.baton.as_deref(), Some("b"));
        assert!(matches!(
            &response.results[1],
            StreamResult::Ok {
                response: StreamResponse::GetAutocommit {
                    is_autocommit: false
                }
            }
        ));
        let StreamResult::Error { error } = response.results.into_iter().nth(2).unwrap() else {
            panic!("expected an error");
        };
        let Error::Database(e) = error.into_error("SELECT * FROM x") else {
            panic!("expected a database error");
        };
        assert_eq!(e.code.as_deref(), Some("SQLITE_ERROR"));
    }
}
