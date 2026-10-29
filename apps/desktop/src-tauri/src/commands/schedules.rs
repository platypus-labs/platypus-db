//! The scheduled tasks' commands, and their runs: a loop checks every few seconds for the
//! tasks whose time has come, and runs each on a connection of its own, with the saved
//! password (a task cannot ask for one). A script runs its statements one at a time; an
//! export and a backup write a file named by the time of the run into the task's folder,
//! where the newest `keep` stay.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use platypus_core::{Connection, ExecOptions, ExecResult};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::oneshot;
use uuid::Uuid;

use super::backup::{BackupEvent, BackupFormat, BackupRequest, backup};
use super::connections::{SavedConfig, saved_config};
use super::{AppState, connect, read_only_error, sql};
use crate::error::{CommandError, CommandResult};
use crate::export::Export;
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
    let outcome = execute(&app.state::<AppState>(), &schedule, started).await;
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
    schedule: &Schedule,
    started: chrono::DateTime<Utc>,
) -> Result<(String, Option<PathBuf>), String> {
    let saved = saved_config(
        state,
        schedule.connection_id,
        None,
        schedule.database.clone(),
    )
    .await
    .map_err(|e| e.message().to_owned())?;
    if saved.none_saved {
        return Err(
            "the password of the connection is not saved: a scheduled task needs a saved \
             password, a password command or .pgpass"
                .to_owned(),
        );
    }
    let label = format!("{} · {}", saved.label, schedule.name);
    match &schedule.task {
        Task::Script { path } => run_script(state, &saved, &label, path)
            .await
            .map(|m| (m, None)),
        Task::Export { sql, format } => {
            let format = format.transfer();
            let file = output(schedule, started, format.extension())?;
            let rows = export(state, saved, label, sql, &file, format).await?;
            prune(schedule, &file, format.extension());
            Ok((format!("{rows} rows written"), Some(file)))
        }
        Task::Backup { format, schema } => {
            let extension = match format {
                BackupFormat::Custom => "dump",
                BackupFormat::Plain => "sql",
            };
            let file = output(schedule, started, extension)?;
            let request = BackupRequest {
                path: file.clone(),
                format: *format,
                schema: schema.clone(),
                structure_only: false,
            };
            // Nothing cancels a scheduled backup, nor listens to its progress.
            let (_never, cancel) = oneshot::channel();
            let event = backup(
                &saved.config,
                &request,
                None,
                &Channel::new(|_| Ok(())),
                cancel,
            )
            .await
            .map_err(|e| e.message().to_owned())?;
            match event {
                BackupEvent::Finished { bytes, .. } => {
                    prune(schedule, &file, extension);
                    Ok((format!("{} written", size(bytes)), Some(file)))
                }
                BackupEvent::Failed { message, .. } => Err(message),
                BackupEvent::Progress { .. } => Err("the backup did not end".to_owned()),
            }
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

/// Deletes the task's files beyond the newest `keep`, `written` never among them.
fn prune(schedule: &Schedule, written: &Path, extension: &str) {
    let Some(folder) = written.parent() else {
        return;
    };
    for old in surplus(schedule, folder, extension) {
        if old != written {
            let _ = std::fs::remove_file(old);
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
    let (done, ended) = oneshot::channel::<Result<u64, String>>();
    let done = std::sync::Mutex::new(Some(done));
    let events = Channel::new(move |body| {
        if let tauri::ipc::InvokeResponseBody::Json(json) = body
            && let Ok(event) = serde_json::from_str::<serde_json::Value>(&json)
        {
            let outcome = match event["kind"].as_str() {
                Some("done") => Some(Ok(event["rows"].as_u64().unwrap_or(0))),
                Some("failed") => Some(Err(event["error"]["message"]
                    .as_str()
                    .unwrap_or("the export failed")
                    .to_owned())),
                Some("cancelled") => Some(Err("the export was cancelled".to_owned())),
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
    let options = platypus_transfer::ExportOptions {
        format,
        driver: Some(saved.config.driver),
        ..platypus_transfer::ExportOptions::default()
    };
    let _job = Export::start(
        (saved.config, label, Arc::clone(&state.query_log)),
        sql.to_owned(),
        file.to_owned(),
        options,
        events,
    );
    ended
        .await
        .unwrap_or_else(|_| Err("the export ended without saying how".to_owned()))
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
    schedule: Schedule,
) -> CommandResult<Vec<ScheduleView>> {
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
}
