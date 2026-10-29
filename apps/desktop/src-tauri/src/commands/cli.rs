//! Runs a script with the database's own command-line client found on the PATH (`psql`,
//! `mysql`, `sqlcmd` or `sqlite3`; none for DuckDB), with the settings and password of the
//! editor's session, for scripts that rely on the client's own commands such as `\copy`,
//! `source` or `:r`.

use std::process::{Output, Stdio};
use std::time::Instant;

use platypus_core::{Auth, ConnectionConfig, DriverKind, SslMode};
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
        DriverKind::Snowflake
        | DriverKind::BigQuery
        | DriverKind::Databricks
        | DriverKind::Trino
        | DriverKind::Adbc
        | DriverKind::Oracle
        | DriverKind::Db2
        | DriverKind::Odbc => Err(CommandError::new(
            "cli",
            format!("terminal mode is not available for {}", driver.name()),
        )),
    }
}

/// Arguments and environment of a command-line tool.
pub(super) type Invocation = (Vec<String>, Vec<(&'static str, String)>);

/// What the PostgreSQL tools (`psql`, `pg_dump`, `pg_restore`) need to reach `config`'s
/// database at `host:port`: the password and TLS go in the environment.
pub(super) fn pg_connection(
    config: &ConnectionConfig,
    host: &str,
    port: Option<u16>,
) -> Invocation {
    let mut args: Vec<String> = [
        "-h",
        host,
        "-U",
        config.user.as_str(),
        "-d",
        config.database.as_str(),
    ]
    .map(str::to_owned)
    .into();
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
    let mut args: Vec<String> = ["-h", host, "-u", config.user.as_str()]
        .map(str::to_owned)
        .into();
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
                args.push(config.database.clone());
            }
            env = mysql_env;
        }
        DriverKind::Sqlite | DriverKind::DuckDb | DriverKind::Libsql => {
            args.extend(
                ["-bail", "-header", "-column", config.database.as_str()].map(str::to_owned),
            );
        }
        DriverKind::SqlServer => {
            args = sqlcmd_args(config, host, port);
            if matches!(config.auth(), Ok(Auth::Password)) {
                env.extend(password.map(|p| ("SQLCMDPASSWORD", p)));
            }
        }
        // No client: `program` refuses them.
        DriverKind::ClickHouse
        | DriverKind::Mongo
        | DriverKind::Snowflake
        | DriverKind::BigQuery
        | DriverKind::Databricks
        | DriverKind::Trino
        | DriverKind::Adbc
        | DriverKind::Oracle
        | DriverKind::Db2
        | DriverKind::Odbc => {}
    }
    (args, env)
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
/// dropping its wait drops the child, spawned with `kill_on_drop`.
async fn wait_or_cancel(
    child: Child,
    cancel: oneshot::Receiver<()>,
) -> Option<std::io::Result<Output>> {
    let wait = Box::pin(child.wait_with_output());
    match futures::future::select(wait, cancel).await {
        futures::future::Either::Left((out, _)) => Some(out),
        futures::future::Either::Right(_) => None,
    }
}

/// Runs `script` with the client of the session's database and waits for it to end;
/// `cancel_cli` with the same `run_id` stops it, also while the SSH tunnel opens.
#[tauri::command]
pub async fn run_cli(
    state: State<'_, AppState>,
    session_id: u64,
    run_id: u64,
    script: String,
) -> CommandResult<CliOutput> {
    state.may_run(session_id, &script)?;
    let (stop, cancel) = oneshot::channel();
    lock(&state.clis).insert(run_id, stop);
    let out = run(&state, session_id, script, cancel).await;
    lock(&state.clis).remove(&run_id);
    out
}

pub(super) fn stopped(program: &str) -> CommandError {
    CommandError::new("cancelled", format!("{program} was stopped"))
}

async fn run(
    state: &AppState,
    session_id: u64,
    script: String,
    mut cancel: oneshot::Receiver<()>,
) -> CommandResult<CliOutput> {
    let mut config = state.session(session_id)?.config;
    let program = program(config.driver)?;
    // The tunnel or proxy stays open while the client runs.
    let (tunnel, host, port) = reach(&mut config).await?;
    if cancel.try_recv().is_ok() {
        return Err(stopped(program));
    }
    let (args, env) = invocation(&config, &host, port);
    let started = Instant::now();
    let mut child = spawn(program, (args.clone(), env), true)?;
    if let Some(mut stdin) = child.stdin.take() {
        // Written alongside the reads, so a client that prints a lot cannot block the script;
        // one that exits early closes its input, and its output says why.
        tauri::async_runtime::spawn(async move {
            let _ = stdin.write_all(script.as_bytes()).await;
        });
    }
    let out = wait_or_cancel(child, cancel).await;
    drop(tunnel);
    let out = out
        .ok_or_else(|| stopped(program))?
        .map_err(|e| CommandError::new("cli", format!("{program}: {e}")))?;
    let mut output = String::from_utf8_lossy(&out.stdout).into_owned();
    output.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(CliOutput {
        command: format!("{program} {}", args.join(" ")),
        status: out.status.code(),
        output,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
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
        // `sort` waits for the end of its input, which stays open: it only ends when killed.
        let child = Command::new("sort")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("sort is on every PATH");
        let (stop, cancel) = oneshot::channel();
        stop.send(()).unwrap();
        let out = tauri::async_runtime::block_on(wait_or_cancel(child, cancel));
        assert!(out.is_none());
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
        assert_eq!(args.last().map(String::as_str), Some("shop"));
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
        assert!(env.is_empty());

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
}
