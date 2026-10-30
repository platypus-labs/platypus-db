//! Network paths to database servers. An SSH tunnel listens on a local port and forwards
//! every connection to the database through the SSH server (`ssh -L`), optionally reached
//! through a jump host (`ssh -J`), and the first SSH server, or the database server when
//! there is no tunnel, may be reached through a SOCKS5 or HTTP proxy (see [`proxy`]).
//! [`connect`] runs the connection's command before
//! connecting (see [`hooks`]), opens the tunnel, then the driver's connection through it,
//! and keeps the tunnel alive as long as the connection; its command after disconnecting
//! runs when the connection goes. A server may also be reached through a port a cloud's CLI
//! forwards (`kubectl`, AWS SSM: see [`forward`]), and signed in to with a token its CLI
//! makes (IAM, Entra ID: see [`token`]).

mod cli;
mod forward;
pub mod hooks;
pub mod proxy;
mod token;

use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use platypus_core::{
    CancelHandle, Capabilities, CatalogColumn, CatalogForeignKey, Connection, ConnectionConfig,
    Error, ExecOptions, ExecResult, Explain, FORWARD, MetadataMatch, ObjectMeta, ObjectRef,
    Property, ReferencingKey, Result, SchemaMeta, ServerInfo, SshAuth, SshConfig, TableDetails,
    TransactionState,
};
use russh::client::{self, AuthResult, Handle};
use russh::keys::{self, PrivateKeyWithHashAlg, PublicKeyOrCertificate};

pub use proxy::Proxy;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

fn ssh_error(server: &SshConfig, e: impl fmt::Display) -> Error {
    Error::Connect(format!(
        "SSH {}@{}:{}: {e}",
        server.user, server.host, server.port
    ))
}

/// Checks the server's key against `~/.ssh/known_hosts`: a known key must match, an
/// unknown one is learned (trust on first use, as `ssh` with `accept-new`).
struct KnownHosts {
    host: String,
    port: u16,
    path: PathBuf,
}

impl KnownHosts {
    fn check(&self, key: &PublicKeyOrCertificate) -> Result<bool, russh::Error> {
        // Host certificates are checked by the key they certify.
        let key = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(cert) => cert.public_key().clone().into(),
        };
        match keys::check_known_hosts_path(&self.host, self.port, &key, &self.path) {
            Ok(true) => Ok(true),
            Ok(false) => {
                // Without a writable known_hosts the key is still accepted for this time.
                let _ = keys::known_hosts::learn_known_hosts_path(
                    &self.host, self.port, &key, &self.path,
                );
                Ok(true)
            }
            Err(e) => Err(russh::Error::Keys(e)),
        }
    }
}

impl client::Handler for KnownHosts {
    type Error = russh::Error;

    fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        let checked = self.check(key);
        async move { checked }
    }
}

type Session = Handle<KnownHosts>;

async fn authenticate(session: &mut Session, server: &SshConfig) -> Result<()> {
    let fail = |e: russh::Error| ssh_error(server, e);
    let result = match &server.auth {
        SshAuth::Password(password) => session
            .authenticate_password(&server.user, password.expose())
            .await
            .map_err(fail)?,
        SshAuth::Key { path, passphrase } => {
            let key =
                keys::load_secret_key(path, passphrase.as_ref().map(platypus_core::Secret::expose))
                    .map_err(|e| {
                        ssh_error(
                            server,
                            format!("cannot read the key {}: {e}", path.display()),
                        )
                    })?;
            let hash = session
                .best_supported_rsa_hash()
                .await
                .map_err(fail)?
                .flatten();
            session
                .authenticate_publickey(
                    &server.user,
                    PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                )
                .await
                .map_err(fail)?
        }
        SshAuth::Agent => agent_auth(session, server).await?,
    };
    match result {
        AuthResult::Success => Ok(()),
        AuthResult::Failure { .. } => Err(ssh_error(server, "authentication failed")),
    }
}

/// Tries the agent's keys in turn.
async fn agent_auth(session: &mut Session, server: &SshConfig) -> Result<AuthResult> {
    #[cfg(windows)]
    let mut agent =
        match keys::agent::client::AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent")
            .await
        {
            Ok(agent) => agent.dynamic(),
            Err(_) => keys::agent::client::AgentClient::connect_pageant()
                .await
                .map_err(|e| ssh_error(server, format!("no SSH agent is running: {e}")))?
                .dynamic(),
        };
    #[cfg(unix)]
    let mut agent = keys::agent::client::AgentClient::connect_env()
        .await
        .map_err(|e| ssh_error(server, format!("no SSH agent is running: {e}")))?
        .dynamic();
    let identities = agent
        .request_identities()
        .await
        .map_err(|e| ssh_error(server, format!("the SSH agent did not list its keys: {e}")))?;
    let hash = session
        .best_supported_rsa_hash()
        .await
        .map_err(|e| ssh_error(server, e))?
        .flatten();
    for identity in identities {
        let key = identity.public_key().into_owned();
        let result = session
            .authenticate_publickey_with(&server.user, key, hash, &mut agent)
            .await
            .map_err(|e| ssh_error(server, e))?;
        if result.success() {
            return Ok(result);
        }
    }
    Ok(AuthResult::Failure {
        remaining_methods: russh::MethodSet::empty(),
        partial_success: false,
    })
}

/// How long reaching an SSH server and its handshake may take, as the databases' own
/// connect timeout.
const SSH_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn client_config() -> Arc<client::Config> {
    Arc::new(client::Config {
        keepalive_interval: Some(std::time::Duration::from_secs(30)),
        inactivity_timeout: None,
        nodelay: true,
        ..client::Config::default()
    })
}

/// Logs in to `server`, through its jump host if it has one, and the first of them through
/// `proxy`. Jump hosts stay open as long as the sessions they carry.
async fn open_session(
    server: &SshConfig,
    known_hosts: &Path,
    proxy: Option<&Proxy>,
) -> Result<(Session, Vec<Session>)> {
    let handler = KnownHosts {
        host: server.host.clone(),
        port: server.port,
        path: known_hosts.to_owned(),
    };
    let (mut session, mut jumps) = match &server.jump {
        None => {
            let connecting = async {
                let session = match proxy {
                    None => {
                        client::connect(
                            client_config(),
                            (server.host.as_str(), server.port),
                            handler,
                        )
                        .await
                    }
                    Some(proxy) => {
                        let stream = proxy.dial(&server.host, server.port).await?;
                        client::connect_stream(client_config(), stream, handler).await
                    }
                };
                session.map_err(|e| ssh_error(server, e))
            };
            // An address that drops packets would wait for the system's TCP timeout.
            let session = tokio::time::timeout(SSH_CONNECT_TIMEOUT, connecting)
                .await
                .map_err(|_| {
                    ssh_error(
                        server,
                        format!("no answer within {} s", SSH_CONNECT_TIMEOUT.as_secs()),
                    )
                })??;
            (session, Vec::new())
        }
        Some(jump) => {
            let (via, mut chain) = Box::pin(open_session(jump, known_hosts, proxy)).await?;
            let channel = via
                .channel_open_direct_tcpip(
                    server.host.clone(),
                    u32::from(server.port),
                    "127.0.0.1",
                    0,
                )
                .await
                .map_err(|e| {
                    ssh_error(
                        jump,
                        format!("cannot reach {}:{}: {e}", server.host, server.port),
                    )
                })?;
            let session = client::connect_stream(client_config(), channel.into_stream(), handler)
                .await
                .map_err(|e| ssh_error(server, e))?;
            chain.push(via);
            (session, chain)
        }
    };
    authenticate(&mut session, server).await?;
    jumps.shrink_to_fit();
    Ok((session, jumps))
}

/// Listens on a free local port and hands each connection to `forward`, until the returned
/// task is aborted. `what` names the path in errors.
async fn listen<F, Fut>(what: &str, forward: F) -> Result<(SocketAddr, JoinHandle<()>)>
where
    F: Fn(TcpStream, SocketAddr) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| Error::Connect(format!("cannot open a local port for the {what}: {e}")))?;
    let local = listener
        .local_addr()
        .map_err(|e| Error::Connect(format!("{what}: {e}")))?;
    let accept = tokio::spawn(async move {
        while let Ok((socket, peer)) = listener.accept().await {
            tokio::spawn(forward(socket, peer));
        }
    });
    Ok((local, accept))
}

/// A local port forwarded to a host and port seen from the SSH server, or reached through a
/// proxy. Closing it (dropping) stops listening and ends the SSH sessions.
pub struct Tunnel {
    local: SocketAddr,
    session: Option<Arc<Session>>,
    accept: JoinHandle<()>,
    _jumps: Vec<Session>,
}

impl fmt::Debug for Tunnel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tunnel")
            .field("local", &self.local)
            .finish_non_exhaustive()
    }
}

/// The user's `~/.ssh/known_hosts`, shared with OpenSSH.
fn default_known_hosts() -> Result<PathBuf> {
    std::env::home_dir()
        .map(|home| home.join(".ssh").join("known_hosts"))
        .ok_or_else(|| Error::Connect("SSH: no home directory for known_hosts".to_owned()))
}

impl Tunnel {
    /// Opens a tunnel through `server` to `host:port`, on a free local port. Server keys are
    /// checked against `known_hosts`, by default the user's `~/.ssh/known_hosts`.
    pub async fn open(
        server: &SshConfig,
        host: &str,
        port: u16,
        known_hosts: Option<&Path>,
    ) -> Result<Self> {
        Self::open_via(server, host, port, known_hosts, None).await
    }

    /// As [`Tunnel::open`], with the first SSH server reached through `proxy`.
    pub async fn open_via(
        server: &SshConfig,
        host: &str,
        port: u16,
        known_hosts: Option<&Path>,
        proxy: Option<&Proxy>,
    ) -> Result<Self> {
        let path = match known_hosts {
            Some(path) => path.to_owned(),
            None => default_known_hosts()?,
        };
        let (session, jumps) = open_session(server, &path, proxy).await?;
        let session = Arc::new(session);
        let forward = Arc::clone(&session);
        let target = host.to_owned();
        let (local, accept) = listen("SSH tunnel", move |mut socket, peer| {
            let session = Arc::clone(&forward);
            let target = target.clone();
            async move {
                let Ok(channel) = session
                    .channel_open_direct_tcpip(
                        target,
                        u32::from(port),
                        peer.ip().to_string(),
                        u32::from(peer.port()),
                    )
                    .await
                else {
                    return;
                };
                let mut stream = channel.into_stream();
                let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
            }
        })
        .await?;
        Ok(Self {
            local,
            session: Some(session),
            accept,
            _jumps: jumps,
        })
    }

    /// A local port forwarded to `host:port` through `proxy`. The proxy is tried here, so
    /// that a wrong address or login is said at once rather than as a lost connection, and
    /// the stream it gives carries the first connection to the port.
    pub async fn through_proxy(proxy: &Proxy, host: &str, port: u16) -> Result<Self> {
        let checked = Arc::new(std::sync::Mutex::new(Some(proxy.dial(host, port).await?)));
        let proxy = proxy.clone();
        let target = host.to_owned();
        let (local, accept) = listen("proxy", move |mut socket, _| {
            let ready = checked
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            let proxy = proxy.clone();
            let target = target.clone();
            async move {
                let stream = match ready {
                    Some(stream) => Ok(stream),
                    None => proxy.dial(&target, port).await,
                };
                if let Ok(mut stream) = stream {
                    let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
                }
            }
        })
        .await?;
        Ok(Self {
            local,
            session: None,
            accept,
            _jumps: Vec::new(),
        })
    }

    /// The path to `config`'s server, if it needs one: its SSH tunnel, its proxy, or both;
    /// or the port its cloud's CLI forwards, which goes alone.
    pub async fn of(config: &ConnectionConfig, known_hosts: Option<&Path>) -> Result<Option<Self>> {
        let proxy = Proxy::of(config)?;
        if config.options.get(FORWARD).is_some_and(|f| !f.is_empty()) {
            if config.ssh.is_some() || proxy.is_some() {
                return Err(Error::Config(
                    "a port forward goes alone: without an SSH tunnel or a proxy".to_owned(),
                ));
            }
            return Ok(forward::open(config).await?.map(|(local, running)| Self {
                local,
                session: None,
                accept: running,
                _jumps: Vec::new(),
            }));
        }
        let port = || {
            config
                .port
                .or(config.driver.default_port())
                .ok_or_else(|| Error::Config("a tunnel or proxy needs a server port".to_owned()))
        };
        let tunnel = match (&config.ssh, &proxy) {
            (Some(ssh), proxy) => {
                Self::open_via(ssh, &config.host, port()?, known_hosts, proxy.as_ref()).await?
            }
            (None, Some(proxy)) => Self::through_proxy(proxy, &config.host, port()?).await?,
            (None, None) => return Ok(None),
        };
        Ok(Some(tunnel))
    }

    /// The local address that leads to the target.
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.is_closed()) || self.accept.is_finished()
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

/// Opens `driver`'s connection for `config`, through its SSH tunnel or proxy when it has one; see
/// [`Tunnel::open`] for `known_hosts`. The command before connecting runs first, and the
/// one after disconnecting when the connection goes, or at once when it could not be made.
pub async fn connect<F, Fut>(
    config: &ConnectionConfig,
    known_hosts: Option<&Path>,
    driver: F,
) -> Result<Box<dyn Connection>>
where
    F: FnOnce(ConnectionConfig) -> Fut,
    Fut: Future<Output = Result<Box<dyn Connection>>>,
{
    let mut config = config.clone();
    // Read before the commands run: a wrong proxy is said without running them.
    Proxy::of(&config)?;
    // On a failure below, the use goes, and the command after runs if it was the last.
    let hooks = hooks::acquire(&mut config).await?;
    sign_in(&mut config).await?;
    let (inner, tunnel) = connect_direct(config, known_hosts, driver).await?;
    if tunnel.is_none() && hooks.is_none() {
        return Ok(inner);
    }
    Ok(Box::new(Held {
        inner,
        tunnel,
        _hooks: hooks,
    }))
}

/// Puts in `config` the token its cloud's CLI makes when it signs in with one (IAM, Entra
/// ID), as the password: for a client of the database's own, which [`connect`] does not open.
pub async fn sign_in(config: &mut ConnectionConfig) -> Result<()> {
    let auth = config.auth()?;
    if !auth.fits(config.driver) {
        return Err(Error::Config(format!(
            "{} does not sign in to {}: choose another way in the connection's settings",
            auth.name(),
            config.driver.name()
        )));
    }
    if let Some(token) = token::fetch(config).await? {
        config.password = Some(token);
    }
    Ok(())
}

/// The driver's connection, through the SSH tunnel or proxy when there is one.
async fn connect_direct<F, Fut>(
    mut config: ConnectionConfig,
    known_hosts: Option<&Path>,
    driver: F,
) -> Result<(Box<dyn Connection>, Option<Tunnel>)>
where
    F: FnOnce(ConnectionConfig) -> Fut,
    Fut: Future<Output = Result<Box<dyn Connection>>>,
{
    let tunnel = Tunnel::of(&config, known_hosts).await?;
    // The driver never sees the path.
    config.options.remove(proxy::PROXY);
    let Some(tunnel) = tunnel else {
        return Ok((driver(config).await?, None));
    };
    config.ssh = None;
    config.connect_addr = Some(tunnel.local_addr());
    Ok((driver(config).await?, Some(tunnel)))
}

/// A connection with what lives as long as it: its SSH tunnel or proxy, and its use of its
/// commands, whose command after disconnecting runs when the last use goes. Dropped in this
/// order: the connection closes before the command after runs.
#[derive(Debug)]
struct Held {
    inner: Box<dyn Connection>,
    tunnel: Option<Tunnel>,
    _hooks: Option<hooks::Use>,
}

impl Held {
    fn tunnel_closed(&self) -> bool {
        self.tunnel.as_ref().is_some_and(Tunnel::is_closed)
    }
}

#[async_trait]
impl Connection for Held {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        self.inner.execute(sql, options).await
    }

    async fn execute_all(&self, statements: &[&str], expected: &[u64]) -> platypus_core::Executed {
        self.inner.execute_all(statements, expected).await
    }

    fn cancel_handle(&self) -> CancelHandle {
        self.inner.cancel_handle()
    }

    fn server_info(&self) -> &ServerInfo {
        self.inner.server_info()
    }

    fn is_closed(&self) -> bool {
        self.inner.is_closed() || self.tunnel_closed()
    }

    async fn check_alive(&self) -> bool {
        !self.tunnel_closed() && self.inner.check_alive().await
    }

    fn take_messages(&self) -> Vec<String> {
        self.inner.take_messages()
    }

    async fn schemas(&self) -> Result<Vec<SchemaMeta>> {
        self.inner.schemas().await
    }

    async fn objects(&self, schema: &str) -> Result<Vec<ObjectMeta>> {
        self.inner.objects(schema).await
    }

    async fn table_details(&self, schema: &str, table: &str) -> Result<TableDetails> {
        self.inner.table_details(schema, table).await
    }

    async fn referencing_keys(&self, schema: &str, table: &str) -> Result<Vec<ReferencingKey>> {
        self.inner.referencing_keys(schema, table).await
    }

    async fn catalog_columns(&self) -> Result<Vec<CatalogColumn>> {
        self.inner.catalog_columns().await
    }

    fn transaction(&self) -> TransactionState {
        self.inner.transaction()
    }

    async fn set_autocommit(&self, on: bool) -> Result<()> {
        self.inner.set_autocommit(on).await
    }

    async fn commit(&self) -> Result<()> {
        self.inner.commit().await
    }

    async fn rollback(&self) -> Result<()> {
        self.inner.rollback().await
    }

    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }

    async fn explain(&self, sql: &str, analyze: bool) -> Result<Explain> {
        self.inner.explain(sql, analyze).await
    }

    async fn object_ddl(&self, object: &ObjectRef<'_>) -> Result<String> {
        self.inner.object_ddl(object).await
    }

    async fn objects_ddl(&self, objects: &[ObjectRef<'_>]) -> Vec<Result<String>> {
        self.inner.objects_ddl(objects).await
    }

    async fn catalog_foreign_keys(&self) -> Result<Vec<CatalogForeignKey>> {
        self.inner.catalog_foreign_keys().await
    }

    async fn object_properties(&self, object: &ObjectRef<'_>) -> Result<Vec<Property>> {
        self.inner.object_properties(object).await
    }

    async fn databases(&self) -> Result<Vec<String>> {
        self.inner.databases().await
    }

    async fn search_metadata(&self, needle: &str) -> Result<Vec<MetadataMatch>> {
        self.inner.search_metadata(needle).await
    }

    async fn server_sessions(&self) -> Result<Vec<platypus_core::ServerSession>> {
        self.inner.server_sessions().await
    }

    async fn server_locks(&self) -> Result<Vec<platypus_core::ServerLock>> {
        self.inner.server_locks().await
    }

    async fn end_server_session(&self, id: &str, how: platypus_core::EndSession) -> Result<()> {
        self.inner.end_server_session(id, how).await
    }

    async fn top_queries(&self) -> Result<Vec<platypus_core::ServerQuery>> {
        self.inner.top_queries().await
    }

    async fn object_statistics(&self) -> Result<Vec<platypus_core::ObjectStatistics>> {
        self.inner.object_statistics().await
    }

    async fn server_parameters(&self) -> Result<Vec<platypus_core::ServerParameter>> {
        self.inner.server_parameters().await
    }

    async fn server_metrics(&self) -> Result<platypus_core::ServerMetrics> {
        self.inner.server_metrics().await
    }

    async fn maintain(
        &self,
        schema: &str,
        table: &str,
        operation: platypus_core::Maintenance,
    ) -> Result<Vec<String>> {
        self.inner.maintain(schema, table, operation).await
    }

    async fn bulk_load(&self, rows: &platypus_core::BulkRows<'_>) -> Result<u64> {
        self.inner.bulk_load(rows).await
    }
}
