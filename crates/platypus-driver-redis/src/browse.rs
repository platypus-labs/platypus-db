//! `BROWSE`: the keys of one type as the rows of a virtual table, read as the reader asks
//! for them. Keys come from `SCAN … TYPE` (never `KEYS`, which blocks the server), the
//! contents of big keys a chunk at a time (`HSCAN`, `SSCAN`, `LRANGE`, `ZRANGE`, `XRANGE`).

use std::collections::VecDeque;
use std::sync::Arc;

use arrow_schema::{DataType, Schema, SchemaRef};
use futures::stream::{self, StreamExt};
use platypus_core::{Error, ExecResult, RecordBatchStream, Result};
use redis::aio::MultiplexedConnection;
use redis::{Cmd, Value};

use crate::command::{Browse, KeyType, display};
use crate::value::{Column, batch, field, text, to_json};

/// Keys asked of each `SCAN`, and items of each chunk of a big key.
const CHUNK: usize = 1000;

/// The schema of the virtual table of `kind`.
#[must_use]
pub fn schema(kind: KeyType) -> SchemaRef {
    let fields: Vec<_> = kind
        .columns()
        .iter()
        .map(|&(name, _)| match (kind, name) {
            (_, "ttl") => field(name, DataType::Int64, "ttl"),
            (KeyType::List, "index") => field(name, DataType::Int64, "integer"),
            (KeyType::Zset, "score") => field(name, DataType::Float64, "double"),
            (KeyType::Json, "value") => field(name, DataType::Utf8, "json"),
            (KeyType::Stream, "fields") => field(name, DataType::Utf8, "document"),
            _ => field(name, DataType::Utf8, "string"),
        })
        .collect();
    Arc::new(Schema::new(fields))
}

/// The rows built for one batch.
#[derive(Default)]
struct Rows {
    keys: Vec<Option<String>>,
    /// `field`, `member`, `index` (as text) or `id`.
    second: Vec<Option<String>>,
    index: Vec<Option<i64>>,
    values: Vec<Option<String>>,
    numbers: Vec<Option<f64>>,
    ttls: Vec<Option<i64>>,
}

impl Rows {
    fn len(&self) -> usize {
        self.keys.len()
    }

    fn truncate(&mut self, len: usize) {
        for column in [&mut self.keys, &mut self.second, &mut self.values] {
            column.truncate(len);
        }
        self.index.truncate(len);
        self.numbers.truncate(len);
        self.ttls.truncate(len);
    }

    fn columns(self, kind: KeyType) -> Vec<Column> {
        use Column::{Float, Int, Text};
        match kind {
            KeyType::String | KeyType::Json => {
                vec![Text(self.keys), Text(self.values), Int(self.ttls)]
            }
            KeyType::Hash | KeyType::Stream => {
                vec![Text(self.keys), Text(self.second), Text(self.values)]
            }
            KeyType::List => vec![Text(self.keys), Int(self.index), Text(self.values)],
            KeyType::Set => vec![Text(self.keys), Text(self.second)],
            KeyType::Zset => vec![Text(self.keys), Text(self.second), Float(self.numbers)],
        }
    }
}

/// Where the reading of a big key is.
enum Inside {
    /// The cursor of `HSCAN` or `SSCAN`; 0 once it has gone round.
    Scan {
        key: Vec<u8>,
        cursor: u64,
        started: bool,
    },
    /// The next index of `LRANGE` or `ZRANGE`.
    Range { key: Vec<u8>, start: i64 },
    /// The last id read by `XRANGE`.
    Stream { key: Vec<u8>, after: Option<String> },
}

struct Browser {
    conn: MultiplexedConnection,
    kind: KeyType,
    pattern: Vec<u8>,
    /// Rows still to read, under `LIMIT`.
    left: u64,
    /// `None` once `SCAN` has gone round.
    cursor: Option<u64>,
    keys: VecDeque<Vec<u8>>,
    inside: Option<Inside>,
}

/// The rows of `browse` on `conn`, `size` per batch.
#[must_use]
pub fn run(conn: MultiplexedConnection, browse: Browse, size: usize) -> ExecResult {
    let kind = browse.kind;
    let schema = schema(kind);
    let browser = Browser {
        conn,
        kind,
        pattern: browse.pattern.unwrap_or_else(|| b"*".to_vec()),
        left: browse.limit.unwrap_or(u64::MAX),
        cursor: Some(0),
        keys: VecDeque::new(),
        inside: None,
    };
    let size = size.max(1);
    let batches_schema = Arc::clone(&schema);
    let inner = stream::unfold(Some(browser), move |browser| {
        let schema = Arc::clone(&batches_schema);
        async move {
            let mut browser = browser?;
            match browser.next(size).await {
                Ok(rows) if rows.len() == 0 => None,
                Ok(rows) => {
                    let batch = batch(&schema, rows.columns(browser.kind));
                    Some((batch, Some(browser)))
                }
                Err(e) => Some((Err(e), None)),
            }
        }
    })
    .boxed();
    ExecResult::Rows(RecordBatchStream::new(schema, inner))
}

async fn query(conn: &mut MultiplexedConnection, cmd: &Cmd) -> Result<Value> {
    match cmd.query_async::<Value>(conn).await {
        Ok(Value::ServerError(e)) => Err(crate::error::server(&e)),
        Ok(v) => Ok(v),
        Err(e) => Err(crate::error::map(&e)),
    }
}

fn int(value: &Value) -> Option<i64> {
    match value {
        Value::Int(n) => Some(*n),
        Value::BulkString(b) => std::str::from_utf8(b).ok()?.parse().ok(),
        Value::SimpleString(s) => s.parse().ok(),
        _ => None,
    }
}

fn bytes(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::BulkString(b) => Some(b.clone()),
        Value::SimpleString(s) => Some(s.clone().into_bytes()),
        Value::VerbatimString { text, .. } => Some(text.clone().into_bytes()),
        Value::Nil => None,
        other => Some(text(other).into_bytes()),
    }
}

fn items(value: Value) -> Vec<Value> {
    match value {
        Value::Array(items) | Value::Set(items) => items,
        Value::Map(pairs) => pairs.into_iter().flat_map(|(k, v)| [k, v]).collect(),
        Value::Nil => Vec::new(),
        other => vec![other],
    }
}

/// A `[cursor, items]` reply of the `SCAN` family.
fn scan_reply(value: Value) -> Result<(u64, Vec<Value>)> {
    let mut parts = items(value).into_iter();
    let (Some(cursor), Some(found)) = (parts.next(), parts.next()) else {
        return Err(Error::Other("unexpected SCAN reply".to_owned()));
    };
    let cursor = int(&cursor)
        .and_then(|c| u64::try_from(c).ok())
        .or_else(|| bytes(&cursor).and_then(|b| String::from_utf8(b).ok()?.parse().ok()))
        .ok_or_else(|| crate::value::unexpected("SCAN", &cursor))?;
    Ok((cursor, items(found)))
}

/// A seconds TTL as the column holds it: `None` without expiry (or for a key gone).
const fn ttl(value: &Value) -> Option<i64> {
    match value {
        Value::Int(n) if *n >= 0 => Some(*n),
        _ => None,
    }
}

impl Browser {
    async fn next(&mut self, size: usize) -> Result<Rows> {
        let mut rows = Rows::default();
        let limit = usize::try_from(self.left).unwrap_or(usize::MAX);
        let size = size.min(limit);
        while rows.len() < size {
            let room = size - rows.len();
            if self.inside.is_some() {
                self.read_inside(room.min(CHUNK), &mut rows).await?;
            } else if !self.keys.is_empty() {
                if matches!(self.kind, KeyType::String | KeyType::Json) {
                    let take = room.min(CHUNK).min(self.keys.len());
                    let keys: Vec<_> = self.keys.drain(..take).collect();
                    self.read_values(&keys, &mut rows).await?;
                } else if let Some(key) = self.keys.pop_front() {
                    self.inside = Some(match self.kind {
                        KeyType::List | KeyType::Zset => Inside::Range { key, start: 0 },
                        KeyType::Stream => Inside::Stream { key, after: None },
                        _ => Inside::Scan {
                            key,
                            cursor: 0,
                            started: false,
                        },
                    });
                }
            } else if let Some(cursor) = self.cursor {
                self.scan(cursor).await?;
            } else {
                break;
            }
        }
        // A small key comes whole, whatever the count asked.
        rows.truncate(limit);
        self.left = self.left.saturating_sub(rows.len() as u64);
        Ok(rows)
    }

    async fn scan(&mut self, cursor: u64) -> Result<()> {
        let mut cmd = redis::cmd("SCAN");
        cmd.arg(cursor)
            .arg("MATCH")
            .arg(&self.pattern)
            .arg("COUNT")
            .arg(CHUNK)
            .arg("TYPE")
            .arg(self.kind.redis_name());
        let (next, found) = scan_reply(query(&mut self.conn, &cmd).await?)?;
        let mut keys: Vec<Vec<u8>> = found.iter().filter_map(bytes).collect();
        keys.sort_unstable();
        self.keys.extend(keys);
        self.cursor = (next != 0).then_some(next);
        Ok(())
    }

    /// Values and TTLs of string or JSON keys, in one round trip.
    async fn read_values(&mut self, keys: &[Vec<u8>], rows: &mut Rows) -> Result<()> {
        let mut pipe = redis::pipe();
        for key in keys {
            match self.kind {
                KeyType::Json => pipe.cmd("JSON.GET").arg(key),
                _ => pipe.cmd("GET").arg(key),
            };
            pipe.cmd("TTL").arg(key);
        }
        let replies: Vec<Value> = pipe
            .ignore_errors()
            .query_async(&mut self.conn)
            .await
            .map_err(|e| crate::error::map(&e))?;
        for (key, pair) in keys.iter().zip(replies.chunks(2)) {
            let [value, expiry] = pair else { continue };
            // Gone, or of another type, since the scan.
            if matches!(value, Value::Nil | Value::ServerError(_)) {
                continue;
            }
            rows.keys.push(Some(display(key)));
            rows.values.push(bytes(value).map(|b| display(&b)));
            rows.ttls.push(ttl(expiry));
        }
        Ok(())
    }

    async fn read_inside(&mut self, count: usize, rows: &mut Rows) -> Result<()> {
        let Some(inside) = self.inside.take() else {
            return Ok(());
        };
        let kind = self.kind;
        let (more, next) = match inside {
            Inside::Scan {
                key,
                cursor,
                started,
            } => {
                if started && cursor == 0 {
                    return Ok(());
                }
                let name = if kind == KeyType::Hash {
                    "HSCAN"
                } else {
                    "SSCAN"
                };
                let mut cmd = redis::cmd(name);
                cmd.arg(&key).arg(cursor).arg("COUNT").arg(count);
                let (next, found) = scan_reply(query(&mut self.conn, &cmd).await?)?;
                let shown = display(&key);
                if kind == KeyType::Hash {
                    for pair in found.chunks(2) {
                        let [f, v] = pair else { continue };
                        rows.keys.push(Some(shown.clone()));
                        rows.second.push(bytes(f).map(|b| display(&b)));
                        rows.values.push(bytes(v).map(|b| display(&b)));
                    }
                } else {
                    for member in &found {
                        rows.keys.push(Some(shown.clone()));
                        rows.second.push(bytes(member).map(|b| display(&b)));
                    }
                }
                (
                    next != 0,
                    Inside::Scan {
                        key,
                        cursor: next,
                        started: true,
                    },
                )
            }
            Inside::Range { key, start } => {
                let n = i64::try_from(count).unwrap_or(i64::MAX);
                let stop = start + n - 1;
                let read = if kind == KeyType::List {
                    self.list_range(&key, start, stop, rows).await?
                } else {
                    self.zset_range(&key, start, stop, rows).await?
                };
                (
                    read == count,
                    Inside::Range {
                        key,
                        start: stop + 1,
                    },
                )
            }
            Inside::Stream { key, after } => {
                let (read, last) = self
                    .stream_range(&key, after.as_deref(), count, rows)
                    .await?;
                (read == count, Inside::Stream { key, after: last })
            }
        };
        if more {
            self.inside = Some(next);
        }
        Ok(())
    }

    async fn list_range(
        &mut self,
        key: &[u8],
        start: i64,
        stop: i64,
        rows: &mut Rows,
    ) -> Result<usize> {
        let mut cmd = redis::cmd("LRANGE");
        cmd.arg(key).arg(start).arg(stop);
        let found = items(query(&mut self.conn, &cmd).await?);
        let shown = display(key);
        for (i, value) in (start..).zip(&found) {
            rows.keys.push(Some(shown.clone()));
            rows.index.push(Some(i));
            rows.values.push(bytes(value).map(|b| display(&b)));
        }
        Ok(found.len())
    }

    async fn zset_range(
        &mut self,
        key: &[u8],
        start: i64,
        stop: i64,
        rows: &mut Rows,
    ) -> Result<usize> {
        let mut cmd = redis::cmd("ZRANGE");
        cmd.arg(key).arg(start).arg(stop).arg("WITHSCORES");
        // RESP3 answers `[[member, score], …]`, RESP2 `[member, score, …]`.
        let flat: Vec<Value> = items(query(&mut self.conn, &cmd).await?)
            .into_iter()
            .flat_map(|item| match item {
                Value::Array(pair) => pair,
                other => vec![other],
            })
            .collect();
        let shown = display(key);
        for pair in flat.chunks(2) {
            let [member, score] = pair else { continue };
            rows.keys.push(Some(shown.clone()));
            rows.second.push(bytes(member).map(|b| display(&b)));
            rows.numbers.push(match score {
                Value::Double(d) => Some(*d),
                other => text(other).parse().ok(),
            });
        }
        Ok(flat.len() / 2)
    }

    async fn stream_range(
        &mut self,
        key: &[u8],
        after: Option<&str>,
        count: usize,
        rows: &mut Rows,
    ) -> Result<(usize, Option<String>)> {
        let start = after.map_or_else(|| "-".to_owned(), |id| format!("({id}"));
        let mut cmd = redis::cmd("XRANGE");
        cmd.arg(key).arg(start).arg("+").arg("COUNT").arg(count);
        let entries = items(query(&mut self.conn, &cmd).await?);
        let shown = display(key);
        let mut last = after.map(str::to_owned);
        for entry in &entries {
            let Value::Array(parts) = entry else { continue };
            let [id, fields] = parts.as_slice() else {
                continue;
            };
            let id = text(id);
            let pairs = items(fields.clone());
            let object: serde_json::Map<String, serde_json::Value> = pairs
                .chunks(2)
                .filter_map(|p| match p {
                    [k, v] => Some((text(k), to_json(v))),
                    _ => None,
                })
                .collect();
            rows.keys.push(Some(shown.clone()));
            rows.second.push(Some(id.clone()));
            rows.values
                .push(Some(serde_json::Value::Object(object).to_string()));
            last = Some(id);
        }
        Ok((entries.len(), last))
    }
}
