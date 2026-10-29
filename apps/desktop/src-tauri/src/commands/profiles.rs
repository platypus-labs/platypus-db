//! The commands of the saved transfer profiles, and their runs: a profile opens the
//! connections it names from the saved ones, with their saved passwords (a run from a
//! profile asks for none), checks what each allows, and starts the same export, import,
//! copy or migration the dialogs do, streaming the same events.

use std::path::PathBuf;

use platypus_core::Secret;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use super::connections::SavedConfig;
use super::schedules::unattended;
use super::{AppState, export, read_only_error, sql, transfer};
use crate::error::{CommandError, CommandResult};
use crate::export::ExportEvent;
use crate::profiles::{KeyStore, ProfileTask, Profiles, TransferProfile, import_plan};
use crate::scheduler::Scheduler;

/// The event the schedules' list listens to: a task changed.
const SCHEDULES_CHANGED: &str = "schedules-changed";

/// The bucket keys of the profiles live beside the connections' passwords.
impl KeyStore for AppState {
    fn get(&self, id: Uuid) -> Result<Option<Secret>, String> {
        self.with_store(|s| s.ssh_secret(id))
            .map_err(|e| e.message().to_owned())
    }

    fn set(&self, id: Uuid, key: Option<&Secret>) -> Result<(), String> {
        self.with_store(|s| s.set_ssh_secret(id, key))
            .map_err(|e| e.message().to_owned())
    }
}

/// The saved connection `id` on `database`, for a run that reads its data out: what the
/// sessions refuse (`exportable`) it refuses too.
async fn source(
    state: &AppState,
    id: Uuid,
    database: Option<String>,
) -> CommandResult<SavedConfig> {
    let saved = unattended(state, id, database).await?;
    if saved.restrictions.no_export {
        return Err(CommandError::new(
            "restricted",
            format!("{} does not let its data be exported", saved.label),
        ));
    }
    Ok(saved)
}

/// The saved connection `id` on `database`, for a run that writes into it: what the
/// sessions refuse (`editable`) it refuses too.
async fn target(
    state: &AppState,
    id: Uuid,
    database: Option<String>,
) -> CommandResult<SavedConfig> {
    let saved = unattended(state, id, database).await?;
    if saved.read_only {
        return Err(read_only_error(&saved.label));
    }
    if saved.restrictions.no_data_editing {
        return Err(CommandError::new(
            "restricted",
            format!("{} changes its rows only by SQL", saved.label),
        ));
    }
    Ok(saved)
}

/// The name a run of `profile` on `saved` bears in the query log.
fn log_label(saved: &SavedConfig, profile: &TransferProfile) -> String {
    format!("{} · {}", saved.label, profile.name)
}

/// Starts `profile` with its connections' checks; returns the id `cancel_export` takes.
/// `redirect` replaces the file (or folder) an export writes, for a scheduled run.
pub(crate) async fn start(
    state: &AppState,
    profile: &TransferProfile,
    redirect: Option<PathBuf>,
    on_event: Channel<ExportEvent>,
) -> CommandResult<u64> {
    let label = |saved: &SavedConfig| log_label(saved, profile);
    match &profile.task {
        ProfileTask::ExportQuery {
            connection_id,
            database,
            sql,
            path,
            ..
        } => {
            let saved = source(state, *connection_id, database.clone()).await?;
            if saved.read_only && !sql::only_reads(sql, saved.config.driver) {
                return Err(read_only_error(&saved.label));
            }
            let options = exporting(profile, saved.config.driver)?;
            let path = redirect.unwrap_or_else(|| path.clone());
            let name = label(&saved);
            Ok(export::start_query(
                state,
                (saved.config, name),
                (sql.clone(), path, options),
                on_event,
            ))
        }
        ProfileTask::ExportTables {
            connection_id,
            database,
            tables,
            dir,
            ..
        } => {
            let saved = source(state, *connection_id, database.clone()).await?;
            let options = exporting(profile, saved.config.driver)?;
            let dir = redirect.unwrap_or_else(|| dir.clone());
            let name = label(&saved);
            Ok(export::start_tables(
                state,
                (saved.config, name),
                (tables.clone(), dir, options),
                on_event,
            ))
        }
        _ => start_into(state, profile, on_event).await,
    }
}

/// [`start`] for the profiles that write into a database: an import, a copy, a migration.
async fn start_into(
    state: &AppState,
    profile: &TransferProfile,
    on_event: Channel<ExportEvent>,
) -> CommandResult<u64> {
    let label = |saved: &SavedConfig| log_label(saved, profile);
    let invalid = |e: String| CommandError::new("invalid", e);
    match &profile.task {
        ProfileTask::ImportFile {
            connection_id,
            database,
            path,
            plan,
            options,
        } => {
            let saved = target(state, *connection_id, database.clone()).await?;
            let plan = import_plan(plan).map_err(invalid)?;
            let name = label(&saved);
            Ok(transfer::start_file(
                state,
                (saved.config, name),
                (path.clone(), options.clone(), plan),
                on_event,
            ))
        }
        ProfileTask::TransferTable {
            source_connection_id,
            source_database,
            target_connection_id,
            target_database,
            plan,
        } => {
            let from = source(state, *source_connection_id, source_database.clone()).await?;
            let to = target(state, *target_connection_id, target_database.clone()).await?;
            let plan = import_plan(plan).map_err(invalid)?;
            let name = label(&to);
            Ok(transfer::start_copy(
                state,
                from.config,
                (to.config, name),
                plan,
                on_event,
            ))
        }
        ProfileTask::Migrate {
            source_connection_id,
            source_database,
            target_connection_id,
            target_database,
            source_schema,
            target_schema,
            tables,
            objects,
        } => {
            let from = source(state, *source_connection_id, source_database.clone()).await?;
            let to = target(state, *target_connection_id, target_database.clone()).await?;
            let name = label(&to);
            Ok(transfer::start_migrate(
                state,
                from.config,
                (to.config, name),
                (
                    source_schema.clone(),
                    target_schema.clone(),
                    tables.clone(),
                    *objects,
                ),
                on_event,
            ))
        }
        ProfileTask::ExportQuery { .. } | ProfileTask::ExportTables { .. } => Err(
            CommandError::new("invalid", "an export writes into no database"),
        ),
    }
}

/// The export options of `profile`, for a database of `driver` (the SQL format writes
/// statements for it).
fn exporting(
    profile: &TransferProfile,
    driver: platypus_core::DriverKind,
) -> CommandResult<platypus_transfer::ExportOptions> {
    let options = profile
        .task
        .export_options()
        .map_err(|e| CommandError::new("invalid", e))?
        .unwrap_or_default();
    Ok(platypus_transfer::ExportOptions {
        driver: options.driver.or(Some(driver)),
        ..options
    })
}

#[tauri::command]
pub fn list_transfer_profiles(profiles: State<'_, Profiles>) -> Vec<TransferProfile> {
    profiles.list()
}

/// Saves a profile, new or changed; returns it as kept, without the secret key of its
/// bucket. The tasks that run it follow its first connection.
#[tauri::command]
pub fn save_transfer_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    profiles: State<'_, Profiles>,
    scheduler: State<'_, Scheduler>,
    profile: TransferProfile,
) -> CommandResult<TransferProfile> {
    let known = |id| {
        state
            .with_store(|s| Ok(s.get(id).is_some()))
            .unwrap_or(false)
    };
    let saved = profiles
        .put(profile, known, &*state)
        .map_err(|e| CommandError::new("invalid", e))?;
    scheduler.follow_profile(saved.id, saved.task.first_connection());
    let _ = app.emit(SCHEDULES_CHANGED, ());
    Ok(saved)
}

/// Deletes a profile and its secret key, unless a scheduled task runs it.
#[tauri::command]
pub fn delete_transfer_profile(
    state: State<'_, AppState>,
    profiles: State<'_, Profiles>,
    scheduler: State<'_, Scheduler>,
    id: Uuid,
) -> CommandResult<Vec<TransferProfile>> {
    remove(&profiles, &scheduler, &*state, id)?;
    Ok(profiles.list())
}

/// [`delete_transfer_profile`] on its parts.
fn remove(
    profiles: &Profiles,
    scheduler: &Scheduler,
    keys: &dyn KeyStore,
    id: Uuid,
) -> CommandResult<()> {
    if let Some(task) = scheduler.using_profile(id) {
        return Err(CommandError::new(
            "in_use",
            format!("the scheduled task {task} runs this profile: delete or change the task first"),
        ));
    }
    profiles
        .delete(id, keys)
        .map_err(|e| CommandError::new("io", e))
}

/// Runs a profile as its dialog would: returns the id `cancel_export` takes; progress and
/// the outcome arrive on `on_event`.
#[tauri::command]
pub async fn run_transfer_profile(
    state: State<'_, AppState>,
    profiles: State<'_, Profiles>,
    id: Uuid,
    on_event: Channel<ExportEvent>,
) -> CommandResult<u64> {
    let profile = profiles
        .restored(id, &*state)
        .map_err(|e| CommandError::new("not_found", e))?;
    start(&state, &profile, None, on_event).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::plan::{Repeat, Schedule, Task};

    struct NoKeys;

    impl KeyStore for NoKeys {
        fn get(&self, _: Uuid) -> Result<Option<Secret>, String> {
            Ok(None)
        }

        fn set(&self, _: Uuid, _: Option<&Secret>) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn a_profile_a_task_runs_is_not_deleted() {
        let profiles = Profiles::open(None);
        let scheduler = Scheduler::open(None);
        let profile = profiles
            .put(
                TransferProfile {
                    id: Uuid::new_v4(),
                    name: "Orders".to_owned(),
                    task: ProfileTask::ExportQuery {
                        connection_id: Uuid::new_v4(),
                        database: None,
                        sql: "SELECT 1".to_owned(),
                        path: PathBuf::from("/out/orders.csv"),
                        options: serde_json::Value::Null,
                    },
                },
                |_| true,
                &NoKeys,
            )
            .unwrap();
        let mut schedule = Schedule {
            id: Uuid::new_v4(),
            name: "Nightly orders".to_owned(),
            enabled: true,
            connection_id: Uuid::nil(),
            database: None,
            task: Task::Profile {
                profile_id: profile.id,
            },
            repeat: Repeat::Day,
            time: "07:00".to_owned(),
            timezone: "UTC".to_owned(),
            folder: None,
            keep: 0,
            retries: 0,
            retry_minutes: 5,
        };
        profiles.follow(&mut schedule, profile.id).unwrap();
        scheduler
            .put(schedule.clone(), "2026-11-10T06:00:00Z".parse().unwrap())
            .unwrap();

        let refused = remove(&profiles, &scheduler, &NoKeys, profile.id).unwrap_err();
        assert!(
            refused.message().contains("Nightly orders"),
            "{}",
            refused.message()
        );
        assert!(profiles.get(profile.id).is_some());

        scheduler.delete(schedule.id).unwrap();
        remove(&profiles, &scheduler, &NoKeys, profile.id).unwrap();
        assert!(profiles.get(profile.id).is_none());
    }
}
