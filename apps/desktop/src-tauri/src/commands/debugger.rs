//! The routine debugger (PL/pgSQL): a call debugged step by step on two sessions opened for
//! it from the saved connection, never the user's own ones (the call blocks its session for
//! as long as it is stopped). Each change of state is also sent as a [`STATE`] event.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use platypus_core::{
    Connection, DebugArgument, DebugAvailability, DebugFrame, DebugRoutine, DebugSession,
    DebugState, DebugStep, DebugVariable, Debugger, RoutineRef,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use super::{AppState, connect, lock};
use crate::error::{CommandError, CommandResult};
use crate::querylog::Logged;

/// The event of a debugged call's change of state, with a [`StateEvent`].
pub const STATE: &str = "debug-state";

/// The calls being debugged, by id.
#[derive(Debug, Default)]
pub struct Debugs {
    sessions: Mutex<HashMap<u64, Arc<dyn DebugSession>>>,
    next_id: AtomicU64,
}

impl Debugs {
    fn get(&self, id: u64) -> CommandResult<Arc<dyn DebugSession>> {
        lock(&self.sessions)
            .get(&id)
            .cloned()
            .ok_or_else(|| CommandError::not_found("debugged call", id))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StateEvent {
    pub debug_id: u64,
    pub state: DebugState,
}

/// Sends the new state of call `debug_id`; a finished call is let go, its sessions closed.
fn changed(app: &AppHandle, debugs: &Debugs, debug_id: u64, state: &DebugState) {
    if matches!(state, DebugState::Finished { .. }) {
        lock(&debugs.sessions).remove(&debug_id);
    }
    let _ = app.emit(
        STATE,
        StateEvent {
            debug_id,
            state: state.clone(),
        },
    );
}

/// The debugger of the session's database.
fn debugger_of(conn: &Arc<dyn Connection>) -> CommandResult<Arc<dyn Debugger>> {
    Ok(conn.debugger()?)
}

/// Whether the session's database can debug routines, and routine `schema.name` (its
/// argument types after its name, as the navigator lists it).
#[derive(Debug, Serialize)]
pub struct DebugInfo {
    pub availability: DebugAvailability,
    pub routine: DebugRoutine,
}

#[tauri::command]
pub async fn debug_routine(
    state: State<'_, AppState>,
    session_id: u64,
    schema: String,
    name: String,
) -> CommandResult<DebugInfo> {
    let conn = state.live(session_id).await?;
    let debugger = debugger_of(&conn)?;
    let availability = debugger.availability(&*conn).await?;
    let routine = debugger
        .routine(
            &*conn,
            RoutineRef::Name {
                schema: &schema,
                name: &name,
            },
        )
        .await?;
    Ok(DebugInfo {
        availability,
        routine,
    })
}

/// Creates the debugger's extension in the session's database; not on a read-only
/// connection. Returns what the database can do then.
#[tauri::command]
pub async fn debug_install(
    state: State<'_, AppState>,
    session_id: u64,
) -> CommandResult<DebugAvailability> {
    state.writable(session_id)?;
    let conn = state.live(session_id).await?;
    let debugger = debugger_of(&conn)?;
    super::one_off(&conn, async { Ok(debugger.install(&*conn).await?) }).await?;
    Ok(debugger.availability(&*conn).await?)
}

/// A call stopped at its first statement.
#[derive(Debug, Serialize)]
pub struct DebugStarted {
    pub debug_id: u64,
    pub routine: DebugRoutine,
    /// The routine's source, whose lines the positions count.
    pub source: String,
    pub state: DebugState,
}

/// Calls routine `routine` (its id) with `arguments` on a session opened for it, stopped at
/// its first statement; not on a read-only connection, since the routine may write.
#[tauri::command]
pub async fn debug_start(
    app: AppHandle,
    state: State<'_, AppState>,
    debugs: State<'_, Debugs>,
    session_id: u64,
    routine: String,
    arguments: Vec<DebugArgument>,
) -> CommandResult<DebugStarted> {
    state.writable(session_id)?;
    let session = state.session(session_id)?;
    let open = |role: &'static str| {
        let config = session.config.clone();
        let label = format!("{} · {role}", session.label);
        let log = Arc::clone(&state.query_log);
        async move {
            let raw: Arc<dyn Connection> = Arc::from(connect(&config).await?);
            Ok::<_, CommandError>(Logged::wrap(raw, &log, &label))
        }
    };
    let target = open("debug target").await?;
    let driver = open("debugger").await?;
    let debugger = debugger_of(&target)?;
    let routine = debugger.routine(&*target, RoutineRef::Id(&routine)).await?;
    let call: Arc<dyn DebugSession> =
        Arc::from(debugger.start(target, driver, &routine, &arguments).await?);
    let source = match call.source(&routine.id).await {
        Ok(source) => source,
        Err(e) => {
            let _ = call.stop().await;
            return Err(e.into());
        }
    };
    let debug_id = debugs.next_id.fetch_add(1, Ordering::Relaxed);
    let started = call.state();
    lock(&debugs.sessions).insert(debug_id, call);
    changed(&app, &debugs, debug_id, &started);
    Ok(DebugStarted {
        debug_id,
        routine,
        source,
        state: started,
    })
}

/// Lets the call go on: into or over the next statement, or on to a breakpoint.
#[tauri::command]
pub async fn debug_step(
    app: AppHandle,
    debugs: State<'_, Debugs>,
    debug_id: u64,
    step: DebugStep,
) -> CommandResult<DebugState> {
    let call = debugs.get(debug_id)?;
    changed(&app, &debugs, debug_id, &DebugState::Running);
    let result = call.step(step).await;
    // On an error too: the page learns where the call is now.
    let now = call.state();
    changed(&app, &debugs, debug_id, &now);
    Ok(result?)
}

#[tauri::command]
pub async fn debug_continue(
    app: AppHandle,
    debugs: State<'_, Debugs>,
    debug_id: u64,
) -> CommandResult<DebugState> {
    debug_step(app, debugs, debug_id, DebugStep::Continue).await
}

/// Sets (`on`) or clears the breakpoint at line `line` of routine `routine`.
#[tauri::command]
pub async fn debug_set_breakpoint(
    debugs: State<'_, Debugs>,
    debug_id: u64,
    routine: String,
    line: i32,
    on: bool,
) -> CommandResult<()> {
    Ok(debugs
        .get(debug_id)?
        .set_breakpoint(&routine, line, on)
        .await?)
}

#[tauri::command]
pub async fn debug_stack(
    debugs: State<'_, Debugs>,
    debug_id: u64,
) -> CommandResult<Vec<DebugFrame>> {
    Ok(debugs.get(debug_id)?.stack().await?)
}

#[tauri::command]
pub async fn debug_variables(
    debugs: State<'_, Debugs>,
    debug_id: u64,
    frame: u32,
) -> CommandResult<Vec<DebugVariable>> {
    Ok(debugs.get(debug_id)?.variables(frame).await?)
}

/// Gives variable `name` (declared on `line`) of frame `frame` a new value, `None` for NULL.
#[tauri::command]
pub async fn debug_set_variable(
    debugs: State<'_, Debugs>,
    debug_id: u64,
    frame: u32,
    name: String,
    line: i32,
    value: Option<String>,
) -> CommandResult<()> {
    Ok(debugs
        .get(debug_id)?
        .set_variable(frame, &name, line, value.as_deref())
        .await?)
}

/// The source of routine `routine` as the call runs it, for a frame of another routine.
#[tauri::command]
pub async fn debug_source(
    debugs: State<'_, Debugs>,
    debug_id: u64,
    routine: String,
) -> CommandResult<String> {
    Ok(debugs.get(debug_id)?.source(&routine).await?)
}

/// Ends the call, aborting it where it is; `None` when it had ended already.
#[tauri::command]
pub async fn debug_stop(
    app: AppHandle,
    debugs: State<'_, Debugs>,
    debug_id: u64,
) -> CommandResult<Option<DebugState>> {
    let Some(call) = lock(&debugs.sessions).get(&debug_id).cloned() else {
        return Ok(None);
    };
    let state = call.stop().await?;
    changed(&app, &debugs, debug_id, &state);
    Ok(Some(state))
}
