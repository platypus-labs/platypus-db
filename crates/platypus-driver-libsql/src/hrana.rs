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
