//! The bridge with agents: a server on a local port, behind a token, through which agents see
//! the open app. The `--mcp` server and the CLI ask it for the sessions open in the app and
//! for the user's yes to a change ([`platypus_mcp::HttpLink`]), and tell it they are connected;
//! MCP clients that speak HTTP use its `/mcp` endpoint, the same tools as `--mcp`.
//!
//! While it runs, its port and token are in `agent-bridge.json` in the data folder, which only
//! the user can read; the token stays the same across starts (`agent-token`) until the user
//! makes a new one, so that the clients set up with it keep working.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use platypus_mcp::{
    ANSWER_TIMEOUT, AppLink, BridgeFile, ContextRequest, ContextVerdict, HELLO_EVERY, OpenSession,
    Verdict, WriteRequest,
};
use serde::Serialize;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[cfg(windows)]
mod acl;
mod commands;
mod http;

pub use commands::*;

use crate::commands::lock;

/// The port the bridge listens on unless Settings › Agents say otherwise.
pub const DEFAULT_PORT: u16 = 47_337;

/// The file of the token, in the data folder.
const TOKEN_FILE: &str = "agent-token";

/// An agent is forgotten when it has not said hello for this long.
const FORGET_AFTER: Duration = Duration::from_secs(HELLO_EVERY.as_secs() * 3);

/// The page's events.
pub const WRITE_REQUEST_EVENT: &str = "agent-write-request";
/// A request no longer waits: answered, timed out, or its agent went away.
pub const WRITE_SETTLED_EVENT: &str = "agent-write-settled";
/// The same for the agents' requests to see what the user works on.
pub const CONTEXT_REQUEST_EVENT: &str = "agent-context-request";
pub const CONTEXT_SETTLED_EVENT: &str = "agent-context-settled";
pub const AGENTS_EVENT: &str = "agents-connected";

/// What the bridge needs from the app: kept apart so that tests run it without a window.
pub trait Host: Send + Sync + 'static {
    /// The sessions open in the app.
    fn sessions(&self) -> Vec<OpenSession>;
    /// Sends `event` with `payload` to the page.
    fn notify(&self, event: &str, payload: serde_json::Value);
}

/// An agent connected to the bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectedAgent {
    /// The run of its server.
    pub session: String,
    /// Its name, as it introduced itself.
    pub agent: String,
}

/// A request waiting for the user.
struct Pending<R, V> {
    request: R,
    since: Instant,
    answer: oneshot::Sender<V>,
}

/// A request waiting for the user, as the page gets it when it starts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Waited<R> {
    pub request: R,
    /// How long it has waited, so that the page counts down from there.
    pub waited_ms: u64,
}

/// A change waiting for the user.
pub type WaitingWrite = Waited<WriteRequest>;
/// A request to see what the user works on, waiting for the user.
pub type WaitingContext = Waited<ContextRequest>;

/// The requests of one kind waiting for the user, by id, with when they came and how to
/// answer them.
struct Queue<R, V> {
    pending: Mutex<HashMap<String, Pending<R, V>>>,
    /// The page's event for a request that no longer waits.
    settled: &'static str,
}

impl<R: Clone + Serialize, V> Queue<R, V> {
    fn new(settled: &'static str) -> Self {
        Self {
            pending: Mutex::default(),
            settled,
        }
    }

    /// Answers the request `id`; `false` when it no longer waits.
    fn answer(&self, id: &str, verdict: V) -> bool {
        lock(&self.pending)
            .remove(id)
            .is_some_and(|pending| pending.answer.send(verdict).is_ok())
    }

    /// The requests waiting, the oldest first.
    fn waiting(&self) -> Vec<Waited<R>> {
        let mut waiting: Vec<(Instant, R)> = lock(&self.pending)
            .values()
            .map(|p| (p.since, p.request.clone()))
            .collect();
        waiting.sort_by_key(|(since, _)| *since);
        waiting
            .into_iter()
            .map(|(since, request)| Waited {
                request,
                waited_ms: u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX),
            })
            .collect()
    }

    /// Tells the page of `request` with `event` and waits for the user's answer; `refused`
    /// stands for it when nobody answers within [`ANSWER_TIMEOUT`].
    async fn ask(
        &self,
        host: &dyn Host,
        (id, request): (String, R),
        event: &str,
        refused: V,
    ) -> Result<V, String> {
        let (answer, answered) = oneshot::channel();
        let payload = serde_json::to_value(&request).map_err(|e| e.to_string())?;
        lock(&self.pending).insert(
            id.clone(),
            Pending {
                request,
                since: Instant::now(),
                answer,
            },
        );
        let _waiting = Waiting {
            queue: self,
            host,
            id,
        };
        host.notify(event, payload);
        Ok(match tokio::time::timeout(ANSWER_TIMEOUT, answered).await {
            Ok(Ok(verdict)) => verdict,
            _ => refused,
        })
    }
}

/// What the bridge shares between its requests, the app's endpoint and the commands.
pub struct Link {
    host: Box<dyn Host>,
    /// The changes waiting for the user.
    writes: Queue<WriteRequest, Verdict>,
    /// The looks at the user's work waiting for them.
    contexts: Queue<ContextRequest, ContextVerdict>,
    /// The agents connected, by session, with when they last said hello.
    agents: Mutex<HashMap<String, (String, Instant)>>,
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link").finish_non_exhaustive()
    }
}

impl Link {
    pub fn new(host: impl Host) -> Self {
        Self {
            host: Box::new(host),
            writes: Queue::new(WRITE_SETTLED_EVENT),
            contexts: Queue::new(CONTEXT_SETTLED_EVENT),
            agents: Mutex::default(),
        }
    }

    /// Answers the change `id`; `false` when it no longer waits.
    pub fn answer(&self, id: &str, verdict: Verdict) -> bool {
        self.writes.answer(id, verdict)
    }

    /// The changes waiting for the user, the oldest first.
    pub fn waiting(&self) -> Vec<WaitingWrite> {
        self.writes.waiting()
    }

    /// Answers the request `id` to see what the user works on; `false` when it no longer waits.
    pub fn answer_context(&self, id: &str, verdict: ContextVerdict) -> bool {
        self.contexts.answer(id, verdict)
    }

    /// The requests to see what the user works on that wait, the oldest first.
    pub fn waiting_contexts(&self) -> Vec<WaitingContext> {
        self.contexts.waiting()
    }

    /// The agents connected, by name.
    pub fn connected(&self) -> Vec<ConnectedAgent> {
        let mut agents: Vec<ConnectedAgent> = lock(&self.agents)
            .iter()
            .map(|(session, (agent, _))| ConnectedAgent {
                session: session.clone(),
                agent: agent.clone(),
            })
            .collect();
        agents.sort_by(|a, b| a.agent.cmp(&b.agent).then(a.session.cmp(&b.session)));
        agents
    }

    fn agents_changed(&self) {
        self.host.notify(
            AGENTS_EVENT,
            serde_json::to_value(self.connected()).unwrap_or_default(),
        );
    }

    /// Forgets the agents that stopped saying hello.
    fn forget_silent(&self) {
        let forgot = {
            let mut agents = lock(&self.agents);
            let before = agents.len();
            agents.retain(|_, (_, seen)| seen.elapsed() < FORGET_AFTER);
            agents.len() != before
        };
        if forgot {
            self.agents_changed();
        }
    }
}

/// Removes a request from the waiting ones when its wait ends, however it ends (the agent's
/// request dropped included), and tells the page.
struct Waiting<'a, R, V> {
    queue: &'a Queue<R, V>,
    host: &'a dyn Host,
    id: String,
}

impl<R, V> Drop for Waiting<'_, R, V> {
    fn drop(&mut self) {
        lock(&self.queue.pending).remove(&self.id);
        self.host
            .notify(self.queue.settled, serde_json::json!(self.id));
    }
}

/// Why a request nobody answered is refused.
fn unanswered() -> String {
    format!(
        "nobody answered in PlatypusDB within {} minutes",
        ANSWER_TIMEOUT.as_secs() / 60
    )
}

#[async_trait::async_trait]
impl AppLink for Link {
    async fn sessions(&self) -> Result<Vec<OpenSession>, String> {
        Ok(self.host.sessions())
    }

    async fn confirm(&self, request: WriteRequest) -> Result<Verdict, String> {
        let id = request.id.clone();
        let refused = Verdict::Deny {
            reason: Some(unanswered()),
        };
        self.writes
            .ask(&*self.host, (id, request), WRITE_REQUEST_EVENT, refused)
            .await
    }

    async fn context(&self, request: ContextRequest) -> Result<ContextVerdict, String> {
        let id = request.id.clone();
        let refused = ContextVerdict::Deny {
            reason: Some(unanswered()),
        };
        self.contexts
            .ask(&*self.host, (id, request), CONTEXT_REQUEST_EVENT, refused)
            .await
    }

    async fn hello(&self, session: &str, agent: &str) -> Result<(), String> {
        let new = lock(&self.agents)
            .insert(session.to_owned(), (agent.to_owned(), Instant::now()))
            .is_none_or(|(known, _)| known != agent);
        if new {
            self.agents_changed();
        }
        Ok(())
    }
}

/// How the bridge is set up, for Settings › Agents.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct BridgeStatus {
    pub enabled: bool,
    pub port: u16,
    /// The MCP endpoint, while the bridge runs.
    pub url: Option<String>,
    pub token: String,
    /// Why it does not run, e.g. the port is taken.
    pub error: Option<String>,
}

/// The bridge's server, while it runs.
struct Running {
    port: u16,
    stop: CancellationToken,
}

/// The bridge of the app: [`Bridge::start`] serves it as Settings say.
pub struct Bridge {
    link: Arc<Link>,
    data_dir: Option<PathBuf>,
    running: Mutex<Option<Running>>,
    /// Held while the bridge starts: two starts at once would both bind, or race.
    starting: tokio::sync::Mutex<()>,
    /// Why the last start failed.
    error: Mutex<Option<String>>,
}

impl Bridge {
    /// The bridge that writes its files in `data_dir`; without one (the app cannot find its
    /// folder) agents cannot find it and it does not start.
    pub fn new(link: Arc<Link>, data_dir: Option<PathBuf>) -> Self {
        Self {
            link,
            data_dir,
            running: Mutex::default(),
            starting: tokio::sync::Mutex::default(),
            error: Mutex::default(),
        }
    }

    pub fn link(&self) -> &Arc<Link> {
        &self.link
    }

    fn data_dir(&self) -> Result<&Path, String> {
        self.data_dir
            .as_deref()
            .ok_or_else(|| "the app's data folder is unknown".to_owned())
    }

    /// The token agents give, made the first time.
    pub fn token(&self) -> Result<String, String> {
        let dir = self.data_dir()?;
        let path = dir.join(TOKEN_FILE);
        match std::fs::read_to_string(&path) {
            Ok(token) if !token.trim().is_empty() => Ok(token.trim().to_owned()),
            _ => new_token(dir),
        }
    }

    /// Serves the bridge on `port` (when `enabled`) with `agent`'s tools at `/mcp`, after
    /// stopping the one running. A start that fails is kept for [`status`](Self::status).
    pub async fn start(
        &self,
        enabled: bool,
        port: u16,
        agent: Result<Arc<platypus_mcp::Agent>, String>,
    ) {
        let _starting = self.starting.lock().await;
        self.stop();
        // One left by a run that did not exit (a crash): agents would ask whoever holds its
        // port now.
        if let Some(dir) = &self.data_dir {
            let _ = std::fs::remove_file(BridgeFile::path(dir));
        }
        let started = match agent {
            Ok(agent) if enabled => self.serve(port, agent).await,
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        };
        *lock(&self.error) = started.err();
    }

    async fn serve(&self, port: u16, agent: Arc<platypus_mcp::Agent>) -> Result<(), String> {
        let dir = self.data_dir()?;
        let token = self.token()?;
        let listener = bind(port)
            .await
            .map_err(|e| format!("cannot listen on port {port}: {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        write_private(
            &BridgeFile::path(dir),
            &serde_json::to_string(&BridgeFile {
                port,
                token: token.clone(),
            })
            .map_err(|e| e.to_string())?,
        )?;
        let stop = CancellationToken::new();
        let mcp = platypus_mcp::http_service(agent, stop.child_token());
        tokio::spawn(http::serve(
            listener,
            Arc::clone(&self.link),
            token,
            mcp,
            stop.clone(),
        ));
        let link = Arc::clone(&self.link);
        let sweeping = stop.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = sweeping.cancelled() => break,
                    () = tokio::time::sleep(HELLO_EVERY / 2) => link.forget_silent(),
                }
            }
        });
        *lock(&self.running) = Some(Running { port, stop });
        Ok(())
    }

    /// Stops the server and removes its file: agents see the app closed.
    pub fn stop(&self) {
        if let Some(running) = lock(&self.running).take() {
            running.stop.cancel();
            if let Some(dir) = &self.data_dir {
                let _ = std::fs::remove_file(BridgeFile::path(dir));
            }
            lock(&self.link.agents).clear();
            self.link.agents_changed();
        }
    }

    /// Makes a new token: clients set up with the old one are refused.
    pub fn regenerate(&self) -> Result<(), String> {
        new_token(self.data_dir()?).map(drop)
    }

    pub fn status(&self, enabled: bool, port: u16) -> BridgeStatus {
        let running = lock(&self.running).as_ref().map(|r| r.port);
        let (token, token_error) = match self.token() {
            Ok(token) => (token, None),
            Err(e) => (String::new(), Some(e)),
        };
        BridgeStatus {
            enabled,
            port: running.unwrap_or(port),
            url: running.map(|port| format!("http://127.0.0.1:{port}/mcp")),
            token,
            error: lock(&self.error).clone().or(token_error),
        }
    }
}

/// Listens on `port`. The server stopped a moment ago may still hold it (its task ends
/// after the cancel), so a port in use is tried again for a short while.
async fn bind(port: u16) -> std::io::Result<tokio::net::TcpListener> {
    let mut tries = 0;
    loop {
        match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && port != 0 && tries < 20 => {
                tries += 1;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            other => return other,
        }
    }
}

/// Writes and returns a new random token in `dir`.
fn new_token(dir: &Path) -> Result<String, String> {
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    write_private(&dir.join(TOKEN_FILE), &token)?;
    Ok(token)
}

/// Writes `text` to `path`, readable only by the user: mode 0600 on Unix, an access list of
/// the user alone on Windows. The rights are set on every write, before the text goes in,
/// so a file left by an older version (or loosened by hand) is tightened too.
fn write_private(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let cannot = |e: std::io::Error| format!("cannot write {}: {e}", path.display());
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path).map_err(cannot)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(cannot)?;
    }
    #[cfg(windows)]
    acl::restrict_to_user(path)?;
    file.write_all(text.as_bytes()).map_err(cannot)
}

#[cfg(test)]
mod tests;
