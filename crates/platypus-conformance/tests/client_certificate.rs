//! Login with a client certificate on PostgreSQL and MySQL, over mutual TLS. The servers are
//! the `postgres-mtls` and `mysql-mtls` services of `docker/compose.yml` (profile `tls`), which
//! accept only a certificate signed by the test CA in `docker/tls`; an unreachable one is
//! skipped unless `PLATYPUS_TEST_PG_MTLS_URL` / `PLATYPUS_TEST_MYSQL_MTLS_URL` is set.

use std::path::PathBuf;
use std::time::Duration;

use arrow_cast::display::array_value_to_string;
use futures::TryStreamExt;
use platypus_core::{
    Connection, ConnectionConfig, Driver, DriverKind, Error, ExecOptions, ExecResult, SslMode,
};
use platypus_driver_mysql::MySqlDriver;
use platypus_driver_pg::PgDriver;

fn tls_file(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docker/tls")
        .join(name)
}

fn driver(kind: DriverKind) -> &'static dyn Driver {
    match kind {
        DriverKind::Postgres => &PgDriver,
        DriverKind::MySql => &MySqlDriver,
        other => unreachable!("{other:?}"),
    }
}

/// The server's config with the CA to trust and, if `identity`, the test client certificate.
fn config(url: &str, identity: bool) -> ConnectionConfig {
    let mut config = ConnectionConfig::from_url(url).unwrap();
    config.ssl_mode = SslMode::VerifyFull;
    config.ssl_root_cert = Some(tls_file("ca.crt"));
    if identity {
        config.ssl_client_cert = Some(tls_file("client.crt"));
        config.ssl_client_key = Some(tls_file("client.key"));
    }
    config.connect_timeout = Some(Duration::from_secs(5));
    config
}

/// The URL of the server, or `None` (skipped) when it is not up and not required.
async fn server(env: &str, default: &str, name: &str) -> Option<String> {
    let (url, required) = match std::env::var(env) {
        Ok(url) => (url, true),
        Err(_) => (default.to_owned(), false),
    };
    let probe = config(&url, true);
    match driver(probe.driver).connect(&probe).await {
        Ok(_) => Some(url),
        Err(e) if !required => {
            eprintln!(
                "skipping {name}: not reachable (or not trusting the test certificate) at {url}: {e}"
            );
            None
        }
        Err(e) => panic!("cannot connect to {env}: {e}"),
    }
}

async fn first_row(conn: &dyn Connection, sql: &str) -> Vec<String> {
    let ExecResult::Rows(stream) = conn.execute(sql, &ExecOptions::default()).await.unwrap() else {
        panic!("{sql}: rows expected");
    };
    let batches: Vec<_> = stream.try_collect().await.unwrap();
    let batch = batches.first().expect("a row");
    batch
        .columns()
        .iter()
        .map(|c| array_value_to_string(c, 0).unwrap())
        .collect()
}

/// A connect that must fail, the error as text.
async fn refused(config: &ConnectionConfig) -> String {
    match driver(config.driver).connect(config).await {
        Ok(_) => panic!("the server accepted the login"),
        Err(e) => e.to_string(),
    }
}

async fn check_server(url: &str, (user_sql, tls_sql, tls_expected): (&str, &str, &str)) {
    // With the certificate the login works, and the server sees the certificate.
    let with = config(url, true);
    let conn = driver(with.driver).connect(&with).await.unwrap();
    let row = first_row(&*conn, user_sql).await;
    assert_eq!(row[0].to_lowercase(), "platypus");
    let row = first_row(&*conn, tls_sql).await;
    assert!(
        row.join(" ").contains(tls_expected),
        "{tls_sql}: {row:?}, expected {tls_expected}"
    );

    // Without it the server refuses, though the password is right.
    let err = refused(&config(url, false)).await;
    assert_ne!(err, "");

    // A half-set identity is a configuration error, before any connection.
    let mut half = config(url, true);
    half.ssl_client_key = None;
    let err = driver(half.driver).connect(&half).await.err().unwrap();
    assert!(matches!(err, Error::Config(_)), "{err}");

    // A certificate file that is not there says which.
    let mut missing = config(url, true);
    missing.ssl_client_cert = Some(tls_file("no-such.crt"));
    let err = refused(&missing).await;
    assert!(err.contains("no-such.crt"), "{err}");

    // So does a key file that is not there.
    let mut missing = config(url, true);
    missing.ssl_client_key = Some(tls_file("no-such.key"));
    let err = refused(&missing).await;
    assert!(err.contains("no-such.key"), "{err}");

    // With the key of the wrong certificate (the server's) the handshake fails.
    let mut wrong = config(url, true);
    wrong.ssl_client_key = Some(tls_file("server.key"));
    refused(&wrong).await;
}

#[tokio::test]
async fn client_certificate_postgres() {
    let Some(url) = server(
        "PLATYPUS_TEST_PG_MTLS_URL",
        "postgres://platypus@localhost:46433/platypus",
        "postgres client certificate",
    )
    .await
    else {
        return;
    };
    check_server(
        &url,
        (
            "SELECT current_user",
            "SELECT ssl::text, client_dn FROM pg_stat_ssl WHERE pid = pg_backend_pid()",
            "true /CN=platypus",
        ),
    )
    .await;
}

#[tokio::test]
async fn client_certificate_mysql() {
    let Some(url) = server(
        "PLATYPUS_TEST_MYSQL_MTLS_URL",
        "mysql://platypus:platypus@localhost:46307/platypus",
        "mysql client certificate",
    )
    .await
    else {
        return;
    };
    check_server(
        &url,
        (
            "SELECT SUBSTRING_INDEX(CURRENT_USER(), '@', 1)",
            "SELECT VARIABLE_VALUE FROM performance_schema.session_status WHERE VARIABLE_NAME = 'Ssl_cipher'",
            "TLS",
        ),
    )
    .await;
}
