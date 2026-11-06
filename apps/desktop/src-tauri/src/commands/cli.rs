//! Runs a script with the database's own command-line client found on the PATH (`psql`,
//! `mysql`, `sqlcmd` or `sqlite3`; none for DuckDB), with the settings and password of the
//! editor's session, for scripts that rely on the client's own commands such as `\copy`,
//! `source` or `:r`.

use std::process::{Output, Stdio};
use std::time::{Duration, Instant};

use platypus_core::{Auth, ConnectionConfig, DriverKind, ExecOptions, ExecResult, SslMode};
use platypus_net::Tunnel;
use serde::Serialize;
use tauri::State;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::oneshot;

use super::{AppState, lock};
use crate::error::{CommandError, CommandResult};

#[derive(Debug, Serialize)]
pub struct CliOutput {
    /// The command line, without the password (which goes in the environment).
    command: String,
    /// The exit code; `None` when the client was stopped by a signal.
    status: Option<i32>,
    /// What the client printed, standard output then standard error.
    output: String,
    elapsed_ms: u64,
}

fn program(driver: DriverKind) -> CommandResult<&'static str> {
    match driver {
        DriverKind::Postgres => Ok("psql"),
        DriverKind::MySql => Ok("mysql"),
        DriverKind::Sqlite => Ok("sqlite3"),
        DriverKind::SqlServer => Ok("sqlcmd"),
        DriverKind::Redis => Ok("redis-cli"),
        // DuckDB locks the file for the process that opens it, and the session holds it.
        DriverKind::DuckDb => Err(CommandError::new(
            "cli",
            "the duckdb client cannot open a database file that PlatypusDB has open",
        )),
        // The connection is to the HTTP interface; clickhouse-client speaks the native
        // protocol, on another port.
        DriverKind::ClickHouse => Err(CommandError::new(
            "cli",
            "clickhouse-client uses the native protocol (port 9000), not the HTTP port of this connection",
        )),
        DriverKind::Libsql => Err(CommandError::new(
            "cli",
            "libSQL servers have no command-line client to run scripts with",
        )),
        // mongosh takes the password only on its command line, where other users see it.
        DriverKind::Mongo => Err(CommandError::new(
            "cli",
            "terminal mode is not available for MongoDB",
        )),
        DriverKind::Elasticsearch => Err(CommandError::new(
            "cli",
            "terminal mode is not available for Elasticsearch",
        )),
        // cqlsh takes the password only on its command line, where other users see it.
        DriverKind::Cassandra => Err(CommandError::new(
            "cli",
            "terminal mode is not available for Cassandra",
        )),
        DriverKind::Snowflake
        | DriverKind::BigQuery
        | DriverKind::Databricks
        | DriverKind::Trino
        | DriverKind::Adbc
        | DriverKind::Oracle
        | DriverKind::Db2
        | DriverKind::Odbc
        | DriverKind::Plugin => Err(CommandError::new(
            "cli",
            format!("terminal mode is not available for {}", driver.name()),
        )),
    }
}

/// Arguments and environment of a command-line tool.
pub(super) type Invocation = (Vec<String>, Vec<(&'static str, String)>);

/// The database name as the PostgreSQL tools' `-d` takes it: a connection string of the
/// name alone, quoted. Given bare, a name holding `=` (or a URI) would be read as a
/// connection string of its own, able to point the tools, and the password, at another host.
fn pg_database(name: &str) -> String {
    if name.is_empty() {
        return String::new();
    }
    format!(
        "dbname='{}'",
        name.replace('\\', r"\\").replace('\'', r"\'")
    )
}

/// `value` as an argument that a client cannot take for one of its options.
pub(super) fn not_an_option<'a>(what: &str, value: &'a str) -> CommandResult<&'a str> {
    if value.starts_with('-') {
        return Err(CommandError::new(
            "invalid",
            format!("the {what} cannot start with '-': {value}"),
        ));
    }
    Ok(value)
}

/// What the PostgreSQL tools (`psql`, `pg_dump`, `pg_restore`) need to reach `config`'s
/// database at `host:port`: the password and TLS go in the environment.
pub(super) fn pg_connection(
    config: &ConnectionConfig,
    host: &str,
    port: Option<u16>,
) -> Invocation {
    let mut args: Vec<String> = vec![
        format!("--host={host}"),
        format!("--username={}", config.user),
        "-d".to_owned(),
        pg_database(&config.database),
    ];
    if let Some(port) = port {
        args.extend(["-p".to_owned(), port.to_string()]);
    }
    let mode = match config.ssl_mode {
        SslMode::Disable => "disable",
        SslMode::Prefer => "prefer",
        SslMode::Require => "require",
        SslMode::VerifyCa => "verify-ca",
        SslMode::VerifyFull => "verify-full",
    };
    let mut env = vec![
        ("PGSSLMODE", mode.to_owned()),
        ("PGCLIENTENCODING", "UTF8".to_owned()),
    ];
    if let Some(cert) = &config.ssl_root_cert {
        env.push(("PGSSLROOTCERT", cert.display().to_string()));
    }
    env.extend(
        config
            .password
            .as_ref()
            .map(|p| ("PGPASSWORD", p.expose().to_owned())),
    );
    (args, env)
}

/// What the MySQL tools (`mysql`, `mysqldump`) need to reach `config`'s server at
/// `host:port`, the database left out: each takes it at its own place.
pub(super) fn mysql_connection(
    config: &ConnectionConfig,
    host: &str,
    port: Option<u16>,
) -> Invocation {
    // `--name=value`, which no value can turn into another option.
    let mut args: Vec<String> = vec![format!("--host={host}"), format!("--user={}", config.user)];
    if let Some(port) = port {
        args.push(format!("--port={port}"));
    }
    // The default needs no option: the MariaDB client, often installed as `mysql`,
    // does not know `--ssl-mode`.
    let mode = match config.ssl_mode {
        SslMode::Disable => Some("DISABLED"),
        SslMode::Prefer => None,
        SslMode::Require => Some("REQUIRED"),
        SslMode::VerifyCa => Some("VERIFY_CA"),
        SslMode::VerifyFull => Some("VERIFY_IDENTITY"),
    };
    args.extend(mode.map(|m| format!("--ssl-mode={m}")));
    if let Some(cert) = &config.ssl_root_cert {
        args.push(format!("--ssl-ca={}", cert.display()));
    }
    // An IAM or Entra ID token goes as written, as the driver sends it.
    if config.auth().is_ok_and(Auth::is_token) {
        args.push("--enable-cleartext-plugin".to_owned());
    }
    let env = config
        .password
        .as_ref()
        .map(|p| ("MYSQL_PWD", p.expose().to_owned()))
        .into_iter()
        .collect();
    (args, env)
}

/// `redis-cli`'s arguments for `config` at `host:port`: database, user and TLS.
fn redis_cli_args(config: &ConnectionConfig, host: &str, port: Option<u16>) -> Vec<String> {
    let mut args = vec!["-h".to_owned(), host.to_owned()];
    if let Some(port) = port {
        args.extend(["-p".to_owned(), port.to_string()]);
    }
    let db = config.database.trim().trim_start_matches("db");
    if !db.is_empty() {
        args.extend(["-n".to_owned(), db.to_owned()]);
    }
    if !config.user.is_empty() {
        args.extend(["--user".to_owned(), config.user.clone()]);
    }
    if config.redis_tls() {
        args.push("--tls".to_owned());
        if !matches!(config.ssl_mode, SslMode::VerifyCa | SslMode::VerifyFull) {
            args.push("--insecure".to_owned());
        }
        for (flag, path) in [
            ("--cacert", &config.ssl_root_cert),
            ("--cert", &config.ssl_client_cert),
            ("--key", &config.ssl_client_key),
        ] {
            if let Some(path) = path {
                args.extend([flag.to_owned(), path.display().to_string()]);
            }
        }
    }
    args
}

/// The client's arguments and environment for `config`, reached at `host:port`.
fn invocation(config: &ConnectionConfig, host: &str, port: Option<u16>) -> Invocation {
    let mut args: Vec<String> = Vec::new();
    let mut env = Vec::new();
    let password = config.password.as_ref().map(|p| p.expose().to_owned());
    match config.driver {
        DriverKind::Postgres => {
            // No prompts (stdin is the script), stop at the first error, no ~/.psqlrc.
            args.extend(["-X", "-w", "-v", "ON_ERROR_STOP=1"].map(str::to_owned));
            let (connection, pg_env) = pg_connection(config, host, port);
            args.extend(connection);
            env = pg_env;
        }
        DriverKind::MySql => {
            args.extend(["--table", "--default-character-set=utf8mb4"].map(str::to_owned));
            let (connection, mysql_env) = mysql_connection(config, host, port);
            args.extend(connection);
            if !config.database.is_empty() {
                args.push(format!("--database={}", config.database));
            }
            env = mysql_env;
        }
        DriverKind::Sqlite | DriverKind::DuckDb | DriverKind::Libsql => {
            args.extend(["-bail", "-header", "-column"].map(str::to_owned));
            args.push(sqlite_file(&config.database));
        }
        DriverKind::SqlServer => {
            args = sqlcmd_args(config, host, port);
            if matches!(config.auth(), Ok(Auth::Password)) {
                env.extend(password.map(|p| ("SQLCMDPASSWORD", p)));
            }
        }
        DriverKind::Redis => {
            args = redis_cli_args(config, host, port);
            // The password goes in the environment, not on the command line.
            env.extend(password.map(|p| ("REDISCLI_AUTH", p)));
        }
        // No client: `program` refuses them.
        DriverKind::ClickHouse
        | DriverKind::Mongo
        | DriverKind::Elasticsearch
        | DriverKind::Cassandra
        | DriverKind::Snowflake
        | DriverKind::BigQuery
        | DriverKind::Databricks
        | DriverKind::Trino
        | DriverKind::Adbc
        | DriverKind::Oracle
        | DriverKind::Db2
        | DriverKind::Odbc
        | DriverKind::Plugin => {}
    }
    (args, env)
}

/// The database file as `sqlite3` takes it: a path starting with `-` would be read as an
/// option (older shells know no `--`), so it goes as `./-…`, the same file.
fn sqlite_file(path: &str) -> String {
    if path.starts_with('-') {
        format!("./{path}")
    } else {
        path.to_owned()
    }
}

/// `sqlcmd` stops at the first error, with quoted identifiers on (it turns them off by
/// default) and UTF-8 input and output.
fn sqlcmd_args(config: &ConnectionConfig, host: &str, port: Option<u16>) -> Vec<String> {
    let server = match port {
        Some(port) => format!("{host},{port}"),
        None => host.to_owned(),
    };
    let mut args: Vec<String> = ["-b", "-I", "-f", "65001", "-S", &server]
        .map(str::to_owned)
        .into();
    // Windows' sign-in is `-E`; Entra ID is `-G`, which signs in as `az` or Windows does.
    match config.auth() {
        Ok(Auth::Windows) => args.push("-E".to_owned()),
        Ok(Auth::EntraId) => args.push("-G".to_owned()),
        _ => args.extend(["-U".to_owned(), config.user.clone()]),
    }
    if !config.database.is_empty() {
        args.extend(["-d".to_owned(), config.database.clone()]);
    }
    // `prefer` and `require` encrypt without checking the server's certificate.
    let tls: &[&str] = match config.ssl_mode {
        SslMode::Disable => &[],
        SslMode::Prefer | SslMode::Require => &["-N", "-C"],
        SslMode::VerifyCa | SslMode::VerifyFull => &["-N"],
    };
    args.extend(tls.iter().map(|a| (*a).to_owned()));
    args
}

/// The tunnel, proxy or port forward of `config`, opened when it has one, and the host and
/// port that reach its server: the tunnel's local end, or the server itself. A token the
/// connection signs in with goes in `config` as its password; not SQL Server's, which
/// `sqlcmd -G` gets for itself.
pub(super) async fn reach(
    config: &mut ConnectionConfig,
) -> CommandResult<(Option<Tunnel>, String, Option<u16>)> {
    if config.driver != DriverKind::SqlServer {
        platypus_net::sign_in(config).await?;
    }
    let tunnel = Tunnel::of(config, None).await?;
    let (host, port) = match &tunnel {
        Some(t) => ("127.0.0.1".to_owned(), Some(t.local_addr().port())),
        None => (config.host.clone(), config.port),
    };
    Ok((tunnel, host, port))
}

/// The error of a client that could not start.
fn not_started(program: &str, e: &std::io::Error) -> CommandError {
    CommandError::new(
        "cli",
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("{program} was not found: install the client and put it on the PATH")
        } else {
            format!("cannot start {program}: {e}")
        },
    )
}

/// Starts `program` with `invocation`, its output read through pipes, and its input too when
/// `input`; it is killed when dropped.
pub(super) fn spawn(program: &str, (args, env): Invocation, input: bool) -> CommandResult<Child> {
    let mut command = Command::new(program);
    command
        .args(&args)
        .envs(env)
        .stdin(if input { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashes up.
    command.spawn().map_err(|e| not_started(program, &e))
}

/// Waits for the client to end, or (`None`) for `cancel`: the client is then killed, as
/// dropping its wait drops the child, spawned with `kill_on_drop`. A cancel already sent wins
/// over a client that ends in the meantime.
async fn wait_or_cancel(
    child: Child,
    cancel: oneshot::Receiver<()>,
) -> Option<std::io::Result<Output>> {
    let wait = Box::pin(child.wait_with_output());
    match futures::future::select(cancel, wait).await {
        futures::future::Either::Left(_) => None,
        futures::future::Either::Right((out, _)) => Some(out),
    }
}

/// Runs `script` with the client of the session's database and waits for it to end;
/// `cancel_cli` with the same `run_id` stops it, also while the SSH tunnel opens.
#[tauri::command]
pub async fn run_cli(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    session_id: u64,
    run_id: u64,
    script: String,
) -> CommandResult<CliOutput> {
    state.may_run(session_id, &script)?;
    if super::plugins::query_hooked() {
        super::plugins::before_query(&app, &state, session_id, &script).await?;
    }
    let (stop, cancel) = oneshot::channel();
    lock(&state.clis).insert(run_id, stop);
    let out = run(&state, session_id, run_id, script, cancel).await;
    lock(&state.clis).remove(&run_id);
    out
}

pub(super) fn stopped(program: &str) -> CommandError {
    CommandError::new("cancelled", format!("{program} was stopped"))
}

async fn run(
    state: &AppState,
    session_id: u64,
    run_id: u64,
    script: String,
    mut cancel: oneshot::Receiver<()>,
) -> CommandResult<CliOutput> {
    let session = state.session(session_id)?.config;
    let program = program(session.driver)?;
    let mut config = session.clone();
    // The tunnel or proxy stays open while the client runs.
    let (tunnel, host, port) = reach(&mut config).await?;
    if cancel.try_recv().is_ok() {
        return Err(stopped(program));
    }
    let (args, mut env) = invocation(&config, &host, port);
    let name = client_name(run_id);
    if config.driver == DriverKind::Postgres {
        env.push(("PGAPPNAME", name.clone()));
    }
    let started = Instant::now();
    let mut child = spawn(program, (args.clone(), env), true)?;
    let pid = child.id();
    if let Some(mut stdin) = child.stdin.take() {
        // Written alongside the reads, so a client that prints a lot cannot block the script;
        // one that exits early closes its input, and its output says why.
        tauri::async_runtime::spawn(async move {
            let _ = stdin.write_all(script.as_bytes()).await;
        });
    }
    let out = wait_or_cancel(child, cancel).await;
    drop(tunnel);
    let Some(out) = out else {
        // The client is killed, but its statement runs on in the server: ended there too.
        tauri::async_runtime::spawn(async move {
            let ended =
                tokio::time::timeout(Duration::from_secs(30), end_on_server(&session, &name, pid))
                    .await;
            if let Ok(Err(e)) = ended {
                crate::applog::write(
                    "WARN",
                    &format!(
                        "{program} was stopped; its statement may still run on the server: {}",
                        e.message()
                    ),
                );
            }
        });
        return Err(stopped(program));
    };
    let out = out.map_err(|e| CommandError::new("cli", format!("{program}: {e}")))?;
    let mut output = String::from_utf8_lossy(&out.stdout).into_owned();
    output.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(CliOutput {
        command: format!("{program} {}", args.join(" ")),
        status: out.status.code(),
        output,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

/// The name a client run gives its session on the server (PostgreSQL's `application_name`),
/// so that a cancel finds it: no other run of this app, nor another app, uses it.
fn client_name(run_id: u64) -> String {
    format!("PlatypusDB script {}-{run_id}", std::process::id())
}

/// The query that lists the server's sessions of the killed client process `pid` (named
/// `name`): by name on PostgreSQL, by the process id the client tells the server elsewhere.
/// MySQL and SQL Server show other sessions of the account (MySQL) or of the server (SQL
/// Server, with VIEW SERVER STATE) only to some; the others find none. `None` for a client
/// whose statements end with it (`sqlite3`, `redis-cli`).
fn client_sessions(driver: DriverKind, name: &str, pid: Option<u32>) -> Option<String> {
    match (driver, pid) {
        (DriverKind::Postgres, _) => Some(format!(
            "SELECT pid FROM pg_stat_activity WHERE application_name = '{}' AND pid <> pg_backend_pid()",
            name.replace('\'', "''")
        )),
        (DriverKind::MySql, Some(pid)) => Some(format!(
            "SELECT a.PROCESSLIST_ID FROM performance_schema.session_account_connect_attrs a \
             JOIN performance_schema.session_account_connect_attrs p \
             ON p.PROCESSLIST_ID = a.PROCESSLIST_ID AND p.ATTR_NAME = 'program_name' \
             AND p.ATTR_VALUE IN ('mysql', 'mariadb') \
             WHERE a.ATTR_NAME = '_pid' AND a.ATTR_VALUE = '{pid}' \
             AND a.PROCESSLIST_ID <> CONNECTION_ID()"
        )),
        (DriverKind::SqlServer, Some(pid)) => Some(format!(
            "SELECT session_id FROM sys.dm_exec_sessions WHERE host_process_id = {pid} \
             AND program_name LIKE '%sqlcmd%' AND session_id <> @@SPID"
        )),
        _ => None,
    }
}

/// The statement that ends server session `id`, with what it was running.
fn end_session(driver: DriverKind, id: u64) -> String {
    match driver {
        DriverKind::Postgres => format!("SELECT pg_terminate_backend({id})"),
        _ => format!("KILL {id}"),
    }
}

/// Ends on the server the sessions of the client process `pid` (named `name`) that was
/// killed: the client sent no cancel, so its statement would run on to its end. On a
/// connection of its own, with `config`'s settings; returns how many it ended.
async fn end_on_server(
    config: &ConnectionConfig,
    name: &str,
    pid: Option<u32>,
) -> CommandResult<usize> {
    let Some(find) = client_sessions(config.driver, name, pid) else {
        return Ok(0);
    };
    let conn = super::connect(config).await?;
    let ids: Vec<u64> = platypus_core::query_text(conn.as_ref(), &find)
        .await?
        .iter()
        .filter_map(|row| row.first()?.as_deref()?.trim().parse().ok())
        .collect();
    for &id in &ids {
        let result = conn
            .execute(&end_session(config.driver, id), &ExecOptions::default())
            .await?;
        if let ExecResult::Rows(rows) = result {
            platypus_core::text_rows(rows).await?;
        }
    }
    Ok(ids.len())
}

/// Stops a script running in a command-line client; nothing when it has ended.
#[tauri::command]
pub fn cancel_cli(state: State<'_, AppState>, run_id: u64) {
    if let Some(stop) = lock(&state.clis).remove(&run_id) {
        let _ = stop.send(());
    }
}

#[cfg(test)]
mod tests {
    use platypus_core::Secret;

    use super::*;

    #[test]
    fn cancelling_kills_the_client() {
        // `sort` waits for the end of its input; on Unix tokio spawns only inside a runtime.
        let out = tauri::async_runtime::block_on(async {
            let child = Command::new("sort")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .expect("sort is on every PATH");
            let (stop, cancel) = oneshot::channel();
            stop.send(()).unwrap();
            wait_or_cancel(child, cancel).await
        });
        assert!(out.is_none());
    }

    #[test]
    fn finds_the_killed_client_on_the_server() {
        let name = client_name(7);
        assert_ne!(name, client_name(8), "each run has its own name");
        let pg = client_sessions(DriverKind::Postgres, &name, None).unwrap();
        assert!(pg.contains(&format!("application_name = '{name}'")), "{pg}");
        let mysql = client_sessions(DriverKind::MySql, &name, Some(42)).unwrap();
        assert!(mysql.contains("ATTR_VALUE = '42'"), "{mysql}");
        let sql_server = client_sessions(DriverKind::SqlServer, &name, Some(42)).unwrap();
        assert!(sql_server.contains("host_process_id = 42"), "{sql_server}");
        // Without the process id there is nothing to find it by; sqlite3 and redis-cli end
        // their statements with them.
        assert_eq!(client_sessions(DriverKind::MySql, &name, None), None);
        assert_eq!(client_sessions(DriverKind::Sqlite, &name, Some(42)), None);
        assert_eq!(client_sessions(DriverKind::Redis, &name, Some(42)), None);
        assert_eq!(
            end_session(DriverKind::Postgres, 9),
            "SELECT pg_terminate_backend(9)"
        );
        assert_eq!(end_session(DriverKind::MySql, 9), "KILL 9");
        assert_eq!(end_session(DriverKind::SqlServer, 9), "KILL 9");
    }

    /// Whether `program` runs here, and the server at `port` answers.
    fn available(program: &str, port: u16) -> bool {
        let found = std::process::Command::new(program)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success());
        let up = std::net::TcpStream::connect(("127.0.0.1", port)).is_ok();
        if !found || !up {
            eprintln!("skipping {program}: not installed, or no server on {port}");
        }
        found && up
    }

    /// How many statements holding the test's marker `watcher` sees running, its own aside.
    async fn running(watcher: &dyn platypus_core::Connection, driver: DriverKind) -> String {
        let sql = if driver == DriverKind::Postgres {
            "SELECT count(*) FROM pg_stat_activity WHERE query LIKE '%platypus_cli_cancel%' \
             AND state = 'active' AND pid <> pg_backend_pid()"
        } else {
            "SELECT count(*) FROM information_schema.PROCESSLIST \
             WHERE INFO LIKE '%platypus_cli_cancel%' AND ID <> CONNECTION_ID()"
        };
        let rows = platypus_core::query_text(watcher, sql).await.unwrap();
        rows[0][0].clone().unwrap()
    }

    /// Polls `running` for up to ten seconds until it says `count`.
    async fn wait_running(
        watcher: &dyn platypus_core::Connection,
        driver: DriverKind,
        count: &str,
    ) {
        for _ in 0..100 {
            if running(watcher, driver).await == count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!(
            "{driver:?}: {} statements running, not {count}",
            running(watcher, driver).await
        );
    }

    /// psql and mysql against the dev databases: a long statement, the client killed, and
    /// the statement gone from the server, which would otherwise run it to its end.
    #[tokio::test]
    async fn cancelling_ends_the_statement_on_the_server() {
        for (url, port, script) in [
            (
                "postgres://platypus:platypus@127.0.0.1:55432/platypus?sslmode=disable",
                55432,
                "SELECT pg_sleep(60) AS platypus_cli_cancel;\n",
            ),
            (
                "mysql://platypus:platypus@127.0.0.1:53306/platypus",
                53306,
                "SELECT SLEEP(60) AS platypus_cli_cancel;\n",
            ),
        ] {
            let config = ConnectionConfig::from_url(url).unwrap();
            let program = program(config.driver).unwrap();
            if !available(program, port) {
                continue;
            }
            let watcher = super::super::connect(&config).await.unwrap();
            let name = client_name(1);
            let (args, mut env) = invocation(&config, "127.0.0.1", Some(port));
            if config.driver == DriverKind::Postgres {
                env.push(("PGAPPNAME", name.clone()));
            }
            let mut child = spawn(program, (args, env), true).unwrap();
            let pid = child.id();
            let mut stdin = child.stdin.take().unwrap();
            stdin.write_all(script.as_bytes()).await.unwrap();
            wait_running(watcher.as_ref(), config.driver, "1").await;

            let (stop, cancel) = oneshot::channel();
            stop.send(()).unwrap();
            assert!(wait_or_cancel(child, cancel).await.is_none());
            drop(stdin);
            assert_eq!(
                end_on_server(&config, &name, pid).await.unwrap(),
                1,
                "{program}"
            );
            wait_running(watcher.as_ref(), config.driver, "0").await;
        }
    }

    #[test]
    fn passes_the_password_in_the_environment() {
        let mut config = ConnectionConfig::new(DriverKind::Postgres);
        config.user = "app".to_owned();
        config.database = "shop".to_owned();
        config.password = Some(Secret::new("s3cret"));
        let (args, env) = invocation(&config, "db", Some(5433));
        assert!(args.windows(2).any(|w| w == ["-p", "5433"]));
        assert!(!args.iter().any(|a| a.contains("s3cret")));
        assert!(env.contains(&("PGPASSWORD", "s3cret".to_owned())));

        config.driver = DriverKind::MySql;
        let (args, env) = invocation(&config, "db", None);
        assert_eq!(args.last().map(String::as_str), Some("--database=shop"));
        assert!(
            !args.iter().any(|a| a.starts_with("--ssl-mode")),
            "default SSL mode"
        );
        assert!(env.contains(&("MYSQL_PWD", "s3cret".to_owned())));

        config.driver = DriverKind::SqlServer;
        let (args, env) = invocation(&config, "db", Some(1433));
        assert!(args.windows(2).any(|w| w == ["-S", "db,1433"]));
        assert!(args.windows(2).any(|w| w == ["-d", "shop"]));
        assert!(
            args.iter().any(|a| a == "-C"),
            "prefer trusts the certificate"
        );
        assert!(!args.iter().any(|a| a.contains("s3cret")));
        assert!(env.contains(&("SQLCMDPASSWORD", "s3cret".to_owned())));
    }

    #[test]
    fn no_name_turns_into_an_option_of_the_client() {
        let mut config = ConnectionConfig::new(DriverKind::Postgres);
        config.user = "-U".to_owned();
        config.database = "x host=evil.example port=1 '\\".to_owned();
        let (args, _) = invocation(&config, "-h", None);
        assert!(args.contains(&"--host=-h".to_owned()), "{args:?}");
        assert!(args.contains(&"--username=-U".to_owned()), "{args:?}");
        // The whole name is the database's, quoted: no host of its own.
        assert!(
            args.windows(2)
                .any(|w| w == ["-d", r"dbname='x host=evil.example port=1 \'\\'"]),
            "{args:?}"
        );
        config.database = String::new();
        let (args, _) = invocation(&config, "db", None);
        assert!(args.windows(2).any(|w| w == ["-d", ""]), "{args:?}");

        config.driver = DriverKind::MySql;
        config.database = "--init-command=DROP DATABASE x".to_owned();
        let (args, _) = invocation(&config, "--skip-ssl", None);
        assert!(args.contains(&"--host=--skip-ssl".to_owned()), "{args:?}");
        assert!(args.contains(&"--user=-U".to_owned()), "{args:?}");
        assert_eq!(
            args.last().map(String::as_str),
            Some("--database=--init-command=DROP DATABASE x")
        );
        assert!(!args.iter().any(|a| a.starts_with("--init-command")));

        config.driver = DriverKind::Sqlite;
        config.database = "-cmd".to_owned();
        let (args, _) = invocation(&config, "", None);
        assert_eq!(args.last().map(String::as_str), Some("./-cmd"));
        config.database = "/data/app.db".to_owned();
        let (args, _) = invocation(&config, "", None);
        assert_eq!(args.last().map(String::as_str), Some("/data/app.db"));

        assert!(not_an_option("database", "-x").is_err());
        assert_eq!(not_an_option("database", "shop").ok(), Some("shop"));
    }

    #[test]
    fn clients_sign_in_as_the_connection_does() {
        let mut config = ConnectionConfig::new(DriverKind::MySql);
        config.user = "app".to_owned();
        config.password = Some(Secret::new("token"));
        config
            .options
            .insert(platypus_core::AUTH.to_owned(), "aws_iam".to_owned());
        let (args, env) = invocation(&config, "db", None);
        assert!(args.iter().any(|a| a == "--enable-cleartext-plugin"));
        assert!(env.contains(&("MYSQL_PWD", "token".to_owned())));

        config.driver = DriverKind::SqlServer;
        config
            .options
            .insert(platypus_core::AUTH.to_owned(), "windows".to_owned());
        let (args, env) = invocation(&config, "db", None);
        assert!(args.iter().any(|a| a == "-E"));
        assert!(!args.iter().any(|a| a == "-U"));
        assert_eq!(env, []);

        config
            .options
            .insert(platypus_core::AUTH.to_owned(), "entra_id".to_owned());
        let (args, _) = invocation(&config, "db", None);
        assert!(args.iter().any(|a| a == "-G"));
    }

    #[test]
    fn has_no_client_for_duckdb() {
        assert_eq!(program(DriverKind::Sqlite).ok(), Some("sqlite3"));
        assert!(program(DriverKind::DuckDb).is_err());
        assert!(program(DriverKind::ClickHouse).is_err());
        assert!(program(DriverKind::Libsql).is_err());
        assert!(program(DriverKind::Mongo).is_err());
    }

    #[test]
    fn redis_cli_takes_the_password_from_the_environment() {
        let config = ConnectionConfig::from_url("rediss://app:s3cret@h:6380/2").unwrap();
        let (args, env) = invocation(&config, "h", Some(6380));
        assert_eq!(
            args,
            ["-h", "h", "-p", "6380", "-n", "2", "--user", "app", "--tls"]
        );
        assert_eq!(env, [("REDISCLI_AUTH", "s3cret".to_owned())]);
        assert!(!args.iter().any(|a| a.contains("s3cret")));
        // A client certificate goes to redis-cli as it goes to the driver.
        let mut config = config;
        config.ssl_client_cert = Some("c.pem".into());
        config.ssl_client_key = Some("k.pem".into());
        let (args, _) = invocation(&config, "h", None);
        assert!(args.ends_with(&["--tls", "--cert", "c.pem", "--key", "k.pem"].map(String::from)));
    }
}
