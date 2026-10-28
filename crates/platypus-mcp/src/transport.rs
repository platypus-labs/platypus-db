//! MCP over stdin and stdout, one JSON-RPC message per line. Unlike `rmcp`'s own stdio
//! transport, a line that is not JSON gets JSON-RPC's Parse error (`-32700`, `id` null), so
//! a client whose request was mangled hears about it instead of waiting for its timeout.

use std::sync::Arc;

use rmcp::RoleServer;
use rmcp::model::ErrorData;
use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::Transport;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

/// The longest line read; longer ones are dropped, as `rmcp` does.
const MAX_LINE: usize = 16 * 1024 * 1024;

/// What JSON-RPC answers a line that is not JSON.
const PARSE_ERROR: &str = r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error: the line is not JSON"}}"#;

/// What one line of input is.
#[derive(Debug)]
enum Line {
    Message(Box<RxJsonRpcMessage<RoleServer>>),
    /// Blank, or a notification this server does not know: nothing to answer.
    Ignored,
    /// Not JSON.
    NotJson,
    /// JSON, but no JSON-RPC message.
    Invalid,
}

fn parse(line: &[u8]) -> Line {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let line = line.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(line);
    if line.iter().all(u8::is_ascii_whitespace) {
        return Line::Ignored;
    }
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return Line::NotJson;
    };
    let notification = value.get("id").is_none() && value.get("method").is_some();
    match serde_json::from_value(value) {
        Ok(message) => Line::Message(Box::new(message)),
        // Like `rmcp`, notifications it cannot read (other protocols', newer ones) are
        // skipped.
        Err(_) if notification => Line::Ignored,
        Err(_) => Line::Invalid,
    }
}

/// The stdio transport: messages from `R`, answers to `W`.
pub(crate) struct LineTransport<R, W> {
    input: BufReader<R>,
    /// The line read so far: a read cancelled by the service loop resumes it.
    line: Vec<u8>,
    output: Arc<Mutex<W>>,
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> LineTransport<R, W> {
    pub(crate) fn new(input: R, output: W) -> Self {
        Self {
            input: BufReader::new(input),
            line: Vec::new(),
            output: Arc::new(Mutex::new(output)),
        }
    }
}

async fn write_line<W: AsyncWrite + Unpin>(output: &Mutex<W>, line: &[u8]) -> std::io::Result<()> {
    let mut output = output.lock().await;
    output.write_all(line).await?;
    output.write_all(b"\n").await?;
    output.flush().await
}

impl<R, W> Transport<RoleServer> for LineTransport<R, W>
where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send + 'static,
{
    type Error = std::io::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let output = self.output.clone();
        async move {
            let line = serde_json::to_vec(&item)?;
            write_line(&output, &line).await
        }
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
        loop {
            // The last line may end without a line feed, when stdin closes.
            if let Ok(0) | Err(_) = self.input.read_until(b'\n', &mut self.line).await {
                return None;
            }
            let line = std::mem::take(&mut self.line);
            if line.len() > MAX_LINE {
                continue;
            }
            let answer = match parse(&line) {
                Line::Message(message) => return Some(*message),
                Line::Ignored => continue,
                Line::NotJson => PARSE_ERROR.to_owned(),
                Line::Invalid => serde_json::to_string(&TxJsonRpcMessage::<RoleServer>::error(
                    ErrorData::invalid_request("Invalid request", None),
                    None,
                ))
                .unwrap_or_default(),
            };
            if write_line(&self.output, answer.as_bytes()).await.is_err() {
                return None;
            }
        }
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        self.output.lock().await.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lines() {
        assert!(matches!(
            parse(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\r\n"),
            Line::Message(_)
        ));
        assert!(matches!(parse(b"{this is not json\n"), Line::NotJson));
        assert!(matches!(parse(b"\xff\xfe\n"), Line::NotJson));
        assert!(matches!(parse(b"  \n"), Line::Ignored));
        assert!(matches!(
            parse(b"{\"jsonrpc\":\"2.0\",\"method\":5}"),
            Line::Ignored
        ));
        assert!(matches!(parse(b"[1, 2]"), Line::Invalid));
    }

    #[tokio::test]
    async fn answers_a_line_that_is_not_json_with_a_parse_error() {
        let input: &[u8] = b"{this is not json\n{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}";
        let (output, mut read_back) = tokio::io::duplex(4096);
        let mut transport = LineTransport::new(input, output);
        let message = Transport::<RoleServer>::receive(&mut transport).await;
        assert!(message.is_some());
        assert!(
            Transport::<RoleServer>::receive(&mut transport)
                .await
                .is_none()
        );
        drop(transport);
        let mut written = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut read_back, &mut written)
            .await
            .unwrap();
        let answer: Value = serde_json::from_str(written.trim()).unwrap();
        assert_eq!(answer.get("id"), Some(&Value::Null));
        assert_eq!(answer["error"]["code"], -32700);
    }
}
