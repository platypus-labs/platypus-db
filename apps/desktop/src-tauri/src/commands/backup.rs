//! Backup and restore of a database with its own tools found on the PATH: `pg_dump` and
//! `pg_restore` (or `psql` for a plain SQL file) for PostgreSQL, `mysqldump` and `mysql`
//! for MySQL and MariaDB. They run with the settings and password of the session, through
//! its tunnel or proxy, and report their progress: table by table from the tools' verbose
//! output, or by the bytes of a SQL file sent to the client.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use platypus_core::{Connection, ConnectionConfig, DriverKind, ObjectKind};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;

use super::cli::{Invocation, mysql_connection, pg_connection, reach, spawn, stopped};
use super::{AppState, lock};
use crate::error::{CommandError, CommandResult};

/// How a PostgreSQL backup is written; MySQL's is always SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackupFormat {
    /// `pg_dump`'s compressed archive, restored with `pg_restore` (`.dump`).
    Custom,
    /// SQL statements (`.sql`).
    Plain,
}

#[derive(Debug, Deserialize)]
pub struct BackupRequest {
    pub path: PathBuf,
    pub format: BackupFormat,
    /// Only this schema (PostgreSQL); the whole database otherwise.
    #[serde(default)]
    pub schema: Option<String>,
    /// The structure without the rows.
    #[serde(default)]
    pub structure_only: bool,
}

#[derive(Debug, Deserialize)]
pub struct RestoreRequest {
    pub path: PathBuf,
    /// Drops the archive's objects before creating them again (`pg_restore --clean`).
    #[serde(default)]
    pub clean: bool,
}

/// How far a backup or restore is: tables done of `total`, or bytes of the file sent.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackupEvent {
    Progress {
        /// What the tool is doing, as it says it.
        step: String,
        done: u64,
        /// `None` while the count is unknown.
        total: Option<u64>,
        /// `done` and `total` are bytes, not tables.
        bytes: bool,
    },
    Finished {
        /// The size of the backup file, or of the file restored.
        bytes: u64,
        elapsed_ms: u64,
        /// The tool's own report, e.g. the errors `pg_restore` went past.
        output: String,
    },
    Failed {
        message: String,
        output: String,
    },
}

/// The lines of the tools' verbose output kept for the report.
const KEPT_LINES: usize = 200;

/// The table a verbose line of a tool starts on, if it is one.
fn table_of(line: &str) -> Option<&str> {
    const STARTS: [&str; 3] = [
        "pg_dump: dumping contents of table ",
        "pg_restore: processing data for table ",
        "-- Retrieving table structure for table ",
    ];
    STARTS.iter().find_map(|start| {
        line.strip_prefix(start)
            .map(|rest| rest.trim_end_matches("...").trim_matches('"'))
    })
}

/// The number of tables whose data an archive of `pg_restore -l` lists.
fn archive_tables(listing: &str) -> u64 {
    listing
        .lines()
        .filter(|l| !l.starts_with(';') && l.contains(" TABLE DATA "))
        .count() as u64
}

/// The tables of the database (or of `schema`) the backup goes through, to count it down.
async fn table_count(
    conn: &dyn Connection,
    config: &ConnectionConfig,
    schema: Option<&str>,
) -> Option<u64> {
    let schemas = conn.schemas().await.ok()?;
    let mut total = 0;
    for s in schemas.iter().filter(|s| match schema {
        Some(name) => s.name == name,
        // MySQL's schemas are its databases: the backup is of one.
        None if config.driver == DriverKind::MySql => s.name == config.database,
        None => !s.system,
    }) {
        let objects = conn.objects(&s.name).await.ok()?;
        total += objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Table)
            .count() as u64;
    }
    Some(total)
}

/// The tool, its arguments and environment for a backup of `config` reached at `host:port`.
fn backup_invocation(
    config: &ConnectionConfig,
    host: &str,
    port: Option<u16>,
    request: &BackupRequest,
) -> CommandResult<(&'static str, Invocation)> {
    let file = request.path.display().to_string();
    match config.driver {
        DriverKind::Postgres => {
            let (mut args, env) = pg_connection(config, host, port);
            let format = match request.format {
                BackupFormat::Custom => "--format=custom",
                BackupFormat::Plain => "--format=plain",
            };
            args.extend(["-w", "-v", format, "--file", &file].map(str::to_owned));
            if let Some(schema) = &request.schema {
                args.extend(["--schema".to_owned(), schema.clone()]);
            }
            if request.structure_only {
                args.push("--schema-only".to_owned());
            }
            Ok(("pg_dump", (args, env)))
        }
        DriverKind::MySql => {
            if config.database.is_empty() {
                return Err(CommandError::new(
                    "invalid",
                    "choose the database to back up in the connection",
                ));
            }
            let (mut args, env) = mysql_connection(config, host, port);
            // A consistent snapshot without locking InnoDB tables; routines, triggers and
            // events with the tables.
            args.extend(
                [
                    "--verbose",
                    "--single-transaction",
                    "--routines",
                    "--triggers",
                    "--events",
                    "--default-character-set=utf8mb4",
                ]
                .map(str::to_owned),
            );
            args.push(format!("--result-file={file}"));
            if request.structure_only {
                args.push("--no-data".to_owned());
            }
            args.push(config.database.clone());
            Ok(("mysqldump", (args, env)))
        }
        other => Err(CommandError::new(
            "unsupported",
            format!("backups are not available for {}", other.name()),
        )),
    }
}

/// Whether `path` is an archive of `pg_dump --format=custom`.
async fn is_archive(path: &Path) -> std::io::Result<bool> {
    let mut head = [0u8; 5];
    let mut file = tokio::fs::File::open(path).await?;
    let n = file.read(&mut head).await?;
    Ok(&head[..n] == b"PGDMP")
}

/// A line of a file sent as it is.
fn as_it_is(line: &[u8]) -> Cow<'_, [u8]> {
    Cow::Borrowed(line)
}

/// How a file is restored: a tool that reads it, or a client whose input it is, each line
/// through a filter.
enum Restore {
    Reads(&'static str, Invocation),
    Input(&'static str, Invocation, fn(&[u8]) -> Cow<'_, [u8]>),
}

async fn restore_invocation(
    config: &ConnectionConfig,
    host: &str,
    port: Option<u16>,
    request: &RestoreRequest,
) -> CommandResult<Restore> {
    let file = request.path.display().to_string();
    match config.driver {
        DriverKind::Postgres => {
            let archive = is_archive(&request.path)
                .await
                .map_err(|e| CommandError::new("io", format!("{file}: {e}")))?;
            let (mut args, env) = pg_connection(config, host, port);
            if archive {
                // Objects owned by roles of another server are made by the user restoring.
                args.extend(["-w", "-v", "--no-owner", "--exit-on-error"].map(str::to_owned));
                if request.clean {
                    args.extend(["--clean", "--if-exists"].map(str::to_owned));
                }
                args.push(file);
                Ok(Restore::Reads("pg_restore", (args, env)))
            } else {
                // All or nothing: the first error rolls the file back.
                args.extend(
                    [
                        "-X",
                        "-w",
                        "-q",
                        "-v",
                        "ON_ERROR_STOP=1",
                        "--single-transaction",
                    ]
                    .map(str::to_owned),
                );
                Ok(Restore::Input("psql", (args, env), as_it_is))
            }
        }
        DriverKind::MySql => {
            let (mut args, env) = mysql_connection(config, host, port);
            args.push("--default-character-set=utf8mb4".to_owned());
            if !config.database.is_empty() {
                args.push(config.database.clone());
            }
            Ok(Restore::Input("mysql", (args, env), without_definer))
        }
        other => Err(CommandError::new(
            "unsupported",
            format!("restoring is not available for {}", other.name()),
        )),
    }
}

/// Reads the tool's output line by line, counting the tables it starts on; returns the last
/// lines it wrote.
async fn follow(child: &mut Child, total: Option<u64>, events: &Channel<BackupEvent>) -> String {
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return String::new();
    };
    // The tools write their progress on stderr; `psql` and `mysql` their results on stdout.
    let mut lines = futures::stream::select(Box::pin(lines_of(stdout)), Box::pin(lines_of(stderr)));
    let mut kept = VecDeque::new();
    let mut done = 0;
    while let Some(line) = lines.next().await {
        if let Some(table) = table_of(&line) {
            done += 1;
            let _ = events.send(BackupEvent::Progress {
                step: table.to_owned(),
                done,
                total,
                bytes: false,
            });
        }
        if kept.len() == KEPT_LINES {
            kept.pop_front();
        }
        kept.push_back(line);
    }
    Vec::from(kept).join("\n")
}

/// The lines `reader` gives, to its end; read as bytes, so that text in another encoding
/// (a table's data echoed by a client) does not stop the reading and block the tool.
fn lines_of(
    reader: impl tokio::io::AsyncRead + Unpin + Send + 'static,
) -> impl futures::Stream<Item = String> + Send {
    futures::stream::unfold(BufReader::new(reader), |mut reader| async move {
        let mut line = Vec::new();
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => None,
            Ok(_) => {
                let text = String::from_utf8_lossy(&line);
                Some((text.trim_end_matches(['\r', '\n']).to_owned(), reader))
            }
        }
    })
}

/// `line` of a MySQL dump without its ``DEFINER=`user`@`host` `` clauses: views, triggers and
/// routines are then owned by the user restoring, as `pg_restore --no-owner` does, instead
/// of needing the right to create them for another (`SYSTEM_USER`, `SET_USER_ID`). Rows are
/// left as they are.
fn without_definer(line: &[u8]) -> Cow<'_, [u8]> {
    const DEFINER: &[u8] = b"DEFINER=";
    if line.starts_with(b"INSERT INTO") || !line.windows(DEFINER.len()).any(|w| w == DEFINER) {
        return Cow::Borrowed(line);
    }
    // A quoted name: `x` or 'x', its quote doubled inside.
    let quoted = |at: usize| -> Option<usize> {
        let quote = *line.get(at)?;
        if quote != b'`' && quote != b'\'' {
            return None;
        }
        let mut i = at + 1;
        loop {
            match *line.get(i)? {
                c if c == quote && line.get(i + 1) == Some(&quote) => i += 2,
                c if c == quote => return Some(i + 1),
                _ => i += 1,
            }
        }
    };
    let mut out = Vec::with_capacity(line.len());
    let mut i = 0;
    while i < line.len() {
        if line[i..].starts_with(DEFINER) {
            let host_end = quoted(i + DEFINER.len())
                .filter(|&e| line.get(e) == Some(&b'@'))
                .and_then(|e| {
                    quoted(e + 1).or_else(|| line[e + 1..].starts_with(b"%").then_some(e + 2))
                });
            if let Some(end) = host_end {
                i = end;
                if line.get(i) == Some(&b' ') {
                    i += 1;
                }
                continue;
            }
        }
        out.push(line[i]);
        i += 1;
    }
    Cow::Owned(out)
}

/// Sends `path` to the client's input, a line at a time through `filter`, reporting the
/// bytes read every so often. A file that cannot be read to its end says why on the
/// returned receiver, and the input stays open: closed, the client would take the part it
/// got for the whole file (`psql --single-transaction` would commit it).
async fn send_file(
    child: &mut Child,
    path: PathBuf,
    filter: fn(&[u8]) -> Cow<'_, [u8]>,
    events: Channel<BackupEvent>,
) -> CommandResult<(
    tauri::async_runtime::JoinHandle<()>,
    oneshot::Receiver<String>,
)> {
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|e| CommandError::new("io", format!("{}: {e}", path.display())))?;
    let total = file.metadata().await.map(|m| m.len()).ok();
    let mut file = BufReader::with_capacity(64 * 1024, file);
    let Some(mut stdin) = child.stdin.take() else {
        return Err(CommandError::new("cli", "the client has no input"));
    };
    let (failed, read_failed) = oneshot::channel();
    let sending = tauri::async_runtime::spawn(async move {
        let mut line = Vec::new();
        let mut sent = 0;
        let mut told = Instant::now();
        loop {
            let n = match file.read_until(b'\n', &mut line).await {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    let _ = failed.send(format!("{}: {e}", path.display()));
                    // Held open until the client is stopped.
                    return std::future::pending::<()>().await;
                }
            };
            // A client that stops reading ended: its output says why.
            if stdin.write_all(&filter(&line)).await.is_err() {
                break;
            }
            line.clear();
            sent += n as u64;
            if told.elapsed() >= Duration::from_millis(100) || Some(sent) == total {
                told = Instant::now();
                let _ = events.send(BackupEvent::Progress {
                    step: String::new(),
                    done: sent,
                    total,
                    bytes: true,
                });
            }
        }
        // Closing the input ends the client's script.
        drop(stdin);
    });
    Ok((sending, read_failed))
}

/// Waits for the tool to end while its output is followed, with its exit status (`None`
/// when it could not be had, which is a failure); `None` once `stop` ends first, which kills
/// the tool.
async fn finish(
    mut child: Child,
    total: Option<u64>,
    events: &Channel<BackupEvent>,
    stop: impl Future<Output = ()>,
) -> Option<(Option<std::process::ExitStatus>, String)> {
    let run = async {
        let output = follow(&mut child, total, events).await;
        let status = child.wait().await;
        (status, output)
    };
    tokio::select! {
        (status, output) = run => match status {
            Ok(status) => Some((Some(status), output)),
            Err(e) => Some((None, format!("{output}\n{e}"))),
        },
        () = stop => None,
    }
}

/// Ends when the `cancel_cli` of the run asks, not when nothing can ask any more.
async fn cancelled(cancel: oneshot::Receiver<()>) {
    if cancel.await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// The event of a tool that ended: the backup file's size, or why it failed with the end of
/// its output.
fn outcome(
    program: &str,
    status: Option<std::process::ExitStatus>,
    output: String,
    file: &Path,
    started: Instant,
) -> BackupEvent {
    if status.is_some_and(|s| s.success()) {
        BackupEvent::Finished {
            bytes: std::fs::metadata(file).map_or(0, |m| m.len()),
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            output,
        }
    } else {
        // The last line that names an error, or the last one.
        let message = output
            .lines()
            .rev()
            .find(|l| l.to_lowercase().contains("error"))
            .or_else(|| output.lines().next_back())
            .map_or_else(
                || match status {
                    Some(status) => format!("{program} failed ({status})"),
                    None => format!("{program} ended without saying how"),
                },
                str::to_owned,
            );
        BackupEvent::Failed { message, output }
    }
}

/// Backs up the session's database to `request.path` with `pg_dump` or `mysqldump`; progress
/// and the outcome arrive on `on_event`. `cancel_cli` with `run_id` stops it.
#[tauri::command]
pub async fn backup_database(
    state: State<'_, AppState>,
    session_id: u64,
    run_id: u64,
    request: BackupRequest,
    on_event: Channel<BackupEvent>,
) -> CommandResult<()> {
    state.exportable(session_id)?;
    let config = state.session(session_id)?.config;
    let conn = state.live(session_id).await?;
    let total = table_count(conn.as_ref(), &config, request.schema.as_deref()).await;
    let (stop, cancel) = oneshot::channel();
    lock(&state.clis).insert(run_id, stop);
    let result = backup(&config, &request, total, &on_event, cancel).await;
    lock(&state.clis).remove(&run_id);
    let _ = on_event.send(result?);
    Ok(())
}

/// Backs up `config`'s database as `request` asks, sending its progress on `events` (`total`
/// tables when known); returns how it ended, or an error when the tool could not start or
/// `cancel` stopped it (the file cut short then removed).
pub(crate) async fn backup(
    config: &ConnectionConfig,
    request: &BackupRequest,
    total: Option<u64>,
    events: &Channel<BackupEvent>,
    cancel: oneshot::Receiver<()>,
) -> CommandResult<BackupEvent> {
    let (tunnel, host, port) = reach(config).await?;
    let (program, invocation) = backup_invocation(config, &host, port, request)?;
    let started = Instant::now();
    let child = spawn(program, invocation, false)?;
    let ended = finish(child, total, events, cancelled(cancel)).await;
    drop(tunnel);
    // A file cut short is no backup: where a scheduled task keeps its newest files, it
    // would count as one.
    let Some((status, output)) = ended else {
        let _ = std::fs::remove_file(&request.path);
        return Err(stopped(program));
    };
    let event = outcome(program, status, output, &request.path, started);
    if matches!(event, BackupEvent::Failed { .. }) {
        let _ = std::fs::remove_file(&request.path);
    }
    Ok(event)
}

/// Restores `request.path` into the session's database: a `pg_dump` archive with
/// `pg_restore`, a SQL file through `psql` or `mysql`; not on a read-only connection. Progress
/// and the outcome arrive on `on_event`; `cancel_cli` with `run_id` stops it.
#[tauri::command]
pub async fn restore_database(
    state: State<'_, AppState>,
    session_id: u64,
    run_id: u64,
    request: RestoreRequest,
    on_event: Channel<BackupEvent>,
) -> CommandResult<()> {
    state.writable(session_id)?;
    let config = state.session(session_id)?.config;
    let (stop, cancel) = oneshot::channel();
    lock(&state.clis).insert(run_id, stop);
    let result = async {
        let (tunnel, host, port) = reach(&config).await?;
        let started = Instant::now();
        let (program, child, total, sending, read_failed) =
            match restore_invocation(&config, &host, port, &request).await? {
                Restore::Reads(program, invocation) => {
                    // The archive's table of contents counts its tables.
                    let listing = Command::new(program)
                        .arg("-l")
                        .arg(&request.path)
                        .output()
                        .await
                        .ok()
                        .filter(|o| o.status.success());
                    let total =
                        listing.map(|o| archive_tables(&String::from_utf8_lossy(&o.stdout)));
                    (program, spawn(program, invocation, false)?, total, None, None)
                }
                Restore::Input(program, invocation, filter) => {
                    let mut child = spawn(program, invocation, true)?;
                    let (sending, read_failed) =
                        send_file(&mut child, request.path.clone(), filter, on_event.clone())
                            .await?;
                    (program, child, None, Some(sending), Some(read_failed))
                }
            };
        // Stopped by Cancel, or by a file that cannot be read to its end.
        let unread = std::sync::Mutex::new(None);
        let stop = async {
            let unreadable = async {
                match read_failed {
                    Some(failed) => match failed.await {
                        Ok(why) => why,
                        Err(_) => std::future::pending().await,
                    },
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                () = cancelled(cancel) => {}
                why = unreadable => *unread.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(why),
            }
        };
        let ended = finish(child, total, &on_event, stop).await;
        if let Some(sending) = sending {
            sending.abort();
        }
        drop(tunnel);
        let Some((status, output)) = ended else {
            return Err(match unread.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner) {
                Some(why) => CommandError::new("io", format!("the restore stopped: {why}")),
                None => stopped(program),
            });
        };
        let _ = on_event.send(outcome(program, status, output, &request.path, started));
        Ok(())
    }
    .await;
    lock(&state.clis).remove(&run_id);
    result
}

#[cfg(test)]
mod tests {
    use platypus_core::Secret;

    use super::*;

    #[test]
    fn reads_the_tables_of_the_verbose_output() {
        assert_eq!(
            table_of(r#"pg_dump: dumping contents of table "public.orders""#),
            Some("public.orders")
        );
        assert_eq!(
            table_of(r#"pg_restore: processing data for table "demo.items""#),
            Some("demo.items")
        );
        assert_eq!(
            table_of("-- Retrieving table structure for table customers..."),
            Some("customers")
        );
        assert_eq!(table_of("pg_dump: reading schemas"), None);
    }

    #[test]
    fn leaves_the_definers_out_of_a_mysql_dump() {
        let strip =
            |line: &str| String::from_utf8(without_definer(line.as_bytes()).into_owned()).unwrap();
        assert_eq!(
            strip("/*!50013 DEFINER=`root`@`%` SQL SECURITY DEFINER */"),
            "/*!50013 SQL SECURITY DEFINER */"
        );
        assert_eq!(
            strip("/*!50003 CREATE*/ /*!50017 DEFINER=`ad``min`@`localhost`*/ /*!50003 TRIGGER t"),
            "/*!50003 CREATE*/ /*!50017 */ /*!50003 TRIGGER t"
        );
        assert_eq!(
            strip("CREATE DEFINER='app'@'10.%' PROCEDURE p()"),
            "CREATE PROCEDURE p()"
        );
        // Rows, and text that only looks like a clause, stay.
        let row = "INSERT INTO `t` VALUES (1,'DEFINER=`root`@`%`');";
        assert_eq!(strip(row), row);
        assert_eq!(strip("-- DEFINER= nothing"), "-- DEFINER= nothing");
    }

    #[test]
    fn counts_the_tables_of_an_archive() {
        let listing = ";\n; Archive created at 2026-10-29\n\
            215; 1259 16390 TABLE public orders platypus\n\
            3390; 0 16390 TABLE DATA public orders platypus\n\
            3391; 0 16399 TABLE DATA public items platypus\n";
        assert_eq!(archive_tables(listing), 2);
    }

    #[test]
    fn backs_up_with_the_password_in_the_environment() {
        let mut config = ConnectionConfig::new(DriverKind::Postgres);
        config.user = "app".to_owned();
        config.database = "shop".to_owned();
        config.password = Some(Secret::new("s3cret"));
        let request = BackupRequest {
            path: PathBuf::from("/tmp/shop.dump"),
            format: BackupFormat::Custom,
            schema: Some("sales".to_owned()),
            structure_only: true,
        };
        let (program, (args, env)) =
            backup_invocation(&config, "db", Some(5433), &request).unwrap();
        assert_eq!(program, "pg_dump");
        for pair in [
            ["-d", "shop"],
            ["-p", "5433"],
            ["--schema", "sales"],
            ["--file", "/tmp/shop.dump"],
        ] {
            assert!(args.windows(2).any(|w| w == pair), "{pair:?} in {args:?}");
        }
        assert!(args.contains(&"--format=custom".to_owned()));
        assert!(args.contains(&"--schema-only".to_owned()));
        assert!(!args.iter().any(|a| a.contains("s3cret")));
        assert!(env.contains(&("PGPASSWORD", "s3cret".to_owned())));

        config.driver = DriverKind::MySql;
        let (program, (args, env)) = backup_invocation(&config, "db", None, &request).unwrap();
        assert_eq!(program, "mysqldump");
        assert_eq!(args.last().map(String::as_str), Some("shop"));
        assert!(args.contains(&"--result-file=/tmp/shop.dump".to_owned()));
        assert!(args.contains(&"--no-data".to_owned()));
        assert!(env.contains(&("MYSQL_PWD", "s3cret".to_owned())));

        config.database = String::new();
        assert!(backup_invocation(&config, "db", None, &request).is_err());
        config.driver = DriverKind::Sqlite;
        assert!(backup_invocation(&config, "db", None, &request).is_err());
    }

    #[tokio::test]
    async fn restores_an_archive_with_pg_restore_and_sql_through_the_client() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("shop.dump");
        std::fs::write(&archive, b"PGDMP\x01\x10").unwrap();
        let sql = dir.path().join("shop.sql");
        std::fs::write(&sql, "CREATE TABLE t (id int);").unwrap();
        let mut config = ConnectionConfig::new(DriverKind::Postgres);
        config.database = "shop".to_owned();
        let request = |path: &Path, clean| RestoreRequest {
            path: path.to_owned(),
            clean,
        };
        let Restore::Reads(program, (args, _)) =
            restore_invocation(&config, "db", None, &request(&archive, true))
                .await
                .unwrap()
        else {
            panic!("an archive is read by pg_restore");
        };
        assert_eq!(program, "pg_restore");
        assert!(args.contains(&"--clean".to_owned()));
        assert_eq!(args.last(), Some(&archive.display().to_string()));
        let Restore::Input(program, (args, _), _) =
            restore_invocation(&config, "db", None, &request(&sql, false))
                .await
                .unwrap()
        else {
            panic!("SQL goes to psql");
        };
        assert_eq!(program, "psql");
        assert!(args.contains(&"--single-transaction".to_owned()));
        config.driver = DriverKind::MySql;
        assert!(matches!(
            restore_invocation(&config, "db", None, &request(&sql, false))
                .await
                .unwrap(),
            Restore::Input("mysql", _, _)
        ));
    }

    /// The events sent on a channel, as JSON.
    fn recorder() -> (
        Channel<BackupEvent>,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&events);
        let channel = Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(json) = body {
                sink.lock().unwrap().push(json);
            }
            Ok(())
        });
        (channel, events)
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

    fn mysql(sql: &str) -> String {
        let out = std::process::Command::new("mysql")
            .args([
                "-h",
                "127.0.0.1",
                "--port=53306",
                "-u",
                "platypus",
                "-N",
                "-B",
                "platypus",
                "-e",
                sql,
            ])
            .env("MYSQL_PWD", "platypus")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// A backup of the dev MySQL with mysqldump, table by table, then the table dropped and
    /// restored from the file through `mysql`, by the bytes sent.
    #[tokio::test]
    async fn backs_up_and_restores_mysql() {
        if !available("mysqldump", 53306) || !available("mysql", 53306) {
            return;
        }
        mysql(
            "DROP TABLE IF EXISTS backup_test; CREATE TABLE backup_test (id int PRIMARY KEY, v text); \
             INSERT INTO backup_test VALUES (1, 'one'), (2, 'two');",
        );
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("platypus.sql");
        let config =
            ConnectionConfig::from_url("mysql://platypus:platypus@127.0.0.1:53306/platypus")
                .unwrap();
        let request = BackupRequest {
            path: file.clone(),
            format: BackupFormat::Plain,
            schema: None,
            structure_only: false,
        };
        let (program, invocation) =
            backup_invocation(&config, "127.0.0.1", Some(53306), &request).unwrap();
        let (channel, events) = recorder();
        let (_stop, cancel) = oneshot::channel();
        let started = Instant::now();
        let child = spawn(program, invocation, false).unwrap();
        let (status, output) = finish(child, Some(1), &channel, cancelled(cancel))
            .await
            .unwrap();
        let event = outcome(program, status, output, &file, started);
        assert!(
            matches!(event, BackupEvent::Finished { bytes, .. } if bytes > 0),
            "{event:?}"
        );
        let dump = String::from_utf8_lossy(&std::fs::read(&file).unwrap()).into_owned();
        assert!(dump.contains("CREATE TABLE `backup_test`"), "{dump}");
        assert!(
            events
                .lock()
                .unwrap()
                .iter()
                .any(|e| e.contains("\"step\":\"backup_test\"")),
            "the table counts: {:?}",
            events.lock().unwrap()
        );

        // Restoring over the dev database's views, owned by root, would need SYSTEM_USER:
        // a file of the table and a view of root's, whose definer is left out.
        mysql("DROP TABLE backup_test");
        let file = dir.path().join("restore.sql");
        std::fs::write(
            &file,
            "CREATE TABLE backup_test (id int PRIMARY KEY, v text);\n\
             INSERT INTO backup_test VALUES (1,'one'),(2,'two');\n\
             /*!50001 DROP VIEW IF EXISTS `backup_test_v`*/;\n\
             /*!50001 CREATE ALGORITHM=UNDEFINED */\n\
             /*!50013 DEFINER=`root`@`%` SQL SECURITY DEFINER */\n\
             /*!50001 VIEW `backup_test_v` AS select `v` AS `v` from `backup_test` */;\n",
        )
        .unwrap();
        let restore = RestoreRequest {
            path: file.clone(),
            clean: false,
        };
        let Restore::Input(program, invocation, filter) =
            restore_invocation(&config, "127.0.0.1", Some(53306), &restore)
                .await
                .unwrap()
        else {
            panic!("SQL goes to the client");
        };
        let (channel, events) = recorder();
        let mut child = spawn(program, invocation, true).unwrap();
        let (sending, _) = send_file(&mut child, file.clone(), filter, channel.clone())
            .await
            .unwrap();
        let (_stop, cancel) = oneshot::channel();
        let (status, output) = finish(child, None, &channel, cancelled(cancel))
            .await
            .unwrap();
        sending.abort();
        assert!(status.is_some_and(|s| s.success()), "{output}");
        assert_eq!(
            mysql("SELECT group_concat(v ORDER BY v) FROM backup_test_v"),
            "one,two"
        );
        assert!(
            events
                .lock()
                .unwrap()
                .iter()
                .any(|e| e.contains("\"bytes\":true")),
            "{:?}",
            events.lock().unwrap()
        );
        mysql("DROP VIEW backup_test_v; DROP TABLE backup_test");
    }

    /// `pg_dump` against the dev PostgreSQL: an archive when the tool's version can dump the
    /// server, or the tool's own reason when it cannot (an older `pg_dump`).
    #[tokio::test]
    async fn backs_up_postgres_or_says_why_not() {
        if !available("pg_dump", 55432) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("platypus.dump");
        let config = ConnectionConfig::from_url(
            "postgres://platypus:platypus@127.0.0.1:55432/platypus?sslmode=disable",
        )
        .unwrap();
        let request = BackupRequest {
            path: file.clone(),
            format: BackupFormat::Custom,
            schema: Some("demo".to_owned()),
            structure_only: false,
        };
        let (program, invocation) =
            backup_invocation(&config, "127.0.0.1", Some(55432), &request).unwrap();
        let (channel, _events) = recorder();
        let (_stop, cancel) = oneshot::channel();
        let started = Instant::now();
        let child = spawn(program, invocation, false).unwrap();
        let (status, output) = finish(child, None, &channel, cancelled(cancel))
            .await
            .unwrap();
        match outcome(program, status, output, &file, started) {
            BackupEvent::Finished { .. } => assert!(is_archive(&file).await.unwrap()),
            BackupEvent::Failed { message, .. } => {
                assert!(message.contains("version"), "{message}");
            }
            BackupEvent::Progress { .. } => unreachable!(),
        }
    }

    #[tokio::test]
    async fn a_missing_tool_says_so() {
        let error = spawn("platypus-no-such-tool", (Vec::new(), Vec::new()), false).unwrap_err();
        assert!(error.message().contains("was not found"), "{error:?}");
    }
}
