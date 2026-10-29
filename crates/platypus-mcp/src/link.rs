//! The bridge to the open app: what an agent learns from it (the sessions open in the app)
//! and asks it (the user's yes to a change). The app serves it on a local port with a token
//! it writes to `agent-bridge.json` in the profile's data folder; the `--mcp` server and the
//! CLI reach it with [`HttpLink`], the app's own MCP endpoint with an [`AppLink`] of its own.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// The file of the bridge, in the profile's data folder.
pub const BRIDGE_FILE: &str = "agent-bridge.json";

/// How long the user has to answer a change before it is refused.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(300);

/// Where the open app listens, and the token it asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeFile {
    pub port: u16,
    pub token: String,
}

impl BridgeFile {
    #[must_use]
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join(BRIDGE_FILE)
    }

    /// The bridge of the app open on the profile at `data_dir`, if it is open.
    #[must_use]
    pub fn read(data_dir: &Path) -> Option<Self> {
        serde_json::from_str(&std::fs::read_to_string(Self::path(data_dir)).ok()?).ok()
    }
}

/// A session open in the app: a tab's, or the navigator's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenSession {
    /// The connection's name, as `list_connections` shows it.
    pub connection: String,
    /// The database it works on, where the connection names one.
    pub database: Option<String>,
    /// What the session is for: `main` for the one the connection's editor tabs and
    /// navigator share, `side` for one a dialog opened.
    pub purpose: String,
    /// It runs statements in manual-commit mode.
    pub manual_commit: bool,
    /// A transaction is open in it.
    pub transaction_open: bool,
}

/// A change an agent asks to run, for the user to allow or refuse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WriteRequest {
    pub id: String,
    /// The agent, as it names itself.
    pub agent: String,
    /// The run of the agent's server: "allow for this session" holds for it.
    pub session: String,
    pub connection: String,
    pub statement: String,
    /// The rows the planner expects it to change, where it tells.
    pub estimated_rows: Option<f64>,
    /// Its plan as an indented tree, where the database explains it.
    pub plan: Option<String>,
}

/// The user's answer to a [`WriteRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum Verdict {
    /// This statement runs.
    Once,
    /// This one and the next changes of the session on the connection run without asking.
    Session,
    Deny {
        #[serde(default)]
        reason: Option<String>,
    },
}

/// What an agent reaches the open app for.
#[async_trait::async_trait]
pub trait AppLink: Send + Sync + std::fmt::Debug {
    /// The sessions open in the app; an error when it cannot be reached.
    async fn sessions(&self) -> Result<Vec<OpenSession>, String>;

    /// The user's answer to `request`; an error when nobody can be asked.
    async fn confirm(&self, request: WriteRequest) -> Result<Verdict, String>;

    /// Tells the app that the agent `agent` is connected, as session `session`.
    async fn hello(&self, session: &str, agent: &str) -> Result<(), String>;
}

/// The bridge of an app open on another process, over HTTP on its local port.
#[derive(Debug, Clone)]
pub struct HttpLink {
    data_dir: PathBuf,
}

/// The app is not open, or does not answer.
pub(crate) const CLOSED: &str = "PlatypusDB is not open";

impl HttpLink {
    /// The bridge of the app on the profile at `data_dir`, read again at each call: the app
    /// may open, close or change its port while the agent runs.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    /// `POST`s `body` to `path` of the bridge (`GET` without one), within `timeout`; returns
    /// the answer's body.
    async fn call(
        &self,
        path: &str,
        body: Option<String>,
        timeout: Duration,
    ) -> Result<String, String> {
        let bridge = BridgeFile::read(&self.data_dir).ok_or(CLOSED)?;
        let call = async {
            let mut stream = TcpStream::connect(("127.0.0.1", bridge.port))
                .await
                .map_err(|_| CLOSED.to_owned())?;
            let body = body.unwrap_or_default();
            let method = if body.is_empty() { "GET" } else { "POST" };
            let request = format!(
                "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\n\
                 Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                bridge.token,
                body.len()
            );
            stream
                .write_all(request.as_bytes())
                .await
                .map_err(|e| e.to_string())?;
            let mut answer = Vec::new();
            stream
                .read_to_end(&mut answer)
                .await
                .map_err(|e| e.to_string())?;
            parse_response(&String::from_utf8_lossy(&answer))
        };
        tokio::time::timeout(timeout, call)
            .await
            .map_err(|_| "PlatypusDB did not answer in time".to_owned())?
    }
}

/// The body of an HTTP response, or why it failed.
fn parse_response(text: &str) -> Result<String, String> {
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or("a broken answer from PlatypusDB")?;
    let status = head.split_whitespace().nth(1).unwrap_or_default();
    // The bridge answers in one piece; a chunked one is read as it is.
    let body = if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        unchunk(body)
    } else {
        body.to_owned()
    };
    match status {
        "200" => Ok(body),
        "401" => Err(
            "PlatypusDB refused the bridge's token: it changed since the agent started".to_owned(),
        ),
        _ => Err(if body.trim().is_empty() {
            format!("PlatypusDB answered {status}")
        } else {
            body
        }),
    }
}

/// The data of a chunked body.
fn unchunk(mut body: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = body.split_once("\r\n") {
        let Ok(size) = usize::from_str_radix(size.trim(), 16) else {
            break;
        };
        if size == 0 || rest.len() < size {
            break;
        }
        out.push_str(&rest[..size]);
        body = rest[size..].trim_start_matches("\r\n");
    }
    out
}

#[async_trait::async_trait]
impl AppLink for HttpLink {
    async fn sessions(&self) -> Result<Vec<OpenSession>, String> {
        let body = self
            .call("/v1/sessions", None, Duration::from_secs(5))
            .await?;
        serde_json::from_str(&body).map_err(|e| e.to_string())
    }

    async fn confirm(&self, request: WriteRequest) -> Result<Verdict, String> {
        let body = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        // The user answers in the app: the call waits for them.
        let answer = self
            .call(
                "/v1/confirm",
                Some(body),
                ANSWER_TIMEOUT + Duration::from_secs(10),
            )
            .await?;
        serde_json::from_str(&answer).map_err(|e| e.to_string())
    }

    async fn hello(&self, session: &str, agent: &str) -> Result<(), String> {
        let body = serde_json::json!({ "session": session, "agent": agent }).to_string();
        self.call("/v1/hello", Some(body), Duration::from_secs(5))
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// A bridge that answers each request with `status` and `body`, and keeps what it got.
    async fn fake_bridge(
        dir: &Path,
        status: u16,
        body: &'static str,
    ) -> tokio::sync::mpsc::UnboundedReceiver<String> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let file = BridgeFile {
            port: listener.local_addr().unwrap().port(),
            token: "s3cret".to_owned(),
        };
        std::fs::write(BridgeFile::path(dir), serde_json::to_string(&file).unwrap()).unwrap();
        let (got, received) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buffer = vec![0u8; 8192];
                let n = socket.read(&mut buffer).await.unwrap();
                let _ = got.send(String::from_utf8_lossy(&buffer[..n]).into_owned());
                let answer = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(answer.as_bytes()).await.unwrap();
            }
        });
        received
    }

    #[tokio::test]
    async fn asks_the_open_app_with_its_token() {
        let dir = tempfile::tempdir().unwrap();
        let mut got = fake_bridge(dir.path(), 200, r#"{"answer":"session"}"#).await;
        let link = HttpLink::new(dir.path());
        let request = WriteRequest {
            id: "1".to_owned(),
            agent: "claude-code".to_owned(),
            session: "s".to_owned(),
            connection: "shop".to_owned(),
            statement: "DELETE FROM t WHERE id = 1".to_owned(),
            estimated_rows: Some(1.0),
            plan: None,
        };
        assert_eq!(link.confirm(request).await.unwrap(), Verdict::Session);
        let sent = got.recv().await.unwrap();
        assert!(sent.starts_with("POST /v1/confirm HTTP/1.1"), "{sent}");
        assert!(sent.contains("Authorization: Bearer s3cret"), "{sent}");
        assert!(sent.contains("DELETE FROM t WHERE id = 1"), "{sent}");
    }

    #[tokio::test]
    async fn says_when_the_app_is_closed_or_refuses() {
        let dir = tempfile::tempdir().unwrap();
        let link = HttpLink::new(dir.path());
        assert_eq!(link.sessions().await.unwrap_err(), CLOSED);
        let _got = fake_bridge(dir.path(), 401, "").await;
        assert!(link.sessions().await.unwrap_err().contains("token"));
    }

    #[test]
    fn reads_chunked_answers() {
        assert_eq!(
            parse_response(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n"
            )
            .unwrap(),
            "hello"
        );
        assert_eq!(
            parse_response("HTTP/1.1 403 X\r\n\r\nno").unwrap_err(),
            "no"
        );
    }

    #[test]
    fn verdicts_as_the_app_writes_them() {
        assert_eq!(
            serde_json::from_str::<Verdict>(r#"{"answer":"once"}"#).unwrap(),
            Verdict::Once
        );
        assert_eq!(
            serde_json::from_str::<Verdict>(r#"{"answer":"deny","reason":"not now"}"#).unwrap(),
            Verdict::Deny {
                reason: Some("not now".to_owned())
            }
        );
    }
}
