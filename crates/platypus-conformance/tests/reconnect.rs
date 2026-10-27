//! Sessions the server dropped, found before the next statement: `Connection::check_alive`
//! on MySQL, MariaDB, SQL Server and PostgreSQL, and what SQL Server and SQLite report
//! when connecting. The servers come from `docker/compose.yml`, as in `conformance.rs`;
//! an unreachable one is skipped unless its `PLATYPUS_TEST_*_URL` is set.

use std::time::{Duration, Instant};

use futures::TryStreamExt;
use platypus_core::{Connection, ConnectionConfig, Driver, DriverKind, ExecOptions, ExecResult};
use platypus_driver_mssql::MssqlDriver;
use platypus_driver_mysql::MySqlDriver;
use platypus_driver_pg::PgDriver;
use platypus_driver_sqlite::SqliteDriver;

fn driver(kind: DriverKind) -> &'static dyn Driver {
    match kind {
        DriverKind::Postgres => &PgDriver,
        DriverKind::MySql => &MySqlDriver,
        DriverKind::SqlServer => &MssqlDriver,
        DriverKind::Sqlite => &SqliteDriver,
        other => unreachable!("{other:?}"),
    }
}

/// A server, with the statement that gives the session's id and the one that kills a
/// session by id.
struct Server {
    name: &'static str,
    config: ConnectionConfig,
    session_id: &'static str,
    kill: fn(&str) -> String,
}

async fn config(env: &str, default: &str, name: &str) -> Option<ConnectionConfig> {
    let (url, required) = match std::env::var(env) {
        Ok(url) => (url, true),
        Err(_) => (default.to_owned(), false),
    };
    let config = ConnectionConfig::from_url(&url).unwrap();
    match driver(config.driver).connect(&config).await {
        Ok(_) => Some(config),
        Err(e) if !required => {
            eprintln!("skipping {name}: unreachable at {default}: {e}");
            None
        }
        Err(e) => panic!("cannot connect to {env}: {e}"),
    }
}

async fn servers() -> Vec<Server> {
    let mut all = Vec::new();
    for (name, env, default) in [
        (
            "mysql",
            "PLATYPUS_TEST_MYSQL_URL",
            "mysql://platypus:platypus@localhost:53306/platypus",
        ),
        (
            "mariadb",
            "PLATYPUS_TEST_MARIADB_URL",
            "mysql://platypus:platypus@localhost:53307/platypus",
        ),
    ] {
        if let Some(config) = config(env, default, name).await {
            all.push(Server {
                name,
                config,
                session_id: "SELECT CONNECTION_ID()",
                kill: |id| format!("KILL CONNECTION {id}"),
            });
        }
    }
    if let Some(config) = config(
        "PLATYPUS_TEST_MSSQL_URL",
        "sqlserver://sa:Platypus-2026@localhost:51433/platypus?sslmode=require",
        "sqlserver",
    )
    .await
    {
        all.push(Server {
            name: "sqlserver",
            config,
            session_id: "SELECT @@SPID",
            kill: |id| format!("KILL {id}"),
        });
    }
    if let Some(config) = config(
        "PLATYPUS_TEST_PG_URL",
        "postgres://platypus:platypus@localhost:55432/platypus?sslmode=disable",
        "postgres",
    )
    .await
    {
        all.push(Server {
            name: "postgres",
            config,
            session_id: "SELECT pg_backend_pid()",
            kill: |id| format!("SELECT pg_terminate_backend({id})"),
        });
    }
    all
}

async fn connect(config: &ConnectionConfig) -> Box<dyn Connection> {
    driver(config.driver).connect(config).await.unwrap()
}

/// The first column of the first row of `sql`, as text.
async fn scalar(conn: &dyn Connection, sql: &str) -> String {
    let ExecResult::Rows(stream) = conn.execute(sql, &ExecOptions::default()).await.unwrap() else {
        panic!("no rows from {sql}");
    };
    let batches: Vec<_> = stream.try_collect().await.unwrap();
    arrow_cast::display::array_value_to_string(batches[0].column(0), 0).unwrap()
}

async fn run(conn: &dyn Connection, sql: &str) {
    match conn.execute(sql, &ExecOptions::default()).await.unwrap() {
        ExecResult::Rows(stream) => {
            stream.try_for_each(|_| async { Ok(()) }).await.unwrap();
        }
        ExecResult::Affected { .. } => {}
    }
}

/// `check_alive` until it says the session is gone, for at most 5 s: PostgreSQL learns
/// it from its connection task, a moment after the kill.
async fn gone(conn: &dyn Connection) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if !conn.check_alive().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

#[tokio::test]
async fn qa_fixconn_finds_killed_sessions_before_the_next_statement() {
    for s in servers().await {
        let conn = connect(&s.config).await;
        assert!(conn.check_alive().await, "{}: a live session", s.name);
        let id = scalar(&*conn, s.session_id).await;
        let killer = connect(&s.config).await;
        run(&*killer, &(s.kill)(&id)).await;
        // Found without running a statement on the session.
        assert!(
            gone(&*conn).await,
            "{}: the killed session looks alive",
            s.name
        );
        assert!(conn.is_closed(), "{}: not reported as closed", s.name);
    }
}

#[tokio::test]
async fn qa_fixconn_probe_opens_no_transaction() {
    for s in servers().await {
        let conn = connect(&s.config).await;
        conn.set_autocommit(false).await.unwrap();
        assert!(conn.check_alive().await, "{}", s.name);
        let tx = conn.transaction();
        assert!(!tx.autocommit && !tx.open, "{}: {tx:?}", s.name);
        // Nor is it confused by one that is open.
        run(&*conn, "SELECT 1").await;
        let open = conn.transaction().open;
        assert!(conn.check_alive().await, "{}", s.name);
        assert_eq!(conn.transaction().open, open, "{}", s.name);
        conn.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn qa_fixconn_mysql_sessions_past_wait_timeout_are_found() {
    for s in servers().await {
        if s.config.driver != DriverKind::MySql {
            continue;
        }
        let conn = connect(&s.config).await;
        run(&*conn, "SET SESSION wait_timeout = 1").await;
        tokio::time::sleep(Duration::from_millis(2500)).await;
        assert!(!conn.check_alive().await, "{}: past wait_timeout", s.name);
        assert!(conn.is_closed(), "{}", s.name);
    }
}

#[tokio::test]
async fn qa_fixconn_sqlserver_reports_its_encryption() {
    let Some(server) = servers()
        .await
        .into_iter()
        .find(|s| s.config.driver == DriverKind::SqlServer)
    else {
        return;
    };
    for mode in ["disable", "prefer", "require"] {
        let mut config = server.config.clone();
        config.ssl_mode = mode.parse().unwrap();
        let conn = match MssqlDriver.connect(&config).await {
            Ok(conn) => conn,
            // A server that forces encryption refuses `disable`.
            Err(e) if mode == "disable" => {
                eprintln!("sqlserver: {mode}: {e}");
                continue;
            }
            Err(e) => panic!("sqlserver: {mode}: {e}"),
        };
        let server_says = scalar(
            &*conn,
            "SELECT encrypt_option FROM sys.dm_exec_connections WHERE session_id = @@SPID",
        )
        .await;
        assert_eq!(
            conn.server_info().encrypted,
            server_says.eq_ignore_ascii_case("TRUE"),
            "sqlserver: {mode}: the server says {server_says}"
        );
    }
}

#[tokio::test]
async fn qa_fixconn_sqlite_creates_a_new_file_in_an_existing_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = ConnectionConfig::new(DriverKind::Sqlite);
    config.host.clear();
    let file = dir.path().join("new.db");
    config.database = file.to_string_lossy().into_owned();
    let conn = connect(&config).await;
    run(&*conn, "CREATE TABLE t (id INTEGER)").await;
    assert!(file.exists());
    config.database = dir
        .path()
        .join("missing")
        .join("new.db")
        .to_string_lossy()
        .into_owned();
    let err = SqliteDriver.connect(&config).await.unwrap_err().to_string();
    assert!(err.contains("does not exist"), "{err}");
}
