//! The Kerberos login on PostgreSQL, with a real ticket from a real KDC: the `kdc` and
//! `postgres-gss` services of `docker/compose.yml` (profile `kerberos`). Their names resolve on
//! the compose network only, so the test runs in the `kerberos-client` service, on Linux with
//! MIT Kerberos; anywhere else it says `skipping ...` and proves nothing.
//!
//! The steps share one test, with the environment changing between them: `KRB5CCNAME` and
//! `KRB5_CLIENT_KTNAME` are read by the Kerberos library, and there is no other way to tell it.
#![allow(unsafe_code)]

use std::net::ToSocketAddrs;
use std::process::Command;
use std::time::Duration;

use arrow_cast::display::array_value_to_string;
use futures::TryStreamExt;
use platypus_core::{
    AUTH, Connection, ConnectionConfig, Driver, DriverKind, ExecOptions, ExecResult, KRB_SERVICE,
    Result, Secret, SslMode,
};
use platypus_driver_pg::{KERBEROS_AVAILABLE, PgDriver};

const PRINCIPAL: &str = "alice@PLATYPUS.TEST";

fn config(host: &str, user: &str, auth: Option<&str>) -> ConnectionConfig {
    let mut config = ConnectionConfig {
        host: host.to_owned(),
        port: Some(5432),
        database: "platypus".to_owned(),
        user: user.to_owned(),
        ssl_mode: SslMode::Disable,
        connect_timeout: Some(Duration::from_secs(15)),
        ..ConnectionConfig::new(DriverKind::Postgres)
    };
    if let Some(auth) = auth {
        config.options.insert(AUTH.to_owned(), auth.to_owned());
    }
    config
}

async fn login(config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
    PgDriver.connect(config).await
}

async fn refused(config: &ConnectionConfig) -> String {
    match login(config).await {
        Ok(_) => panic!("{config:?} logged in"),
        Err(e) => e.to_string(),
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

/// That `conn` is logged in as alice by Kerberos, as the server itself tells.
async fn is_alice_by_kerberos(conn: &dyn Connection) {
    let row = first_row(
        conn,
        "SELECT current_user::text, gss_authenticated::text, principal \
         FROM pg_stat_gssapi WHERE pid = pg_backend_pid()",
    )
    .await;
    assert_eq!(row, ["alice", "true", PRINCIPAL]);
}

fn set_env(name: &str, value: &str) {
    // SAFETY: this binary has a single test, and nothing else reads the environment meanwhile.
    unsafe { std::env::set_var(name, value) };
}

fn remove_env(name: &str) {
    // SAFETY: as in `set_env`.
    unsafe { std::env::remove_var(name) };
}

/// `kinit` of alice, from her keytab, with `lifetime` if given, into `KRB5CCNAME`.
fn kinit(keytab: &str, lifetime: Option<&str>) {
    let mut command = Command::new("kinit");
    if let Some(lifetime) = lifetime {
        command.args(["-l", lifetime]);
    }
    let status = command
        .args(["-k", "-t", keytab, PRINCIPAL])
        .status()
        .unwrap();
    assert!(status.success(), "kinit failed");
}

/// `psql` as the app starts it (`pg_connection` in the desktop app's `commands/cli.rs`).
fn psql(host: &str) -> (bool, String) {
    let output = Command::new("psql")
        .args(["-X", "-w", "-t", "-A"])
        .args([
            &format!("--host={host}"),
            "--username=alice",
            "-d",
            "platypus",
        ])
        .args([
            "-c",
            "SELECT current_user, gss_authenticated FROM pg_stat_gssapi WHERE pid = pg_backend_pid()",
        ])
        .env("PGKRBSRVNAME", "postgres")
        .env("PGREQUIREAUTH", "gss")
        .env("PGSSLMODE", "disable")
        .output()
        .unwrap();
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
            + &String::from_utf8_lossy(&output.stderr),
    )
}

/// What the test needs, or why it cannot run.
fn prerequisites() -> std::result::Result<(String, String), String> {
    if !KERBEROS_AVAILABLE {
        return Err("built without the `kerberos` feature".to_owned());
    }
    let host = std::env::var("PLATYPUS_KERBEROS_HOST").map_err(|_| {
        "PLATYPUS_KERBEROS_HOST is not set: run it in the `kerberos-client` service of \
         docker/compose.yml"
            .to_owned()
    })?;
    let keytab = std::env::var("PLATYPUS_KERBEROS_KEYTAB")
        .map_err(|_| "PLATYPUS_KERBEROS_KEYTAB is not set".to_owned())?;
    if !std::path::Path::new(&keytab).exists() {
        return Err(format!("no keytab at {keytab}: the KDC is not up"));
    }
    if std::net::TcpStream::connect((host.as_str(), 5432)).is_err() {
        return Err(format!("no server at {host}:5432"));
    }
    Ok((host, keytab))
}

/// A connection that uses Kerberos sends no password, whatever it holds and whatever the server
/// asks; the server it refuses does take that password.
async fn password_is_never_sent(host: &str) {
    let mut asked = config(host, "platypus", Some("kerberos"));
    asked.password = Some(Secret::new("platypus"));
    let error = refused(&asked).await;
    assert!(error.contains("none was sent"), "{error}");

    let mut by_password = config(host, "platypus", None);
    by_password.password = Some(Secret::new("platypus"));
    login(&by_password).await.unwrap();

    // The other way round: a password login where the server wants Kerberos is told to choose it.
    let mut kerberos_only = config(host, "alice", None);
    kerberos_only.password = Some(Secret::new("anything"));
    let error = refused(&kerberos_only).await;
    assert!(error.contains("Choose Kerberos"), "{error}");
}

#[tokio::test]
async fn logs_in_with_a_real_ticket() {
    let (host, keytab) = match prerequisites() {
        Ok(found) => found,
        Err(why) => {
            println!("skipping kerberos: {why}");
            return;
        }
    };
    let alice = config(&host, "alice", Some("kerberos"));
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("ticket");
    set_env("KRB5CCNAME", &format!("FILE:{}", cache.display()));
    remove_env("KRB5_CLIENT_KTNAME");

    // No ticket: the message says what to run, with the library's own text after it.
    let error = refused(&alice).await;
    for what in ["kinit", "KRB5CCNAME", "KRB5_CLIENT_KTNAME"] {
        assert!(error.contains(what), "{what} in {error}");
    }
    assert!(error.contains("Kerberos said:"), "{error}");
    println!("without a ticket: {error}");

    // The ticket of an external `kinit`, and no password anywhere.
    kinit(&keytab, None);
    let conn = login(&alice).await.unwrap();
    assert!(alice.password.is_none());
    is_alice_by_kerberos(&*conn).await;
    println!("logged in with the ticket of kinit, in {}", cache.display());

    // Over TLS too.
    let mut over_tls = alice.clone();
    over_tls.ssl_mode = SslMode::Require;
    let conn = login(&over_tls).await.unwrap();
    is_alice_by_kerberos(&*conn).await;
    assert!(conn.server_info().encrypted);

    // The service name in the option is the one asked for: this one does not exist.
    let mut other_service = alice.clone();
    other_service
        .options
        .insert(KRB_SERVICE.to_owned(), "nosuchservice".to_owned());
    let error = refused(&other_service).await;
    assert!(error.contains(&format!("nosuchservice/{host}")), "{error}");
    assert!(error.contains("service name"), "{error}");

    // The ticket is for the host's name wherever the socket goes, as through a tunnel; a host
    // that is not the name in the principal is the wrong service, said the same way.
    let address = (host.as_str(), 5432)
        .to_socket_addrs()
        .unwrap()
        .next()
        .unwrap();
    let mut tunnelled = alice.clone();
    tunnelled.connect_addr = Some(address);
    let conn = login(&tunnelled).await.unwrap();
    is_alice_by_kerberos(&*conn).await;
    tunnelled.host = "other-name.platypus.test".into();
    let error = refused(&tunnelled).await;
    assert!(error.contains("does not know"), "{error}");
    assert!(
        error.contains("postgres/other-name.platypus.test"),
        "{error}"
    );

    // `psql` signs in with the same ticket.
    let (ok, printed) = psql(&host);
    assert!(ok, "{printed}");
    assert!(printed.contains("alice|t"), "{printed}");
    println!("psql logged in: {printed}");

    password_is_never_sent(&host).await;

    // An expired ticket is said as such.
    kinit(&keytab, Some("1s"));
    tokio::time::sleep(Duration::from_secs(3)).await;
    let error = refused(&alice).await;
    assert!(error.contains("expired"), "{error}");
    assert!(error.contains("kinit"), "{error}");

    // A keytab instead of a ticket: the library gets the ticket by itself.
    std::fs::remove_file(&cache).unwrap();
    set_env("KRB5_CLIENT_KTNAME", &keytab);
    let conn = login(&alice).await.unwrap();
    is_alice_by_kerberos(&*conn).await;
    println!("logged in with the keytab in KRB5_CLIENT_KTNAME");
    remove_env("KRB5_CLIENT_KTNAME");
}
