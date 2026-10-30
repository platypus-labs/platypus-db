//! Opening a session and the state the statements of one connection share: the driver's
//! session, the keyspace in use and the cancel signal.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use platypus_core::{ConnectionConfig, Error, Result, ServerInfo};
use scylla::client::execution_profile::ExecutionProfile;
use scylla::client::session::Session;
use scylla::client::session_builder::SessionBuilder;
use scylla::cluster::metadata::Peer;
use scylla::errors::TranslationError;
use scylla::policies::address_translator::{AddressTranslator, UntranslatedPeer};
use scylla::policies::host_filter::HostFilter;
use tokio::sync::watch;
use uuid::Uuid;

use crate::error;

/// The port CQL listens on unless the connection says another.
pub const DEFAULT_PORT: u16 = 9042;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Which product the server is, from what `system.local` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Cassandra,
    Scylla,
}

impl Flavor {
    /// The name `ServerInfo::product` carries.
    #[must_use]
    pub const fn product(self) -> &'static str {
        match self {
            Self::Cassandra => "Cassandra",
            Self::Scylla => "ScyllaDB",
        }
    }
}

/// Every address the cluster tells of is the node the connection was opened to: the nodes
/// advertise their own addresses (a container's, a private network's), which the app often
/// cannot reach, and a tunnel reaches one node only. That node coordinates every statement.
struct ToContact(SocketAddr);

#[async_trait]
impl AddressTranslator for ToContact {
    async fn translate_address(
        &self,
        _peer: &UntranslatedPeer,
    ) -> std::result::Result<SocketAddr, TranslationError> {
        Ok(self.0)
    }
}

/// Keeps the first node the cluster lists and no other: all of them are reached at the same
/// address ([`ToContact`]), and one pool of connections is enough.
#[derive(Default)]
struct OneNode(Mutex<Option<Uuid>>);

impl HostFilter for OneNode {
    fn accept(&self, peer: &Peer) -> bool {
        let Ok(mut chosen) = self.0.lock() else {
            return true;
        };
        *chosen.get_or_insert(peer.host_id) == peer.host_id
    }
}

#[derive(Debug)]
pub struct Shared {
    pub session: Session,
    /// Bumped by the cancel handle; running statements watch it.
    pub cancel: watch::Sender<u64>,
    /// The last statement failed because the node could not be reached.
    pub closed: AtomicBool,
    /// The keyspace `USE` or the connection selected.
    pub keyspace: Mutex<Option<String>>,
    /// Warnings the server sent with the statements run since they were last taken.
    pub messages: Mutex<Vec<String>>,
}

/// Waits until the cancel handle is used after the subscription.
async fn cancelled(rx: &mut watch::Receiver<u64>) {
    if rx.changed().await.is_err() {
        std::future::pending::<()>().await;
    }
}

impl Shared {
    /// Runs `request` until the cancel handle is used. Dropping a request stops waiting for
    /// the node, which finishes the statement on its own.
    pub async fn guarded<T>(
        &self,
        cancel: &mut watch::Receiver<u64>,
        request: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        let result = tokio::select! {
            reply = request => reply,
            () = cancelled(cancel) => Err(Error::Cancelled),
        };
        match &result {
            Ok(_) => self.closed.store(false, Ordering::Relaxed),
            Err(e) if error::is_connection_loss(e) => self.closed.store(true, Ordering::Relaxed),
            Err(_) => {}
        }
        result
    }

    /// The keyspace in use.
    pub fn current_keyspace(&self) -> Option<String> {
        self.keyspace.lock().ok().and_then(|k| k.clone())
    }

    /// Notes what the server warned of, to show with the result.
    pub fn warn<'a>(&self, warnings: impl Iterator<Item = &'a str>) {
        if let Ok(mut messages) = self.messages.lock() {
            messages.extend(warnings.map(str::to_owned));
        }
    }
}

/// A contact point written `host`, `host:port`, `[v6]`, `[v6]:port` or as a bare IPv6 address,
/// as `host:port` (an IPv6 host in brackets), with `port` where it names none.
fn with_port(point: &str, port: u16) -> String {
    let bracketed = |host: &str| format!("[{host}]:{port}");
    if let Some(rest) = point.strip_prefix('[') {
        return match rest.split_once(']') {
            Some((host, tail)) if tail.starts_with(':') => format!("[{host}]{tail}"),
            Some((host, _)) => bracketed(host),
            None => point.to_owned(),
        };
    }
    match point.matches(':').count() {
        0 => format!("{point}:{port}"),
        1 => point.to_owned(),
        _ => bracketed(point),
    }
}

/// The `host:port` pairs of a connection's host field, separated by commas.
fn contact_points(config: &ConnectionConfig) -> Result<Vec<String>> {
    let port = config.port.unwrap_or(DEFAULT_PORT);
    let points: Vec<String> = config
        .host
        .split(',')
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(|h| with_port(h, port))
        .collect();
    if points.is_empty() {
        return Err(Error::Config(
            "the Cassandra connection has no host".to_owned(),
        ));
    }
    Ok(points)
}

/// Where the connection reaches its node: the local end of a tunnel, or the first contact point
/// resolved.
async fn contact_address(config: &ConnectionConfig, point: &str) -> Result<SocketAddr> {
    if let Some(addr) = config.connect_addr {
        return Ok(addr);
    }
    tokio::net::lookup_host(point)
        .await
        .map_err(|e| Error::Connect(format!("cannot resolve {point}: {e}")))?
        .next()
        .ok_or_else(|| Error::Connect(format!("cannot resolve {point}")))
}

/// Opens the session and reads what the server says of itself.
pub async fn open(config: &ConnectionConfig) -> Result<(Shared, ServerInfo)> {
    let points = contact_points(config)?;
    let mut builder = SessionBuilder::new()
        .connection_timeout(config.connect_timeout.unwrap_or(CONNECT_TIMEOUT))
        // The schema is read with queries of ours, when the navigator asks for it.
        .fetch_schema_metadata(false)
        // A statement runs as long as it takes, until it is cancelled.
        .default_execution_profile_handle(
            ExecutionProfile::builder()
                .request_timeout(None)
                .build()
                .into_handle(),
        );
    // One contact point, or a tunnel: that node alone (see `ToContact`).
    let single = config.connect_addr.is_some() || points.len() == 1;
    if single {
        let contact = contact_address(config, &points[0]).await?;
        builder = builder
            .known_node_addr(contact)
            .address_translator(Arc::new(ToContact(contact)))
            .host_filter(Arc::new(OneNode::default()));
    } else {
        builder = builder.known_nodes(&points);
    }
    if !config.user.is_empty() {
        let password = config.password.as_ref().map_or("", |p| p.expose());
        builder = builder.user(&config.user, password);
    }
    let encrypted = config.cql_tls();
    if encrypted {
        let tls = platypus_tls::client_config(config)?
            .ok_or_else(|| Error::Config("TLS is off".to_owned()))?;
        builder = builder.tls_context(Some(Arc::new(tls)));
    }
    let keyspace = config.database.trim();
    if !keyspace.is_empty() {
        builder = builder.use_keyspace(keyspace, keyspace != keyspace.to_lowercase());
    }
    let session = builder
        .build()
        .await
        .map_err(|e| error::connect_error(&e))?;
    let (flavor, version) = identify(&session).await?;
    let info = ServerInfo {
        product: flavor.product().to_owned(),
        version,
        encrypted,
    };
    if let Some(minimum) = info.below_minimum() {
        return Err(Error::Connect(format!(
            "{} {} is older than {minimum}, the oldest version supported",
            info.product, info.version
        )));
    }
    let shared = Shared {
        session,
        cancel: watch::channel(0).0,
        closed: AtomicBool::new(false),
        keyspace: Mutex::new((!keyspace.is_empty()).then(|| keyspace.to_owned())),
        messages: Mutex::new(Vec::new()),
    };
    Ok((shared, info))
}

/// The product and its version: `ScyllaDB` reports its own version in `system.versions` (its
/// `release_version` is the Cassandra version it is compatible with), Cassandra in
/// `system.local`.
async fn identify(session: &Session) -> Result<(Flavor, String)> {
    let scylla = session
        .query_unpaged(
            "SELECT version FROM system.versions WHERE key = 'local'",
            (),
        )
        .await
        .ok()
        .and_then(|r| r.into_rows_result().ok())
        .and_then(|rows| {
            rows.maybe_first_row::<(Option<String>,)>()
                .ok()
                .flatten()
                .and_then(|r| r.0)
        });
    if let Some(version) = scylla {
        return Ok((Flavor::Scylla, version));
    }
    let rows = session
        .query_unpaged("SELECT release_version FROM system.local", ())
        .await
        .map_err(|e| error::connect_error(&e))?
        .into_rows_result()
        .map_err(|e| error::connect_error(&e))?;
    let version = rows
        .maybe_first_row::<(Option<String>,)>()
        .map_err(|e| error::connect_error(&e))?
        .and_then(|r| r.0)
        .unwrap_or_default();
    Ok((Flavor::Cassandra, version))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(host: &str, port: Option<u16>) -> ConnectionConfig {
        let mut c = ConnectionConfig::new(platypus_core::DriverKind::Cassandra);
        c.host = host.to_owned();
        c.port = port;
        c
    }

    #[test]
    fn lists_contact_points_with_ports() {
        assert_eq!(
            contact_points(&config("a, b:9043 ,c", None)).unwrap(),
            ["a:9042", "b:9043", "c:9042"]
        );
        assert_eq!(
            contact_points(&config("::1", Some(1))).unwrap(),
            ["[::1]:1"]
        );
        assert_eq!(
            contact_points(&config("[::1]:7", None)).unwrap(),
            ["[::1]:7"]
        );
        assert_eq!(
            contact_points(&config("[::1]", Some(5))).unwrap(),
            ["[::1]:5"]
        );
        assert!(contact_points(&config(" , ", None)).is_err());
    }
}
