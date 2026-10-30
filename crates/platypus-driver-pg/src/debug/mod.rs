//! The PL/pgSQL debugger, on the `pldbgapi` extension of pldebugger (its library preloaded
//! by the server), as pgAdmin uses it for "direct" debugging: the target session marks the
//! routine with `pldbg_oid_debug` and calls it; at its first statement it sends the notice
//! `PLDBGBREAK:<id>` and waits; the debugger session attaches with `pldbg_attach_to_port`
//! and drives it with `pldbg_step_into`, `pldbg_continue`, … Breakpoints set elsewhere for
//! every session ("global" ones) are not used.

mod session;
mod sql;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use platypus_core::{
    Connection, DebugArgument, DebugAvailability, DebugOutcome, DebugRoutine, DebugSession,
    Debugger, EndSession, Error, Result, RoutineRef, TextRowExt, debug_break_id, query_text,
};
use tokio::sync::watch;

use session::{Call, PgDebugSession};

/// How long a call may take to reach its first statement.
const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a command that only reads (the stack, the variables, a source) may take.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the target may take to end its call once told to.
const SETTLE_TIMEOUT: Duration = Duration::from_secs(5);
/// How often the target's notices are read while waiting for it to stop.
const POLL: Duration = Duration::from_millis(50);

/// The debugger of PL/pgSQL routines.
#[derive(Debug, Default, Clone, Copy)]
pub struct PgDebugger;

#[async_trait]
impl Debugger for PgDebugger {
    async fn availability(&self, conn: &dyn Connection) -> Result<DebugAvailability> {
        let rows = query_text(conn, sql::AVAILABILITY).await?;
        Ok(rows
            .first()
            .map_or(DebugAvailability::NotInstalled, sql::availability))
    }

    async fn install(&self, conn: &dyn Connection) -> Result<()> {
        execute(conn, sql::INSTALL).await
    }

    async fn routine(
        &self,
        conn: &dyn Connection,
        routine: RoutineRef<'_>,
    ) -> Result<DebugRoutine> {
        let rows = query_text(conn, &sql::routine_sql(routine)?).await?;
        let row = rows
            .first()
            .ok_or_else(|| Error::Other("there is no such routine".to_owned()))?;
        let oid = sql::oid(&row.text(0))?;
        let parameters = query_text(conn, &sql::parameters_sql(oid)).await?;
        Ok(sql::routine(row, &parameters))
    }

    async fn start(
        &self,
        target: Arc<dyn Connection>,
        debugger: Arc<dyn Connection>,
        routine: &DebugRoutine,
        arguments: &[DebugArgument],
    ) -> Result<Box<dyn DebugSession>> {
        if let Some(refusal) = &routine.refusal {
            return Err(Error::Other(format!(
                "{} cannot be debugged: {refusal}",
                routine.name
            )));
        }
        let call_sql = sql::call_sql(routine, arguments)?;
        let oid = sql::oid(&routine.id)?;
        let pid = query_text(&*target, "SELECT pg_backend_pid()")
            .await?
            .first()
            .and_then(|r| r.number(0))
            .ok_or_else(|| Error::Other("the target session has no process id".to_owned()))?;
        // Whatever the connection's settings: the call waits as long as it is stopped, and
        // the target's notice must reach the app.
        for sql in sql::TARGET_SETTINGS {
            execute(&*target, sql).await?;
        }
        execute(&*debugger, sql::DEBUGGER_SETTINGS).await?;
        query_text(&*target, &sql::break_on(oid)).await?;
        let _ = target.take_messages();

        let call = Call::spawn(Arc::clone(&target), call_sql);
        let id = match wait_for_break(&*target, &call).await {
            Break::At(id) => id,
            Break::Ended(outcome) => {
                return Err(Error::Other(outcome.error.unwrap_or_else(|| {
                    format!("{} returned without stopping", routine.name)
                })));
            }
            Break::TimedOut => {
                end_target(&*target, &*debugger, pid, &call).await;
                return Err(Error::Other(format!(
                    "{} did not reach its first statement within {} s",
                    routine.name,
                    START_TIMEOUT.as_secs()
                )));
            }
        };
        let attached = tokio::time::timeout(COMMAND_TIMEOUT, async {
            let handle = query_text(&*debugger, &sql::attach(id))
                .await?
                .first()
                .and_then(|r| r.opt(0))
                .and_then(|h| h.parse::<i32>().ok())
                .ok_or_else(|| Error::Other("the debugger did not attach".to_owned()))?;
            let position = sql::position(&query_text(&*debugger, &sql::first_stop(handle)).await?)?;
            Ok::<_, Error>((handle, position))
        })
        .await
        .unwrap_or_else(|_| {
            Err(Error::Other(
                "the debugger did not attach in time".to_owned(),
            ))
        });
        match attached {
            Ok((handle, position)) => Ok(Box::new(PgDebugSession::new(
                target, debugger, handle, pid, call, position,
            ))),
            Err(e) => {
                end_target(&*target, &*debugger, pid, &call).await;
                Err(e)
            }
        }
    }
}

/// What the target did while the debugger waited for it to stop.
enum Break {
    /// It stopped and waits for a debugger to attach to `id`.
    At(i32),
    /// Its call ended without stopping: a value it could not read, say.
    Ended(DebugOutcome),
    TimedOut,
}

/// Waits for the notice the target sends when it stops at the routine's first statement.
/// Its other messages are kept for the outcome.
async fn wait_for_break(target: &dyn Connection, call: &Call) -> Break {
    let deadline = tokio::time::Instant::now() + START_TIMEOUT;
    loop {
        let messages = target.take_messages();
        let id = messages.iter().find_map(|m| debug_break_id(m));
        call.keep_messages(sql::own_messages(messages));
        if let Some(id) = id {
            return Break::At(id);
        }
        if let Some(outcome) = call.outcome() {
            return Break::Ended(outcome);
        }
        if tokio::time::Instant::now() >= deadline {
            return Break::TimedOut;
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Ends the call of the target when the debugger cannot: cancels its statement, then ends
/// its server process if it does not stop.
async fn end_target(target: &dyn Connection, debugger: &dyn Connection, pid: u64, call: &Call) {
    let _ = target.cancel_handle().cancel().await;
    if call.wait(SETTLE_TIMEOUT).await.is_none() {
        let _ = tokio::time::timeout(
            SETTLE_TIMEOUT,
            debugger.end_server_session(&pid.to_string(), EndSession::Terminate),
        )
        .await;
    }
}

/// The outcome of a call, once it ends; shared by the task that runs it and the session.
type Outcome = watch::Receiver<Option<DebugOutcome>>;

/// Runs `sql`, which returns nothing to read.
async fn execute(conn: &dyn Connection, sql: &str) -> Result<()> {
    conn.execute(sql, &platypus_core::ExecOptions::default())
        .await
        .map(drop)
}
