//! The scheduled tasks' commands, and their runs: a loop checks every few seconds for the
//! tasks whose time has come, and runs each on a connection of its own, with the saved
//! password (a task cannot ask for one). A script runs its statements one at a time; an
//! export and a backup write a file named by the time of the run into the task's folder,
//! where the newest `keep` stay; a profile runs as `commands::profiles` runs it; a snapshot
//! stores the structure of a schema in the schema history.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use platypus_core::{Connection, DriverKind, ExecOptions, ExecResult};
use platypus_store::{SnapshotHow, SnapshotOutcome};
use platypus_transfer::dump::{DumpFile, dump};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::oneshot;
use uuid::Uuid;

use super::backup::{BackupEvent, BackupFormat, BackupRequest, backup};
use super::connections::{SavedConfig, saved_config};
use super::profiles::start as start_profile;
use super::schema_history::{SchemaRef, take};
use super::{AppState, connect, lock, read_only_error, sql};
use crate::error::{CommandError, CommandResult};
use crate::export::ExportEvent;
use crate::profiles::{ProfileTask, Profiles};
use crate::querylog::Logged;
use crate::scheduler::plan::{Run, Schedule, Task, surplus};
use crate::scheduler::{ScheduleView, Scheduler};

/// How often the loop looks for tasks whose time has come.
const TICK: Duration = Duration::from_secs(20);

/// The event the list listens to: a task changed, started or ended.
const CHANGED: &str = "schedules-changed";

/// Starts the loop that runs the tasks at their times, for as long as the app runs.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let due = app.state::<Scheduler>().take_due(Utc::now());
            for (schedule, attempt) in due {
                tauri::async_runtime::spawn(run(app.clone(), schedule, attempt, false));
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

/// Runs `schedule` as try `attempt`, unless it is running already, and records how it went.
async fn run(app: AppHandle, schedule: Schedule, attempt: u32, manual: bool) {
    let scheduler = app.state::<Scheduler>();
    if !scheduler.begin(schedule.id) {
        return;
    }
    let _ = app.emit(CHANGED, ());
    let started = Utc::now();
    let outcome = execute(
        &app.state::<AppState>(),
        &app.state::<Profiles>(),
        &schedule,
        started,
    )
    .await;
    let (ok, message, file) = match outcome {
        Ok((message, file)) => (true, message, file),
        Err(message) => (false, message, None),
    };
    scheduler.finish(
        &schedule,
        Run {
            started,
            finished: Utc::now(),
            ok,
            message,
            file,
            attempt,
            manual,
        },
    );
    let _ = app.emit(CHANGED, ());
}

/// What the task did, and the file it wrote; or why it failed.
async fn execute(
    state: &AppState,
    profiles: &Profiles,
    schedule: &Schedule,
    started: chrono::DateTime<Utc>,
) -> Result<(String, Option<PathBuf>), String> {
    // A profile opens the connections it names, and checks each.
    if let Task::Profile { profile_id } = &schedule.task {
        return run_profile(state, profiles, schedule, *profile_id, started).await;
    }
    let saved = unattended(state, schedule.connection_id, schedule.database.clone())
        .await
        .map_err(|e| e.message().to_owned())?;
    let label = format!("{} · {}", saved.label, schedule.name);
    // A snapshot keeps the structure, not the data, so a connection that bars exports allows it.
    if saved.restrictions.no_export
        && !matches!(schedule.task, Task::Script { .. } | Task::Snapshot { .. })
    {
        return Err(format!("{} does not let its data be exported", saved.label));
    }
    match &schedule.task {
        Task::Script { path } => run_script(state, &saved, &label, path)
            .await
            .map(|m| (m, None)),
        Task::Export { sql, format } => {
            if saved.read_only && !sql::only_reads(sql, saved.config.driver) {
                return Err(read_only_error(&saved.label).message().to_owned());
            }
            let format = format.transfer();
            let file = output(schedule, started, format.extension())?;
            let rows = export(state, saved, label, sql, &file, format).await?;
            prune(schedule, &file, format.extension());
            Ok((format!("{rows} rows written"), Some(file)))
        }
        Task::Backup { format, schema } => {
            let driver = saved.config.driver;
            let extension = if driver == DriverKind::Postgres && *format == BackupFormat::Custom {
                "dump"
            } else {
                "sql"
            };
            let file = output(schedule, started, extension)?;
            let message = if matches!(driver, DriverKind::Postgres | DriverKind::MySql) {
                let request = BackupRequest {
                    path: file.clone(),
                    format: *format,
                    schema: schema.clone(),
                    structure_only: false,
                };
                format!("{} written", size(run_backup(&saved, &request).await?))
            } else {
                dump_schema(state, &saved, &label, schema.as_deref(), &file).await?
            };
            prune(schedule, &file, extension);
            Ok((message, Some(file)))
        }
        Task::Snapshot { schema } => run_snapshot(state, &saved, schedule, schema)
            .await
            .map(|m| (m, None)),
        Task::Profile { .. } => unreachable!("run before"),
    }
}

/// The saved connection `connection_id`, on `database` when given, for a run nobody
/// attends: it needs a saved password, a password command or `.pgpass`.
pub(crate) async fn unattended(
    state: &AppState,
    connection_id: Uuid,
    database: Option<String>,
) -> CommandResult<SavedConfig> {
    with_saved_password(state, connection_id, database, true).await
}

/// [`unattended`] for a run of a profile started by hand (`scheduled` false) or by a task.
pub(crate) async fn with_saved_password(
    state: &AppState,
    connection_id: Uuid,
    database: Option<String>,
    scheduled: bool,
) -> CommandResult<SavedConfig> {
    let saved = saved_config(state, connection_id, None, database).await?;
    if saved.none_saved {
        return Err(password_missing(&saved.label, scheduled));
    }
    Ok(saved)
}

/// The refusal of a run that has no saved password to open `label` with.
fn password_missing(label: &str, scheduled: bool) -> CommandError {
    let message = if scheduled {
        "the password of the connection is not saved: a scheduled task needs a saved          password, a password command or .pgpass"
            .to_owned()
    } else {
        format!(
            "the password of {label} is not saved: open the profile in its dialog to run it,              or save the password"
        )
    };
    CommandError::new("auth", message)
}

/// Takes a snapshot of `schema` for its history; says whether it stored one.
async fn run_snapshot(
    state: &AppState,
    saved: &SavedConfig,
    schedule: &Schedule,
    schema: &str,
) -> Result<String, String> {
    let conn = connect(&saved.config).await.map_err(|e| e.to_string())?;
    let at = SchemaRef {
        connection: schedule.connection_id,
        database: schedule.database.as_deref(),
        schema,
    };
    let outcome = take(
        state,
        conn.as_ref(),
        saved.config.driver,
        at,
        None,
        SnapshotHow::Scheduled,
    )
    .await
    .map_err(|e| e.message().to_owned())?;
    Ok(match outcome {
        SnapshotOutcome::Taken(_) => "Snapshot taken".to_owned(),
        SnapshotOutcome::Unchanged(_) => "No changes since the last snapshot".to_owned(),
    })
}

/// Runs the profile `profile_id` for `schedule`: an export goes to the task's folder when it
/// has one, named by the time of the run, and the newest `keep` files stay.
async fn run_profile(
    state: &AppState,
    profiles: &Profiles,
    schedule: &Schedule,
    profile_id: Uuid,
    started: chrono::DateTime<Utc>,
) -> Result<(String, Option<PathBuf>), String> {
    let profile = profiles.restored(profile_id, state)?;
    let output = profile.task.scheduled_output(schedule, started)?;
    if let Some(output) = &output {
        // A file goes into a folder made if missing; the files of tables into a folder of theirs.
        let folder = if matches!(profile.task, ProfileTask::ExportTables { .. }) {
            Some(output.as_path())
        } else {
            output.parent()
        };
        if let Some(folder) = folder {
            std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        }
    }
    let (rows, warnings) = finished(state, |events| {
        start_profile(state, &profile, output.clone(), true, events)
    })
    .await?;
    if let Some(output) = &output
        && let Ok(Some(extension)) = profile.task.scheduled_extension()
    {
        prune(schedule, output, &extension);
    }
    let own_output = profile.task.own_output();
    let verb = if own_output.is_some() {
        "written"
    } else {
        "moved"
    };
    let left = if warnings.is_empty() {
        String::new()
    } else {
        format!("; left out: {}", warnings.join("; "))
    };
    let file = output.or_else(|| own_output.map(Path::to_owned));
    Ok((format!("{rows} rows {verb}{left}"), file))
}

/// Waits for the job `start` begins to end: the rows it moved and what it left out, or why
/// it failed. Nothing cancels it, nor listens to its progress.
pub(crate) async fn finished<F>(
    state: &AppState,
    start: impl FnOnce(Channel<ExportEvent>) -> F,
) -> Result<(u64, Vec<String>), String>
where
    F: Future<Output = CommandResult<u64>>,
{
    let (done, ended) = oneshot::channel::<Result<(u64, Vec<String>), String>>();
    let done = std::sync::Mutex::new(Some(done));
    let events = Channel::new(move |body| {
        if let tauri::ipc::InvokeResponseBody::Json(json) = body
            && let Ok(event) = serde_json::from_str::<serde_json::Value>(&json)
        {
            let outcome = match event["type"].as_str() {
                Some("done") => Some(Ok((
                    event["rows"].as_u64().unwrap_or(0),
                    event["warnings"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|w| w.as_str().map(str::to_owned))
                        .collect(),
                ))),
                Some("failed") => Some(Err(event["error"]["message"]
                    .as_str()
                    .unwrap_or("the job failed")
                    .to_owned())),
                Some("cancelled") => Some(Err("the job was cancelled".to_owned())),
                _ => None,
            };
            if let Some(outcome) = outcome
                && let Some(done) = done.lock().ok().and_then(|mut d| d.take())
            {
                let _ = done.send(outcome);
            }
        }
        Ok(())
    });
    let id = start(events).await.map_err(|e| e.message().to_owned())?;
    let outcome = ended
        .await
        .unwrap_or_else(|_| Err("the job ended without saying how".to_owned()));
    lock(&state.exports).remove(&id);
    outcome
}

/// Backs up the database as `request` asks; returns the size of the file.
async fn run_backup(saved: &SavedConfig, request: &BackupRequest) -> Result<u64, String> {
    // Nothing cancels a scheduled backup, nor listens to its progress.
    let (_never, cancel) = oneshot::channel();
    let event = backup(
        &saved.config,
        request,
        None,
        &Channel::new(|_| Ok(())),
        cancel,
    )
    .await
    .map_err(|e| e.message().to_owned())?;
    match event {
        BackupEvent::Finished { bytes, .. } => Ok(bytes),
        BackupEvent::Failed { message, .. } => Err(message),
        BackupEvent::Progress { .. } => Err("the backup did not end".to_owned()),
    }
}

/// The SQL dump of a schema (`schema`, or the connection's own), for the databases without
/// a backup tool of their own: its DDL and its rows, which run again with Run SQL file.
async fn dump_schema(
    state: &AppState,
    saved: &SavedConfig,
    label: &str,
    schema: Option<&str>,
    file: &Path,
) -> Result<String, String> {
    let raw: Arc<dyn Connection> =
        Arc::from(connect(&saved.config).await.map_err(|e| e.to_string())?);
    let conn = Logged::wrap(raw, &state.query_log, label);
    let dialect =
        platypus_sql::Dialect::of_server(saved.config.driver, &conn.server_info().product);
    let schema = if let Some(schema) = schema {
        schema.to_owned()
    } else {
        let schemas = conn.schemas().await.map_err(|e| e.to_string())?;
        schemas
            .iter()
            .find(|s| s.current)
            .or_else(|| schemas.iter().find(|s| !s.system))
            .map(|s| s.name.clone())
            .ok_or("the database has no schema to back up")?
    };
    let mut out = DumpFile::create(file).map_err(|e| format!("{}: {e}", file.display()))?;
    match dump((&*conn, dialect, &schema), &mut out, (true, None), |_| true).await {
        Ok(dumped) => {
            out.finish()
                .map_err(|e| format!("{}: {e}", file.display()))?;
            let left = if dumped.skipped.is_empty() {
                String::new()
            } else {
                format!("; left out: {}", dumped.skipped.join("; "))
            };
            Ok(format!(
                "{} tables and {} rows of {schema} written{left}",
                dumped.tables, dumped.rows
            ))
        }
        Err(e) => {
            out.discard();
            Err(e.to_string())
        }
    }
}

/// `bytes` in the largest unit under 1024 of them, as the app writes sizes.
fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    #[expect(
        clippy::cast_precision_loss,
        reason = "a size to read, not to compute with"
    )]
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    match unit {
        0 => format!("{bytes} B"),
        _ if value < 10.0 => format!("{value:.1} {}", UNITS[unit]),
        _ => format!("{value:.0} {}", UNITS[unit]),
    }
}

/// The file of a run started at `started`, its folder made if missing.
fn output(
    schedule: &Schedule,
    started: chrono::DateTime<Utc>,
    extension: &str,
) -> Result<PathBuf, String> {
    let file = schedule
        .file_at(started, extension)
        .ok_or("choose the folder the task's files go to")?;
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    Ok(file)
}

/// Deletes the task's files (or folders of files) beyond the newest `keep`, `written` never
/// among them.
fn prune(schedule: &Schedule, written: &Path, extension: &str) {
    let Some(folder) = written.parent() else {
        return;
    };
    for old in surplus(schedule, folder, extension) {
        if old != written {
            let _ = if old.is_dir() {
                std::fs::remove_dir_all(old)
            } else {
                std::fs::remove_file(old)
            };
        }
    }
}

/// Runs the statements of the SQL file at `path` one at a time, stopping at the first that
/// fails; on a read-only connection, only a file that reads.
async fn run_script(
    state: &AppState,
    saved: &SavedConfig,
    label: &str,
    path: &Path,
) -> Result<String, String> {
    let script = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if saved.read_only && !sql::only_reads(&script, saved.config.driver) {
        return Err(read_only_error(&saved.label).message().to_owned());
    }
    let raw: Arc<dyn Connection> =
        Arc::from(connect(&saved.config).await.map_err(|e| e.to_string())?);
    let conn = Logged::wrap(raw, &state.query_log, label);
    let dialect =
        platypus_sql::Dialect::of_server(saved.config.driver, &conn.server_info().product);
    let statements = platypus_sql::split(&script, dialect);
    let mut changed = 0;
    let mut count = 0;
    for (i, statement) in statements.iter().enumerate() {
        let text = statement.text(&script).trim();
        if text.is_empty() {
            continue;
        }
        match conn.execute(text, &ExecOptions::default()).await {
            Ok(ExecResult::Affected { rows }) => changed += rows,
            // The rows of a SELECT are not kept.
            Ok(ExecResult::Rows(_)) => {}
            Err(e) => return Err(format!("statement {}: {e}", i + 1)),
        }
        count += 1;
    }
    Ok(format!("{count} statements run, {changed} rows changed"))
}

/// Writes the rows of `sql` to `file`; returns how many.
async fn export(
    state: &AppState,
    saved: SavedConfig,
    label: String,
    sql: &str,
    file: &Path,
    format: platypus_transfer::Format,
) -> Result<u64, String> {
    let options = platypus_transfer::ExportOptions {
        format,
        driver: Some(saved.config.driver),
        ..platypus_transfer::ExportOptions::default()
    };
    let (rows, _) = finished(state, |events| async {
        Ok(super::export::start_query(
            state,
            (saved.config, label),
            (sql.to_owned(), file.to_owned(), options),
            events,
        ))
    })
    .await?;
    Ok(rows)
}

#[tauri::command]
pub fn list_schedules(scheduler: State<'_, Scheduler>) -> Vec<ScheduleView> {
    scheduler.list()
}

/// Saves a task, new or changed; it runs next at its first time from now.
#[tauri::command]
pub fn save_schedule(
    app: AppHandle,
    scheduler: State<'_, Scheduler>,
    profiles: State<'_, Profiles>,
    mut schedule: Schedule,
) -> CommandResult<Vec<ScheduleView>> {
    if let Task::Profile { profile_id } = schedule.task {
        profiles
            .follow(&mut schedule, profile_id)
            .map_err(|e| CommandError::new("invalid", e))?;
    }
    scheduler
        .put(schedule, Utc::now())
        .map_err(|e| CommandError::new("invalid", e))?;
    let _ = app.emit(CHANGED, ());
    Ok(scheduler.list())
}

#[tauri::command]
pub fn delete_schedule(
    app: AppHandle,
    scheduler: State<'_, Scheduler>,
    id: Uuid,
) -> CommandResult<Vec<ScheduleView>> {
    scheduler
        .delete(id)
        .map_err(|e| CommandError::new("io", e))?;
    let _ = app.emit(CHANGED, ());
    Ok(scheduler.list())
}

/// Runs a task now, apart from its times; its next time stays.
#[tauri::command]
pub fn run_schedule_now(
    app: AppHandle,
    scheduler: State<'_, Scheduler>,
    id: Uuid,
) -> CommandResult<()> {
    let schedule = scheduler
        .get(id)
        .ok_or_else(|| CommandError::new("not_found", "the task is gone"))?;
    tauri::async_runtime::spawn(run(app.clone(), schedule, 1, true));
    Ok(())
}

/// The IANA time zones a task can run in.
#[tauri::command]
pub fn time_zones() -> Vec<&'static str> {
    chrono_tz::TZ_VARIANTS.iter().map(|tz| tz.name()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::plan::{ExportFormat, Repeat};

    #[test]
    fn a_manual_run_without_a_password_does_not_talk_of_tasks() {
        let manual = password_missing("Shop", false);
        assert!(manual.message().contains("Shop"), "{}", manual.message());
        assert!(
            !manual.message().contains("scheduled"),
            "{}",
            manual.message()
        );
        assert!(
            password_missing("Shop", true)
                .message()
                .contains("scheduled task")
        );
    }

    fn schedule(task: Task, folder: Option<PathBuf>) -> Schedule {
        Schedule {
            id: Uuid::new_v4(),
            name: "Daily orders".to_owned(),
            enabled: true,
            connection_id: Uuid::nil(),
            database: None,
            task,
            repeat: Repeat::Day,
            time: "07:00".to_owned(),
            timezone: String::new(),
            folder,
            keep: 2,
            retries: 0,
            retry_minutes: 5,
        }
    }

    #[test]
    fn sizes_as_the_app_writes_them() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(1536), "1.5 KB");
        assert_eq!(size(48_734_208), "46 MB");
    }

    #[test]
    fn writes_into_the_folder_and_keeps_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("exports");
        let s = schedule(
            Task::Export {
                sql: "SELECT 1".to_owned(),
                format: ExportFormat::Csv,
            },
            Some(folder.clone()),
        );
        let first = output(&s, "2026-11-10T06:00:00Z".parse().unwrap(), "csv").unwrap();
        assert!(folder.is_dir(), "made when missing");
        assert_eq!(first.parent(), Some(folder.as_path()));
        let mut files = Vec::new();
        for day in 10..14 {
            let file = output(
                &s,
                format!("2026-11-{day}T06:00:00Z").parse().unwrap(),
                "csv",
            )
            .unwrap();
            std::fs::write(&file, "1").unwrap();
            prune(&s, &file, "csv");
            files.push(file);
        }
        let left: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        assert_eq!(left.len(), 2, "{left:?}");
        assert!(left.contains(&files[3]) && left.contains(&files[2]));

        let without = schedule(
            Task::Script {
                path: PathBuf::from("x.sql"),
            },
            None,
        );
        assert!(output(&without, Utc::now(), "csv").is_err());
    }

    /// A state whose store holds one SQLite connection on `db`; returns its id.
    fn sqlite_state(dir: &Path, db: &Path) -> (AppState, Uuid) {
        use platypus_store::{ConnectionStore, MemorySecrets, PasswordUpdate, SavedConnection};
        let mut store = ConnectionStore::open(
            dir.join("connections.toml"),
            Arc::new(MemorySecrets::default()),
        )
        .unwrap();
        let mut saved = SavedConnection::new("Scheduled", platypus_core::DriverKind::Sqlite);
        saved.database = db.display().to_string();
        let id = saved.id;
        store.save(saved, PasswordUpdate::Keep).unwrap();
        let state = AppState::new(
            (Ok(store), None),
            Err("no vault".to_owned()),
            Err("no history".to_owned()),
            Err("no catalog".to_owned()),
            Ok(platypus_store::SchemaHistoryStore::open_in_memory().unwrap()),
            Err("no settings".to_owned()),
            Err("no scripts".to_owned()),
        );
        (state, id)
    }

    /// The tasks run for real on a SQLite file: a script changes its rows, one that fails
    /// names the statement, and an export writes a CSV file into its folder.
    #[tokio::test]
    async fn runs_scripts_and_exports_on_sqlite() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("tasks.db");
        let (state, connection) = sqlite_state(dir.path(), &db);
        let profiles = Profiles::open(None);
        let saved = saved_config(&state, connection, None, None).await.unwrap();
        let conn = connect(&saved.config).await.unwrap();
        for sql in [
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v INTEGER)",
            "INSERT INTO t (v) VALUES (1), (2)",
        ] {
            conn.execute(sql, &ExecOptions::default()).await.unwrap();
        }
        drop(conn);

        let script = dir.path().join("bump.sql");
        std::fs::write(
            &script,
            "UPDATE t SET v = v + 10;\nUPDATE t SET v = 0 WHERE id = 2;\n",
        )
        .unwrap();
        let mut s = schedule(Task::Script { path: script }, None);
        s.connection_id = connection;
        let (message, file) = execute(&state, &profiles, &s, Utc::now()).await.unwrap();
        assert_eq!(message, "2 statements run, 3 rows changed");
        assert_eq!(file, None);

        let broken = dir.path().join("broken.sql");
        std::fs::write(&broken, "SELECT 1;\nUPDATE nothing SET v = 1;").unwrap();
        s.task = Task::Script { path: broken };
        let error = execute(&state, &profiles, &s, Utc::now())
            .await
            .unwrap_err();
        assert!(error.starts_with("statement 2: "), "{error}");

        let folder = dir.path().join("out");
        s.task = Task::Export {
            sql: "SELECT id, v FROM t ORDER BY id".to_owned(),
            format: ExportFormat::Csv,
        };
        s.folder = Some(folder.clone());
        let (message, file) = execute(&state, &profiles, &s, Utc::now()).await.unwrap();
        assert_eq!(message, "2 rows written");
        let file = file.unwrap();
        assert_eq!(file.parent(), Some(folder.as_path()));
        let csv = std::fs::read_to_string(&file).unwrap();
        assert!(csv.starts_with("id,v"), "{csv}");
        assert!(csv.contains("1,11") && csv.contains("2,0"), "{csv}");

        // SQLite has no backup tool of its own: the backup is the SQL dump of its schema.
        s.task = Task::Backup {
            format: BackupFormat::Custom,
            schema: None,
        };
        let (message, file) = execute(&state, &profiles, &s, Utc::now()).await.unwrap();
        assert_eq!(message, "1 tables and 2 rows of main written");
        let file = file.unwrap();
        assert_eq!(file.extension().and_then(|e| e.to_str()), Some("sql"));
        let sql = std::fs::read_to_string(&file).unwrap();
        assert!(
            sql.contains("CREATE TABLE") && sql.contains("INSERT INTO"),
            "{sql}"
        );
    }

    /// A scheduled profile runs for real on a SQLite file: its export goes into the task's
    /// folder, and a copy creates a table from another.
    #[tokio::test]
    async fn runs_profiles_on_sqlite() {
        use crate::profiles::{ProfileTask, TransferProfile};
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("profiles.db");
        let (state, connection) = sqlite_state(dir.path(), &db);
        let saved = saved_config(&state, connection, None, None).await.unwrap();
        let conn = connect(&saved.config).await.unwrap();
        for sql in [
            "CREATE TABLE t (id INTEGER PRIMARY KEY, v INTEGER)",
            "INSERT INTO t (v) VALUES (5), (6), (7)",
        ] {
            conn.execute(sql, &ExecOptions::default()).await.unwrap();
        }
        drop(conn);

        let profiles = Profiles::open(None);
        let save = |name: &str, task| {
            profiles
                .put(
                    TransferProfile {
                        id: Uuid::new_v4(),
                        name: name.to_owned(),
                        task,
                    },
                    |_| true,
                    &state,
                )
                .unwrap()
        };
        let export = save(
            "Export t",
            ProfileTask::ExportQuery {
                connection_id: connection,
                database: None,
                sql: "SELECT id, v FROM t ORDER BY id".to_owned(),
                path: dir.path().join("own.csv"),
                options: serde_json::Value::Null,
            },
        );
        let folder = dir.path().join("scheduled");
        let mut s = schedule(
            Task::Profile {
                profile_id: export.id,
            },
            Some(folder.clone()),
        );
        profiles.follow(&mut s, export.id).unwrap();
        let (message, file) = execute(&state, &profiles, &s, Utc::now()).await.unwrap();
        assert_eq!(message, "3 rows written");
        let file = file.unwrap();
        assert_eq!(file.parent(), Some(folder.as_path()));
        assert!(std::fs::read_to_string(&file).unwrap().contains("3,7"));
        assert!(!dir.path().join("own.csv").exists(), "the folder wins");
        // Without a folder the profile writes where it says.
        s.folder = None;
        let (_, file) = execute(&state, &profiles, &s, Utc::now()).await.unwrap();
        assert_eq!(file, Some(dir.path().join("own.csv")));

        // Another profile copies the table into a new one.
        let import = save(
            "Copy t",
            ProfileTask::TransferTable {
                source_connection_id: connection,
                source_database: None,
                target_connection_id: connection,
                target_database: None,
                plan: serde_json::json!({
                    "source_table": "t",
                    "table": "copy",
                    "create": true,
                    "columns": [
                        { "source": "id", "target": "id", "data_type": "INTEGER" },
                        { "source": "v", "target": "v", "data_type": "INTEGER" }
                    ]
                }),
            },
        );
        s.task = Task::Profile {
            profile_id: import.id,
        };
        let (message, file) = execute(&state, &profiles, &s, Utc::now()).await.unwrap();
        assert_eq!(message, "3 rows moved");
        assert_eq!(file, None);
        let conn = connect(&saved.config).await.unwrap();
        let ExecResult::Rows(rows) = conn
            .execute("SELECT SUM(v) FROM copy", &ExecOptions::default())
            .await
            .unwrap()
        else {
            panic!("rows");
        };
        let batches: Vec<_> = futures::TryStreamExt::try_collect(rows).await.unwrap();
        assert_eq!(batches[0].num_rows(), 1);

        // A profile that is gone fails the run with a message.
        s.task = Task::Profile {
            profile_id: Uuid::new_v4(),
        };
        let error = execute(&state, &profiles, &s, Utc::now())
            .await
            .unwrap_err();
        assert!(error.contains("gone"), "{error}");
    }

    /// A snapshot task stores the schema once, says so when nothing changed, and needs no
    /// folder.
    #[tokio::test]
    async fn snapshot_tasks_store_a_schema_when_it_changes() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("snap.db");
        let (state, connection) = sqlite_state(dir.path(), &db);
        let profiles = Profiles::open(None);
        let saved = saved_config(&state, connection, None, None).await.unwrap();
        let conn = connect(&saved.config).await.unwrap();
        conn.execute(
            "CREATE TABLE t (id INTEGER PRIMARY KEY)",
            &ExecOptions::default(),
        )
        .await
        .unwrap();
        let mut s = schedule(
            Task::Snapshot {
                schema: "main".to_owned(),
            },
            None,
        );
        s.connection_id = connection;
        assert_eq!(s.problem(), None);
        let run = || execute(&state, &profiles, &s, Utc::now());
        assert_eq!(run().await.unwrap(), ("Snapshot taken".to_owned(), None));
        assert_eq!(run().await.unwrap().0, "No changes since the last snapshot");
        conn.execute("ALTER TABLE t ADD COLUMN v TEXT", &ExecOptions::default())
            .await
            .unwrap();
        assert_eq!(run().await.unwrap().0, "Snapshot taken");
        let list = state
            .with_schema_history(|h| h.list(connection, None, "main"))
            .unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|m| m.how == SnapshotHow::Scheduled));
        assert_eq!(list[0].tables, 1);
    }
}
