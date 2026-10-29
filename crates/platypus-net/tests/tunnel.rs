//! SSH tunnels against an in-process SSH server that forwards to a local echo server:
//! password and key logins, a jump host, a server whose key changed, and a SOCKS5 proxy in
//! front of the SSH server or of the database.
// The test server's handlers keep the shape of russh's async trait.
#![allow(clippy::unused_async_trait_impl)]

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::TryStreamExt;
use platypus_core::{
    CancelHandle, Capabilities, CatalogColumn, CatalogForeignKey, Connection, ConnectionConfig,
    Driver, DriverKind, ExecOptions, ExecResult, MetadataMatch, ObjectKind, ObjectMeta, ObjectRef,
    Property, ReferencingKey, SchemaMeta, Secret, ServerInfo, SshAuth, SshConfig, TableDetails,
    TransactionState,
};
use platypus_net::Tunnel;
use russh::Channel;
use russh::keys::ssh_key::{self, LineEnding, PrivateKey};
use russh::server::{self, Auth, Msg, Server as _, Session};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const USER: &str = "platypus";
const PASSWORD: &str = "tunnel-secret";

fn random_key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), ssh_key::Algorithm::Ed25519).unwrap()
}

#[derive(Clone)]
struct TestServer {
    /// The client key accepted besides the password.
    client_key: ssh_key::PublicKey,
}

impl server::Server for TestServer {
    type Handler = Self;

    fn new_client(&mut self, _: Option<SocketAddr>) -> Self {
        self.clone()
    }
}

impl server::Handler for TestServer {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(if user == USER && password == PASSWORD {
            Auth::Accept
        } else {
            Auth::reject()
        })
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &ssh_key::PublicKey,
    ) -> Result<Auth, Self::Error> {
        Ok(
            if user == USER && key.key_data() == self.client_key.key_data() {
                Auth::Accept
            } else {
                Auth::reject()
            },
        )
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host: &str,
        port: u32,
        _: &str,
        _: u32,
        reply: server::ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        let Ok(mut socket) = TcpStream::connect((host, u16::try_from(port).unwrap())).await else {
            return Ok(());
        };
        reply.accept().await;
        tokio::spawn(async move {
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
        });
        Ok(())
    }
}

/// Starts an SSH server on a free port; returns its port.
async fn ssh_server(host_key: PrivateKey, client_key: ssh_key::PublicKey) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = Arc::new(server::Config {
        keys: vec![host_key],
        auth_rejection_time: std::time::Duration::from_millis(10),
        ..server::Config::default()
    });
    let mut server = TestServer { client_key };
    tokio::spawn(async move {
        let _ = server.run_on_socket(config, &listener).await;
    });
    port
}

/// A TCP server that sends back what it receives, standing in for a database.
async fn echo_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (mut read, mut write) = socket.split();
                let _ = tokio::io::copy(&mut read, &mut write).await;
            });
        }
    });
    port
}

fn server(port: u16, auth: SshAuth) -> SshConfig {
    SshConfig {
        host: "127.0.0.1".to_owned(),
        port,
        user: USER.to_owned(),
        auth,
        jump: None,
    }
}

fn password() -> SshAuth {
    SshAuth::Password(Secret::new(PASSWORD))
}

async fn round_trip(tunnel: &Tunnel) {
    let mut socket = TcpStream::connect(tunnel.local_addr()).await.unwrap();
    socket.write_all(b"SELECT 1").await.unwrap();
    let mut buf = [0; 8];
    socket.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"SELECT 1");
}

/// A SOCKS5 proxy without login that counts the connections it carries; returns its URL.
async fn socks5_proxy(carried: Arc<AtomicUsize>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let carried = Arc::clone(&carried);
            tokio::spawn(async move {
                let mut hello = [0u8; 2];
                socket.read_exact(&mut hello).await.unwrap();
                let mut methods = vec![0u8; usize::from(hello[1])];
                socket.read_exact(&mut methods).await.unwrap();
                socket.write_all(&[5, 0]).await.unwrap();
                let mut request = [0u8; 4];
                socket.read_exact(&mut request).await.unwrap();
                let host = match request[3] {
                    1 => {
                        let mut ip = [0u8; 4];
                        socket.read_exact(&mut ip).await.unwrap();
                        std::net::Ipv4Addr::from(ip).to_string()
                    }
                    3 => {
                        let mut name = vec![0u8; usize::from(socket.read_u8().await.unwrap())];
                        socket.read_exact(&mut name).await.unwrap();
                        String::from_utf8(name).unwrap()
                    }
                    other => panic!("address type {other}"),
                };
                let port = socket.read_u16().await.unwrap();
                let mut target = TcpStream::connect((host.as_str(), port)).await.unwrap();
                socket
                    .write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0])
                    .await
                    .unwrap();
                carried.fetch_add(1, Ordering::SeqCst);
                let _ = tokio::io::copy_bidirectional(&mut socket, &mut target).await;
            });
        }
    });
    format!("socks5://127.0.0.1:{port}")
}

async fn open(
    config: &SshConfig,
    target: u16,
    known_hosts: &Path,
) -> platypus_core::Result<Tunnel> {
    Tunnel::open(config, "127.0.0.1", target, Some(known_hosts)).await
}

#[tokio::test]
async fn forwards_with_a_password_or_a_key() {
    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");
    let client_key = random_key();
    let target = echo_server().await;
    let port = ssh_server(random_key(), client_key.public_key().clone()).await;

    let tunnel = open(&server(port, password()), target, &known_hosts)
        .await
        .unwrap();
    round_trip(&tunnel).await;
    // Two connections through one tunnel, as a pool would open.
    round_trip(&tunnel).await;
    // The first login learned the server's key.
    assert!(
        std::fs::read_to_string(&known_hosts)
            .unwrap()
            .contains(&format!("[127.0.0.1]:{port}"))
    );

    let key_file = dir.path().join("id_ed25519");
    std::fs::write(
        &key_file,
        client_key.to_openssh(LineEnding::LF).unwrap().as_bytes(),
    )
    .unwrap();
    let by_key = SshAuth::Key {
        path: key_file,
        passphrase: None,
    };
    let tunnel = open(&server(port, by_key), target, &known_hosts)
        .await
        .unwrap();
    round_trip(&tunnel).await;

    let wrong = SshAuth::Password(Secret::new("nope"));
    let err = open(&server(port, wrong), target, &known_hosts)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("authentication failed"), "{err}");
}

#[tokio::test]
async fn goes_through_a_jump_host() {
    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");
    let any = random_key().public_key().clone();
    let target = echo_server().await;
    let jump = ssh_server(random_key(), any.clone()).await;
    let inner = ssh_server(random_key(), any).await;

    let mut config = server(inner, password());
    config.jump = Some(Box::new(server(jump, password())));
    let tunnel = open(&config, target, &known_hosts).await.unwrap();
    round_trip(&tunnel).await;
}

#[tokio::test]
async fn refuses_a_changed_server_key() {
    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");
    let target = echo_server().await;
    let port = ssh_server(random_key(), random_key().public_key().clone()).await;
    // Another key was recorded for this server before.
    russh::keys::known_hosts::learn_known_hosts_path(
        "127.0.0.1",
        port,
        random_key().public_key(),
        &known_hosts,
    )
    .unwrap();

    let err = open(&server(port, password()), target, &known_hosts)
        .await
        .unwrap_err();
    assert!(err.to_string().to_lowercase().contains("changed"), "{err}");
}

#[tokio::test]
async fn reaches_the_ssh_server_through_a_proxy() {
    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");
    let any = random_key().public_key().clone();
    let target = echo_server().await;
    let jump = ssh_server(random_key(), any.clone()).await;
    let inner = ssh_server(random_key(), any).await;
    let carried = Arc::new(AtomicUsize::new(0));
    let proxy = platypus_net::Proxy::parse(&socks5_proxy(Arc::clone(&carried)).await).unwrap();

    let tunnel = Tunnel::open_via(
        &server(inner, password()),
        "127.0.0.1",
        target,
        Some(&known_hosts),
        Some(&proxy),
    )
    .await
    .unwrap();
    round_trip(&tunnel).await;
    round_trip(&tunnel).await;
    assert_eq!(carried.load(Ordering::SeqCst), 1, "one SSH session");

    // With a jump host, only the first hop goes through the proxy.
    let mut config = server(inner, password());
    config.jump = Some(Box::new(server(jump, password())));
    let tunnel = Tunnel::open_via(
        &config,
        "127.0.0.1",
        target,
        Some(&known_hosts),
        Some(&proxy),
    )
    .await
    .unwrap();
    round_trip(&tunnel).await;
    assert_eq!(carried.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn forwards_through_a_proxy_alone() {
    let target = echo_server().await;
    let carried = Arc::new(AtomicUsize::new(0));
    let mut config = ConnectionConfig::new(DriverKind::Postgres);
    config.host = "127.0.0.1".to_owned();
    config.port = Some(target);
    config.options.insert(
        platypus_net::proxy::PROXY.to_owned(),
        socks5_proxy(Arc::clone(&carried)).await,
    );
    let tunnel = Tunnel::of(&config, None).await.unwrap().unwrap();
    round_trip(&tunnel).await;
    round_trip(&tunnel).await;
    // The stream of the check made on opening carried the first.
    assert_eq!(carried.load(Ordering::SeqCst), 2);

    config.options.clear();
    assert!(
        Tunnel::of(&config, None).await.unwrap().is_none(),
        "no path needed"
    );
}

/// The servers of the conformance tests, reached through the tunnel or a proxy instead of
/// directly: the driver connects to the local port and still names the server for TLS.
#[tokio::test]
async fn connects_databases_through_the_tunnel() {
    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");
    let port = ssh_server(random_key(), random_key().public_key().clone()).await;
    let carried = Arc::new(AtomicUsize::new(0));
    let proxy = socks5_proxy(Arc::clone(&carried)).await;
    for (env, through_proxy) in [
        ("PLATYPUS_TEST_PG_URL", false),
        ("PLATYPUS_TEST_MYSQL_URL", false),
        ("PLATYPUS_TEST_PG_URL", true),
        ("PLATYPUS_TEST_MYSQL_URL", true),
    ] {
        let Ok(url) = std::env::var(env) else {
            eprintln!("skipping {env}: not set");
            continue;
        };
        let mut config = ConnectionConfig::from_url(&url).unwrap();
        if through_proxy {
            config
                .options
                .insert(platypus_net::proxy::PROXY.to_owned(), proxy.clone());
        } else {
            config.ssh = Some(server(port, password()));
        }
        let before = carried.load(Ordering::SeqCst);
        let conn = platypus_net::connect(&config, Some(&known_hosts), |c| async move {
            match c.driver {
                DriverKind::Postgres => platypus_driver_pg::PgDriver.connect(&c).await,
                _ => platypus_driver_mysql::MySqlDriver.connect(&c).await,
            }
        })
        .await
        .unwrap_or_else(|e| panic!("{env}: {e}"));
        let ExecResult::Rows(stream) = conn
            .execute("SELECT 41 + 1", &ExecOptions::default())
            .await
            .unwrap()
        else {
            panic!("{env}: no rows");
        };
        let batches: Vec<_> = stream.try_collect().await.unwrap();
        assert_eq!(
            batches
                .iter()
                .map(platypus_core::arrow_array::RecordBatch::num_rows)
                .sum::<usize>(),
            1,
            "{env}"
        );
        assert!(!conn.is_closed(), "{env}");
        if through_proxy {
            assert!(carried.load(Ordering::SeqCst) > before, "{env}: proxied");
        }
    }
}

/// A connection whose optional methods answer differently from the trait's defaults.
#[derive(Debug)]
struct Optional;

#[async_trait::async_trait]
impl Connection for Optional {
    async fn execute(&self, _: &str, _: &ExecOptions) -> platypus_core::Result<ExecResult> {
        unimplemented!()
    }
    fn cancel_handle(&self) -> CancelHandle {
        unimplemented!()
    }
    fn server_info(&self) -> &ServerInfo {
        unimplemented!()
    }
    fn is_closed(&self) -> bool {
        false
    }
    async fn schemas(&self) -> platypus_core::Result<Vec<SchemaMeta>> {
        Ok(Vec::new())
    }
    async fn objects(&self, _: &str) -> platypus_core::Result<Vec<ObjectMeta>> {
        Ok(Vec::new())
    }
    async fn table_details(&self, _: &str, _: &str) -> platypus_core::Result<TableDetails> {
        unimplemented!()
    }
    async fn referencing_keys(
        &self,
        _: &str,
        _: &str,
    ) -> platypus_core::Result<Vec<ReferencingKey>> {
        Ok(Vec::new())
    }
    async fn catalog_columns(&self) -> platypus_core::Result<Vec<CatalogColumn>> {
        Ok(Vec::new())
    }
    async fn catalog_foreign_keys(&self) -> platypus_core::Result<Vec<CatalogForeignKey>> {
        Ok(vec![CatalogForeignKey {
            schema: "s".to_owned(),
            table: "orders".to_owned(),
            columns: vec!["customer_id".to_owned()],
            ref_schema: "s".to_owned(),
            ref_table: "customers".to_owned(),
            ref_columns: vec!["id".to_owned()],
        }])
    }
    fn transaction(&self) -> TransactionState {
        unimplemented!()
    }
    async fn set_autocommit(&self, _: bool) -> platypus_core::Result<()> {
        Ok(())
    }
    async fn commit(&self) -> platypus_core::Result<()> {
        Ok(())
    }
    async fn rollback(&self) -> platypus_core::Result<()> {
        Ok(())
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    async fn object_properties(&self, _: &ObjectRef<'_>) -> platypus_core::Result<Vec<Property>> {
        Ok(vec![Property::new("owner", "me")])
    }
    async fn databases(&self) -> platypus_core::Result<Vec<String>> {
        Ok(vec!["other".to_owned()])
    }
    async fn search_metadata(&self, _: &str) -> platypus_core::Result<Vec<MetadataMatch>> {
        Ok(Vec::new())
    }
}

/// Every optional method reaches the driver's connection through the tunnel, not the
/// trait's default (empty, or unsupported).
#[tokio::test]
async fn forwards_optional_methods() {
    let dir = tempfile::tempdir().unwrap();
    let known_hosts = dir.path().join("known_hosts");
    let port = ssh_server(random_key(), random_key().public_key().clone()).await;
    let mut config = ConnectionConfig::new(DriverKind::Postgres);
    config.host = "127.0.0.1".to_owned();
    config.port = Some(echo_server().await);
    config.ssh = Some(server(port, password()));
    let conn = platypus_net::connect(&config, Some(&known_hosts), |_| async {
        Ok(Box::new(Optional) as Box<dyn Connection>)
    })
    .await
    .unwrap();
    let object = ObjectRef {
        schema: "s",
        name: "orders",
        kind: ObjectKind::Table,
        parent: None,
    };
    assert_eq!(conn.catalog_foreign_keys().await.unwrap().len(), 1);
    assert_eq!(conn.object_properties(&object).await.unwrap().len(), 1);
    assert_eq!(conn.databases().await.unwrap(), ["other"]);
    assert!(conn.search_metadata("orders").await.is_ok());
}
