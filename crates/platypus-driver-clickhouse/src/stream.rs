//! Result rows: the `ArrowStream` response body decoded as it arrives, in batches of at
//! most the requested size. Nothing is buffered beyond the IPC message being read.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex as StdMutex};

use arrow_array::{RecordBatch, StringArray};
use arrow_buffer::Buffer;
use arrow_ipc::reader::StreamDecoder;
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use bytes::Bytes;
use futures::stream::BoxStream;
use futures::{StreamExt, TryStreamExt};
use platypus_core::{Error, RecordBatchStream, Result};
use reqwest::Response;
use tokio::sync::OwnedMutexGuard;

use crate::error::{ServerError, transport};
use crate::http::Http;
use crate::types::Adapter;

/// Raw chunks kept to read an exception the server writes into the stream.
const RECENT_CHUNKS: usize = 4;
/// Bytes read after a broken stream to find its exception.
const ERROR_TAIL: usize = 1 << 20;

/// The statement running on the session: holds the connection's lock and, when dropped
/// before the result was read to the end, stops the statement on the server.
#[derive(Debug)]
pub struct Running {
    pub _guard: OwnedMutexGuard<()>,
    pub current: Arc<StdMutex<Option<String>>>,
    pub http: Http,
    pub query_id: String,
    pub complete: bool,
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Ok(mut current) = self.current.lock() {
            *current = None;
        }
        if self.complete {
            return;
        }
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let (http, query_id) = (self.http.clone(), std::mem::take(&mut self.query_id));
            runtime.spawn(async move { http.kill(&query_id).await });
        }
    }
}

struct Decoder {
    body: BoxStream<'static, reqwest::Result<Bytes>>,
    ipc: StreamDecoder,
    buffer: Buffer,
    recent: VecDeque<Bytes>,
    /// A decoded batch and how many of its rows were handed out.
    pending: Option<(RecordBatch, usize)>,
    batch_size: usize,
    sql: String,
    running: Option<Running>,
    adapter: Option<Adapter>,
}

impl Decoder {
    /// The next slice of rows, `None` at the end.
    async fn next_batch(&mut self) -> Option<Result<RecordBatch>> {
        loop {
            if let Some((batch, offset)) = &mut self.pending {
                if *offset < batch.num_rows() {
                    let len = self.batch_size.min(batch.num_rows() - *offset);
                    let slice = batch.slice(*offset, len);
                    *offset += len;
                    return Some(match &self.adapter {
                        Some(adapter) => adapter.apply(&slice),
                        None => Ok(slice),
                    });
                }
                self.pending = None;
            }
            self.running.as_ref()?;
            match self.decode().await {
                Ok(Some(batch)) => self.pending = Some((batch, 0)),
                Ok(None) => {
                    self.release();
                    return None;
                }
                Err(e) => {
                    self.release();
                    return Some(Err(e));
                }
            }
        }
    }

    /// The statement is over on the server: nothing to stop.
    fn release(&mut self) {
        if let Some(mut running) = self.running.take() {
            running.complete = true;
        }
    }

    /// The next whole IPC batch, reading the body as needed; `None` at its end.
    async fn decode(&mut self) -> Result<Option<RecordBatch>> {
        loop {
            if !self.buffer.is_empty() {
                match self.ipc.decode(&mut self.buffer) {
                    Ok(Some(batch)) => return Ok(Some(batch)),
                    Ok(None) => {}
                    Err(e) => return Err(self.stream_error(Error::Arrow(e)).await),
                }
            }
            match self.body.next().await {
                Some(Ok(chunk)) => {
                    if self.recent.len() == RECENT_CHUNKS {
                        self.recent.pop_front();
                    }
                    self.recent.push_back(chunk.clone());
                    self.buffer = Buffer::from(chunk);
                }
                Some(Err(e)) => return Err(self.stream_error(transport(e)).await),
                None => {
                    return match self.ipc.finish() {
                        Ok(()) => Ok(None),
                        Err(e) => Err(self.stream_error(Error::Arrow(e)).await),
                    };
                }
            }
        }
    }

    /// The exception the server wrote into the stream, else `fallback`.
    async fn stream_error(&mut self, fallback: Error) -> Error {
        let mut tail: Vec<u8> = self.recent.iter().flatten().copied().collect();
        while tail.len() < ERROR_TAIL {
            match self.body.next().await {
                Some(Ok(chunk)) => tail.extend_from_slice(&chunk),
                _ => break,
            }
        }
        ServerError::parse(&String::from_utf8_lossy(&tail))
            .map_or(fallback, |e| e.into_error(&self.sql))
    }
}

/// The rows of an `ArrowStream` response, typed by the described `types` when known.
/// Returns once the schema has arrived.
pub async fn arrow_rows(
    response: Response,
    batch_size: usize,
    sql: &str,
    running: Running,
    types: &[(String, String)],
) -> Result<RecordBatchStream> {
    let mut decoder = Decoder {
        body: response.bytes_stream().boxed(),
        ipc: StreamDecoder::new(),
        buffer: Buffer::from(Vec::<u8>::new()),
        recent: VecDeque::new(),
        pending: None,
        batch_size: batch_size.max(1),
        sql: sql.to_owned(),
        running: Some(running),
        adapter: None,
    };
    // The schema message comes first; the same read may already hold a batch.
    while decoder.ipc.schema().is_none() {
        match decoder.decode().await {
            Ok(Some(batch)) => decoder.pending = Some((batch, 0)),
            Ok(None) if decoder.ipc.schema().is_some() => break,
            Ok(None) => {
                decoder.release();
                return Err(Error::Other("the result has no schema".to_owned()));
            }
            Err(e) => {
                decoder.release();
                return Err(e);
            }
        }
    }
    let ipc_schema = decoder
        .ipc
        .schema()
        .ok_or_else(|| Error::Other("the result has no schema".to_owned()))?;
    let adapter = Adapter::new(&ipc_schema, types);
    let schema = adapter.schema();
    decoder.adapter = Some(adapter);
    let batches = futures::stream::unfold(decoder, |mut d| async move {
        d.next_batch().await.map(|batch| (batch, d))
    });
    Ok(RecordBatchStream::new(schema, batches.boxed()))
}

/// A result in a text format the user asked for (`FORMAT JSON`, `FORMAT TSV`): one row
/// per line, in a column named after the format.
pub async fn text_rows(
    response: Response,
    format: &str,
    running: Running,
) -> Result<RecordBatchStream> {
    let body: Vec<Bytes> = response
        .bytes_stream()
        .try_collect()
        .await
        .map_err(transport)?;
    let mut running = running;
    running.complete = true;
    drop(running);
    let text = String::from_utf8_lossy(&body.concat()).into_owned();
    let schema: SchemaRef = Arc::new(Schema::new(vec![Field::new(format, DataType::Utf8, false)]));
    let lines = StringArray::from_iter_values(text.lines());
    let batch = RecordBatch::try_new(Arc::clone(&schema), vec![Arc::new(lines)])?;
    Ok(RecordBatchStream::new(
        schema,
        futures::stream::iter([Ok(batch)]).boxed(),
    ))
}
