//! A server that accepts the TCP connection and never answers the startup (a stalled proxy, a
//! firewall that drops packets after the handshake) ends the connect at `connect_timeout`
//! with a readable error instead of hanging for good. No database is needed.

use std::time::{Duration, Instant};

use platypus_core::ConnectionConfig;
use platypus_driver_pg::PgConnection;
use tokio::net::TcpListener;

#[tokio::test]
async fn a_server_that_never_answers_ends_the_connect_at_the_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    // Accepts and keeps the sockets open, without a byte back.
    let held = tokio::spawn(async move {
        let mut sockets = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            sockets.push(socket);
        }
    });
    let config = ConnectionConfig::from_url(&format!(
        "postgres://u:p@127.0.0.1:{port}/db?sslmode=disable&connect_timeout=1"
    ))
    .unwrap();

    let started = Instant::now();
    let error = match PgConnection::connect(&config).await {
        Ok(_) => panic!("connected to a server that never answers"),
        Err(e) => e.to_string(),
    };

    assert!(
        started.elapsed() < Duration::from_secs(5),
        "gave up after {:?}",
        started.elapsed()
    );
    assert!(error.contains("did not answer within 1 s"), "{error}");
    held.abort();
}
