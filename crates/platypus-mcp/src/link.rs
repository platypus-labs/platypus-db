//! The bridge to the open app: what an agent learns from it (the sessions open in the app)
//! and asks it (the user's yes to a change). The app serves it on a local port with a token
//! it writes to `agent-bridge.json` in the profile's data folder; the `--mcp` server and the
//! CLI reach it with [`HttpLink`], the app's own MCP endpoint with an [`AppLink`] of its own.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use uuid::Uuid;

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

/// What of the user's work an agent asks to see.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContextPart {
    /// The text of the active tab's editor.
    Editor,
    /// The part of it the user selected.
    Selection,
    /// The result the active tab's grid shows.
    Result,
}

impl ContextPart {
    pub const ALL: [Self; 3] = [Self::Editor, Self::Selection, Self::Result];
}

/// A look at what the user is working on, for the user to allow or refuse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextRequest {
    pub id: String,
    /// The agent, as it names itself.
    pub agent: String,
    /// The run of the agent's server: "share for this session" holds for it.
    pub session: String,
    pub parts: Vec<ContextPart>,
}

/// The rows of the result the active tab shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextResult {
    pub columns: Vec<String>,
    /// Text as the database gave it; `None` is SQL NULL.
    pub rows: Vec<Vec<Option<String>>>,
    /// The result has more rows than these, or they were cut to fit.
    pub truncated: bool,
    /// How many rows the whole result has, when the app knows (it does not while the rows
    /// are still being fetched).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_rows: Option<usize>,
}

/// What the user shared. Each part the user did not share, or the tab lacks, is `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserContext {
    /// The active tab's connection, by name.
    pub connection: Option<String>,
    pub connection_id: Option<Uuid>,
    /// The active tab's title.
    pub tab: Option<String>,
    pub editor: Option<String>,
    pub selection: Option<String>,
    pub result: Option<ContextResult>,
}

/// The user's answer to a [`ContextRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum ContextVerdict {
    /// Shared this once.
    Once { context: UserContext },
    /// Shared, and the next requests of the session are answered without asking.
    Session { context: UserContext },
    Deny {
        #[serde(default)]
        reason: Option<String>,
    },
}

/// How long the open app has to draw a diagram: it asks the user nothing, so a longer wait
/// means the page is not answering.
pub const DIAGRAM_TIMEOUT: Duration = Duration::from_secs(20);

/// A column of a table to draw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagramColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub primary_key: bool,
}

/// A foreign key to draw, to a table of the same diagram.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagramKey {
    pub name: String,
    pub columns: Vec<String>,
    pub table: String,
    pub ref_columns: Vec<String>,
}

/// A table to draw: only what the picture shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagramTable {
    pub name: String,
    pub columns: Vec<DiagramColumn>,
    pub foreign_keys: Vec<DiagramKey>,
}

/// A diagram an agent asks the app to draw as SVG. The model is what the agent could read
/// from the catalog anyway: the app opens no connection and asks the user nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagramRequest {
    pub id: String,
    /// The agent, as it names itself.
    pub agent: String,
    /// The run of the agent's server.
    pub session: String,
    pub schema: String,
    pub tables: Vec<DiagramTable>,
}

/// The app's answer to a [`DiagramRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum DiagramAnswer {
    /// The picture, as SVG text.
    Svg { svg: String },
    /// The app could not draw it.
    Failed { message: String },
}

/// What an agent reaches the open app for.
#[async_trait::async_trait]
pub trait AppLink: Send + Sync + std::fmt::Debug {
    /// The sessions open in the app; an error when it cannot be reached.
    async fn sessions(&self) -> Result<Vec<OpenSession>, String>;

    /// The user's answer to `request`; an error when nobody can be asked.
    async fn confirm(&self, request: WriteRequest) -> Result<Verdict, String>;

    /// What the user shares of their work, or why not; an error when nobody can be asked.
    /// An app that cannot share it keeps this answer.
    async fn context(&self, _request: ContextRequest) -> Result<ContextVerdict, String> {
        Err("this app cannot share what the user is working on".to_owned())
    }

    /// The SVG of the diagram in `request`, drawn by the app; an error when it cannot be
    /// reached. An app that cannot draw keeps this answer.
    async fn diagram(&self, _request: DiagramRequest) -> Result<DiagramAnswer, String> {
        Err("this app cannot draw diagrams".to_owned())
    }

    /// Tells the app that the agent `agent` is connected, as session `session`.
    async fn hello(&self, session: &str, agent: &str) -> Result<(), String>;

    /// Whether the app may answer at all: a quick look, before work for a question.
    fn reachable(&self) -> bool {
        true
    }
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

impl HttpLink {
    /// `POST`s `request` as JSON to `path` and reads the answer as `A`.
    async fn post<Q: Serialize, A: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        request: &Q,
        timeout: Duration,
    ) -> Result<A, String> {
        let body = serde_json::to_string(request).map_err(|e| e.to_string())?;
        let answer = self.call(path, Some(body), timeout).await?;
        serde_json::from_str(&answer).map_err(|e| e.to_string())
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
        // The user answers in the app: the call waits for them.
        self.post(
            "/v1/confirm",
            &request,
            ANSWER_TIMEOUT + Duration::from_secs(10),
        )
        .await
    }

    async fn context(&self, request: ContextRequest) -> Result<ContextVerdict, String> {
        // As for a change: the user answers in the app.
        self.post(
            "/v1/context",
            &request,
            ANSWER_TIMEOUT + Duration::from_secs(10),
        )
        .await
    }

    async fn diagram(&self, request: DiagramRequest) -> Result<DiagramAnswer, String> {
        // Nobody is asked: the page answers at once or not at all.
        self.post(
            "/v1/diagram",
            &request,
            DIAGRAM_TIMEOUT + Duration::from_secs(5),
        )
        .await
    }

    fn reachable(&self) -> bool {
        BridgeFile::read(&self.data_dir).is_some()
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

    #[tokio::test]
    async fn asks_what_the_user_works_on_with_the_token() {
        let dir = tempfile::tempdir().unwrap();
        let mut got = fake_bridge(
            dir.path(),
            200,
            r#"{"answer":"once","context":{"tab":"Orders","editor":"SELECT 1","result":{"columns":["n"],"rows":[["1"],[null]],"truncated":false}}}"#,
        )
        .await;
        let link = HttpLink::new(dir.path());
        let request = ContextRequest {
            id: "1".to_owned(),
            agent: "claude-code".to_owned(),
            session: "s".to_owned(),
            parts: vec![ContextPart::Editor, ContextPart::Result],
        };
        let ContextVerdict::Once { context } = link.context(request).await.unwrap() else {
            panic!("not shared once");
        };
        assert_eq!(context.editor.as_deref(), Some("SELECT 1"));
        assert_eq!(context.selection, None);
        assert_eq!(context.result.unwrap().rows[1], [None]);
        let sent = got.recv().await.unwrap();
        assert!(sent.starts_with("POST /v1/context HTTP/1.1"), "{sent}");
        assert!(sent.contains("Authorization: Bearer s3cret"), "{sent}");
        assert!(sent.contains(r#""parts":["editor","result"]"#), "{sent}");
    }

    #[tokio::test]
    async fn asks_the_open_app_to_draw_a_diagram() {
        let dir = tempfile::tempdir().unwrap();
        let mut got = fake_bridge(dir.path(), 200, r#"{"answer":"svg","svg":"<svg/>"}"#).await;
        let request = DiagramRequest {
            id: "1".to_owned(),
            agent: "claude-code".to_owned(),
            session: "s".to_owned(),
            schema: "main".to_owned(),
            tables: Vec::new(),
        };
        let answer = HttpLink::new(dir.path()).diagram(request).await.unwrap();
        assert_eq!(
            answer,
            DiagramAnswer::Svg {
                svg: "<svg/>".to_owned()
            }
        );
        let sent = got.recv().await.unwrap();
        assert!(sent.starts_with("POST /v1/diagram HTTP/1.1"), "{sent}");
        assert!(sent.contains("Authorization: Bearer s3cret"), "{sent}");
        assert_eq!(
            serde_json::from_str::<DiagramAnswer>(r#"{"answer":"failed","message":"no"}"#).unwrap(),
            DiagramAnswer::Failed {
                message: "no".to_owned()
            }
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
