//! Opening connections: TCP, or TLS with the rustls configuration the other network drivers
//! share ([`platypus_tls`]), then the `redis` crate's multiplexed connection on that stream.
//! The session opens one per database it uses: `SELECT` on a shared connection would move
//! every other user of it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use platypus_core::{ConnectionConfig, Error, Result};
use redis::aio::MultiplexedConnection;
use redis::{AsyncConnectionConfig, ErrorKind, ProtocolVersion, RedisConnectionInfo, RedisError};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::ServerName;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// What opening a connection to the server takes.
pub struct Opener {
    host: String,
    port: u16,
    connect_addr: Option<std::net::SocketAddr>,
    tls: Option<(TlsConnector, ServerName<'static>)>,
    info: RedisConnectionInfo,
    timeout: Duration,
    /// The server refused RESP3 (`HELLO 3`): RESP2 from then on.
    resp2: AtomicBool,
}

impl std::fmt::Debug for Opener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Opener")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls", &self.tls.is_some())
            .finish_non_exhaustive()
    }
}

impl Opener {
    pub fn new(config: &ConnectionConfig) -> Result<Self> {
        if config.host.trim().is_empty() {
            return Err(Error::Config("the Redis connection has no host".to_owned()));
        }
        let tls = if config.redis_tls() {
            let tls = platypus_tls::client_config(config)?
                .ok_or_else(|| Error::Config("TLS is off".to_owned()))?;
            let host = config.host.trim_start_matches('[').trim_end_matches(']');
            let host = host.split('%').next().unwrap_or(host).to_owned();
            let name = ServerName::try_from(host)
                .map_err(|e| Error::Config(format!("invalid host for TLS: {e}")))?;
            Some((TlsConnector::from(Arc::new(tls)), name))
        } else {
            None
        };
        let mut info =
            RedisConnectionInfo::default().set_lib_name("platypusdb", platypus_core::VERSION);
        if !config.user.is_empty() {
            info = info.set_username(&config.user);
        }
        if let Some(password) = &config.password {
            info = info.set_password(password.expose());
        }
        Ok(Self {
            host: config.host.clone(),
            port: config.port.unwrap_or(6379),
            connect_addr: config.connect_addr,
            tls,
            info,
            timeout: config.connect_timeout.unwrap_or(CONNECT_TIMEOUT),
            resp2: AtomicBool::new(false),
        })
    }

    pub const fn encrypted(&self) -> bool {
        self.tls.is_some()
    }

    /// A new connection to database `db`, in RESP3 when the server speaks it.
    pub async fn open(&self, db: i64) -> std::result::Result<MultiplexedConnection, RedisError> {
        if !self.resp2.load(Ordering::Relaxed) {
            match self.open_as(db, ProtocolVersion::RESP3).await {
                Err(e) if refuses_resp3(&e) => self.resp2.store(true, Ordering::Relaxed),
                other => return other,
            }
        }
        self.open_as(db, ProtocolVersion::RESP2).await
    }

    async fn open_as(
        &self,
        db: i64,
        protocol: ProtocolVersion,
    ) -> std::result::Result<MultiplexedConnection, RedisError> {
        let info = self.info.clone().set_db(db).set_protocol(protocol);
        let tcp = tokio::time::timeout(self.timeout, async {
            if let Some(addr) = self.connect_addr {
                TcpStream::connect(addr).await
            } else {
                let host = self.host.trim_start_matches('[').trim_end_matches(']');
                TcpStream::connect((host, self.port)).await
            }
        })
        .await
        .map_err(|_| io_error(std::io::ErrorKind::TimedOut, "connection timed out"))??;
        let _ = tcp.set_nodelay(true);
        match &self.tls {
            Some((connector, name)) => {
                let stream =
                    tokio::time::timeout(self.timeout, connector.connect(name.clone(), tcp))
                        .await
                        .map_err(|_| {
                            io_error(std::io::ErrorKind::TimedOut, "TLS handshake timed out")
                        })??;
                self.handshake(&info, stream).await
            }
            None => self.handshake(&info, tcp).await,
        }
    }

    async fn handshake<S>(
        &self,
        info: &RedisConnectionInfo,
        stream: S,
    ) -> std::result::Result<MultiplexedConnection, RedisError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        // No response timeout: a command runs as long as it takes, until cancelled.
        let config = AsyncConnectionConfig::new().set_response_timeout(None);
        let (conn, driver) = tokio::time::timeout(
            self.timeout,
            MultiplexedConnection::new_with_config(info, stream, config),
        )
        .await
        .map_err(|_| io_error(std::io::ErrorKind::TimedOut, "the server did not answer"))??;
        tokio::spawn(driver);
        Ok(conn)
    }
}

fn io_error(kind: std::io::ErrorKind, message: &str) -> RedisError {
    RedisError::from(std::io::Error::new(kind, message.to_owned()))
}

/// The server does not take `HELLO 3` (Redis before 6, some compatible servers).
fn refuses_resp3(e: &RedisError) -> bool {
    e.kind() == ErrorKind::RESP3NotSupported
        || (e.code() == Some("ERR")
            && e.detail()
                .is_some_and(|d| d.to_ascii_lowercase().contains("hello")))
}
