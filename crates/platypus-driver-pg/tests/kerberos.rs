//! What the Kerberos login does before any ticket is involved, against a server that only speaks
//! the first messages of the protocol. No KDC and no Docker: the real login is in
//! `crates/platypus-conformance/tests/kerberos.rs`.

use std::time::Duration;

use platypus_core::{AUTH, ConnectionConfig, DriverKind, Secret, SslMode};
use platypus_driver_pg::PgConnection;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// `R` with the authentication request `code`: 0 is OK, 3 a password, 5 MD5, 7 GSS, 9 SSPI.
fn request(code: i32) -> Vec<u8> {
    let mut message = vec![b'R'];
    // MD5's salt.
    let extra: i32 = if code == 5 { 4 } else { 0 };
    message.extend_from_slice(&(8 + extra).to_be_bytes());
    message.extend_from_slice(&code.to_be_bytes());
    message.extend(std::iter::repeat_n(0, usize::try_from(extra).unwrap()));
    message
}

/// A server that answers the startup message with `answer`, then waits for the client to go,
/// and returns all the client sent after the startup message.
async fn server(answer: Vec<u8>) -> (u16, JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let length = socket.read_i32().await.unwrap();
        let mut startup = vec![0; usize::try_from(length).unwrap() - 4];
        socket.read_exact(&mut startup).await.unwrap();
        socket.write_all(&answer).await.unwrap();
        let mut rest = Vec::new();
        let _ = socket.read_to_end(&mut rest).await;
        rest
    });
    (port, handle)
}

fn config(port: u16, auth: Option<&str>) -> ConnectionConfig {
    let mut config = ConnectionConfig {
        host: "127.0.0.1".to_owned(),
        port: Some(port),
        database: "d".to_owned(),
        user: "alice".to_owned(),
        ssl_mode: SslMode::Disable,
        password: Some(Secret::new("do-not-send-me")),
        connect_timeout: Some(Duration::from_secs(60)),
        ..ConnectionConfig::new(DriverKind::Postgres)
    };
    if let Some(auth) = auth {
        config.options.insert(AUTH.to_owned(), auth.to_owned());
    }
    config
}

async fn refusal(config: &ConnectionConfig) -> String {
    match PgConnection::connect(config).await {
        Ok(_) => panic!("logged in"),
        Err(e) => e.to_string(),
    }
}

#[tokio::test]
async fn a_kerberos_connection_refuses_to_send_a_password() {
    for code in [3, 5] {
        let (port, sent) = server(request(code)).await;
        let error = refusal(&config(port, Some("kerberos"))).await;
        assert!(error.contains("none was sent"), "{error}");
        assert!(error.contains("pg_hba.conf"), "{error}");
        assert_eq!(sent.await.unwrap(), b"", "nothing goes after the startup");
    }
}

#[tokio::test]
async fn a_kerberos_connection_refuses_a_server_that_does_not_ask() {
    let (port, sent) = server(request(0)).await;
    let error = refusal(&config(port, Some("kerberos"))).await;
    assert!(error.contains("without asking"), "{error}");
    assert_eq!(sent.await.unwrap(), b"");
}

#[tokio::test]
async fn a_password_connection_does_not_start_a_kerberos_login() {
    for code in [7, 9] {
        let (port, sent) = server(request(code)).await;
        let error = refusal(&config(port, None)).await;
        assert!(error.contains("Choose Kerberos"), "{error}");
        assert_eq!(sent.await.unwrap(), b"");
    }
}

#[cfg(not(feature = "kerberos"))]
#[tokio::test]
async fn a_build_without_the_feature_says_so() {
    let (port, sent) = server(request(7)).await;
    let error = refusal(&config(port, Some("kerberos"))).await;
    assert!(error.contains("no Kerberos"), "{error}");
    assert!(error.contains("`kerberos` feature"), "{error}");
    assert_eq!(sent.await.unwrap(), b"");
}

/// The system's Kerberos really runs: on Windows SSPI, elsewhere GSSAPI. This machine has no
/// ticket for this made-up service, and the message says what to do whichever way it fails.
#[cfg(feature = "kerberos")]
#[tokio::test]
async fn without_a_ticket_the_message_says_what_to_do() {
    let (port, sent) = server(request(7)).await;
    let error = refusal(&config(port, Some("kerberos"))).await;
    println!("{error}");
    let expected: &[&str] = if cfg!(windows) {
        &["klist", "Windows said:"]
    } else {
        &["Kerberos said:"]
    };
    for what in expected {
        assert!(error.contains(what), "{what} in {error}");
    }
    assert!(!error.contains("do-not-send-me"), "{error}");
    assert_eq!(sent.await.unwrap(), b"", "no token without a ticket");
}
