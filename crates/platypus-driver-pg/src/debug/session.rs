//! A call being debugged: the task that runs it on the target session, and the
//! [`DebugSession`] that drives it from the debugger session.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use platypus_core::{
    Connection, DEBUG_RESULT_ROWS, DebugFrame, DebugOutcome, DebugPosition, DebugSession,
    DebugState, DebugStep, DebugVariable, Error, ExecOptions, ExecResult, Result, TextRow,
    query_text, text_rows,
};
use tokio::sync::{OnceCell, watch};

use super::{COMMAND_TIMEOUT, Outcome, SETTLE_TIMEOUT, end_target, sql};

/// The call of the debugged routine, run by a task of its own on the target session, which
/// it blocks until it ends.
#[derive(Debug, Clone)]
pub(super) struct Call {
    outcome: Outcome,
    /// The messages the call sent before it stopped the first time, read while waiting
    /// for the debugger's notice.
    early: Arc<Mutex<Vec<String>>>,
}

impl Call {
    pub(super) fn spawn(target: Arc<dyn Connection>, sql: String) -> Self {
        let (tx, outcome) = watch::channel(None);
        tokio::spawn(async move {
            let outcome = run(&*target, &sql).await;
            let _ = tx.send(Some(outcome));
        });
        Self {
            outcome,
            early: Arc::default(),
        }
    }

    pub(super) fn keep_messages(&self, messages: Vec<String>) {
        self.early
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(messages);
    }

    /// How the call ended, `None` while it runs.
    pub(super) fn outcome(&self) -> Option<DebugOutcome> {
        let outcome = self.outcome.borrow().clone()?;
        Some(self.with_early(outcome))
    }

    /// Waits until the call ends.
    pub(super) async fn ended(&self) -> DebugOutcome {
        let mut rx = self.outcome.clone();
        let outcome = match rx.wait_for(Option::is_some).await {
            Ok(outcome) => outcome.clone().unwrap_or_default(),
            // The task is gone without an outcome: the runtime is shutting down.
            Err(_) => DebugOutcome {
                error: Some("the call was interrupted".to_owned()),
                ..DebugOutcome::default()
            },
        };
        self.with_early(outcome)
    }

    /// Waits up to `timeout` for the call to end.
    pub(super) async fn wait(&self, timeout: Duration) -> Option<DebugOutcome> {
        tokio::time::timeout(timeout, self.ended()).await.ok()
    }

    fn with_early(&self, mut outcome: DebugOutcome) -> DebugOutcome {
        let early = self.early.lock().unwrap_or_else(PoisonError::into_inner);
        outcome.messages.splice(0..0, early.iter().cloned());
        outcome
    }
}

/// Runs the call and keeps what it returned, or the error it ended with.
async fn run(target: &dyn Connection, sql: &str) -> DebugOutcome {
    let mut outcome = DebugOutcome::default();
    match target.execute(sql, &ExecOptions::default()).await {
        Ok(ExecResult::Rows(stream)) => {
            outcome.columns = stream
                .schema()
                .fields()
                .iter()
                .map(|f| f.name().clone())
                .collect();
            match text_rows(stream).await {
                Ok(mut rows) => {
                    outcome.truncated = rows.len() > DEBUG_RESULT_ROWS;
                    rows.truncate(DEBUG_RESULT_ROWS);
                    outcome.rows = rows;
                }
                Err(e) => outcome.error = Some(e.to_string()),
            }
        }
        Ok(ExecResult::Affected { .. }) => {}
        Err(e) => outcome.error = Some(e.to_string()),
    }
    outcome.messages = sql::own_messages(target.take_messages());
    outcome
}

/// What a statement of the debugger session came to.
enum Driven {
    Rows(Vec<TextRow>),
    /// The call ended first: the statement waited for a stop that will not come.
    Ended(DebugOutcome),
}

/// A call stopped in a PL/pgSQL routine, driven from the debugger session. Dropped before
/// [`stop`](DebugSession::stop), it stops in the background.
#[derive(Debug)]
pub(super) struct PgDebugSession(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    target: Arc<dyn Connection>,
    debugger: Arc<dyn Connection>,
    /// The debugger's handle of the target, which its statements take.
    handle: i32,
    /// The target's server process, ended when nothing else stops the call.
    pid: u64,
    call: Call,
    state: Mutex<DebugState>,
    /// Held by the command under way: they run one at a time.
    busy: tokio::sync::Mutex<()>,
    /// The final state, once the session is over.
    ended: OnceCell<DebugState>,
}

impl PgDebugSession {
    pub(super) fn new(
        target: Arc<dyn Connection>,
        debugger: Arc<dyn Connection>,
        handle: i32,
        pid: u64,
        call: Call,
        position: DebugPosition,
    ) -> Self {
        Self(Arc::new(Inner {
            target,
            debugger,
            handle,
            pid,
            call,
            state: Mutex::new(DebugState::Stopped { position }),
            busy: tokio::sync::Mutex::new(()),
            ended: OnceCell::new(),
        }))
    }
}

impl Drop for PgDebugSession {
    fn drop(&mut self) {
        if self.0.ended.initialized() {
            return;
        }
        // Never leave the target blocked in its call.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let inner = Arc::clone(&self.0);
            runtime.spawn(async move {
                inner.stop().await;
            });
        }
    }
}

/// Fails unless a command can run in `state`: only while the call is stopped at a line.
fn ensure_stopped(state: &DebugState) -> Result<()> {
    match state {
        DebugState::Stopped { .. } => Ok(()),
        DebugState::Running => Err(Error::Other(
            "the call is running: wait for it to stop".to_owned(),
        )),
        DebugState::Finished { .. } => Err(Error::Other("the debugged call has ended".to_owned())),
    }
}

/// The state after a step, from what the debugger said.
fn after_step(driven: Driven) -> Result<DebugState> {
    Ok(match driven {
        Driven::Rows(rows) => DebugState::Stopped {
            position: sql::position(&rows)?,
        },
        Driven::Ended(outcome) => DebugState::Finished { outcome },
    })
}

impl Inner {
    fn state(&self) -> DebugState {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Records `state`, unless the session is over: a step that saw the call end as a stop
    /// aborted it says less than the stop.
    fn set_state(&self, state: DebugState) {
        let state = self.ended.get().cloned().unwrap_or(state);
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = state;
    }

    /// Runs `sql` on the debugger session, unless the call ends first, which would leave the
    /// statement waiting for a stop forever: then it is cancelled.
    async fn drive(&self, sql: &str) -> Result<Driven> {
        let query = query_text(&*self.debugger, sql);
        tokio::pin!(query);
        tokio::select! {
            rows = &mut query => match rows {
                Ok(rows) => Ok(Driven::Rows(rows)),
                // A debugger that fails as the call ends (the target's server process
                // ended, say) says less than how the call ended.
                Err(e) => match self.call.wait(Duration::from_millis(200)).await {
                    Some(outcome) => Ok(Driven::Ended(outcome)),
                    None => Err(e),
                },
            },
            outcome = self.call.ended() => {
                let _ = self.debugger.cancel_handle().cancel().await;
                let _ = tokio::time::timeout(SETTLE_TIMEOUT, &mut query).await;
                Ok(Driven::Ended(outcome))
            }
        }
    }

    /// Runs a command while the call is stopped: `sql` and what follows, within
    /// [`COMMAND_TIMEOUT`].
    async fn command(&self, statements: &[String]) -> Result<Vec<TextRow>> {
        let _busy = self.busy.lock().await;
        ensure_stopped(&self.state())?;
        let run = async {
            let mut last = Vec::new();
            for sql in statements {
                match self.drive(sql).await? {
                    Driven::Rows(rows) => last = rows,
                    Driven::Ended(outcome) => {
                        let state = self.finish(outcome);
                        self.set_state(state);
                        return Err(Error::Other("the debugged call has ended".to_owned()));
                    }
                }
            }
            Ok(last)
        };
        tokio::time::timeout(COMMAND_TIMEOUT, run)
            .await
            .unwrap_or_else(|_| {
                Err(Error::Other(format!(
                    "the debugger did not answer within {} s",
                    COMMAND_TIMEOUT.as_secs()
                )))
            })
    }

    /// The final state of a call that ended with `outcome`, recorded once.
    fn finish(&self, outcome: DebugOutcome) -> DebugState {
        let state = DebugState::Finished { outcome };
        let _ = self.ended.set(state.clone());
        self.ended.get().cloned().unwrap_or(state)
    }

    async fn step(&self, step: DebugStep) -> Result<DebugState> {
        let _busy = self.busy.lock().await;
        let before = self.state();
        ensure_stopped(&before)?;
        self.set_state(DebugState::Running);
        let state = match self.drive(&sql::step(self.handle, step)).await {
            Ok(driven) => after_step(driven),
            Err(e) => Err(e),
        };
        match state {
            Ok(DebugState::Finished { outcome }) => {
                let state = self.finish(outcome);
                self.set_state(state.clone());
                Ok(state)
            }
            Ok(state) => {
                self.set_state(state.clone());
                Ok(state)
            }
            Err(e) => {
                // The debugger session failed: the call cannot be driven any more.
                let state = self.end(false).await;
                self.set_state(state);
                Err(e)
            }
        }
    }

    /// Ends the call, aborting it through the debugger when it is stopped at a line
    /// (`stopped`), cancelling its statement otherwise, and ending the target's server
    /// process when neither works.
    async fn end(&self, stopped: bool) -> DebugState {
        self.ended
            .get_or_init(|| async {
                if stopped {
                    let _ =
                        tokio::time::timeout(SETTLE_TIMEOUT, self.drive(&sql::abort(self.handle)))
                            .await;
                }
                let outcome = if let Some(outcome) = self.call.wait(SETTLE_TIMEOUT).await {
                    outcome
                } else {
                    // The debugger may be waiting for a stop: free it for the request.
                    let _ = self.debugger.cancel_handle().cancel().await;
                    end_target(&*self.target, &*self.debugger, self.pid, &self.call).await;
                    self.call
                        .wait(SETTLE_TIMEOUT)
                        .await
                        .unwrap_or_else(|| DebugOutcome {
                            error: Some("the target session did not answer".to_owned()),
                            ..DebugOutcome::default()
                        })
                };
                DebugState::Finished {
                    outcome: DebugOutcome {
                        aborted: true,
                        ..outcome
                    },
                }
            })
            .await
            .clone()
    }

    async fn stop(&self) -> DebugState {
        if let Some(state) = self.ended.get() {
            return state.clone();
        }
        let state = if let Ok(_busy) = self.busy.try_lock() {
            // Stopped at a line: the debugger tells the target to abort.
            self.end(true).await
        } else {
            // A step is under way: the call runs, and a cancel stops it.
            let _ = self.target.cancel_handle().cancel().await;
            self.end(false).await
        };
        self.set_state(state.clone());
        state
    }
}

#[async_trait]
impl DebugSession for PgDebugSession {
    fn state(&self) -> DebugState {
        self.0.state()
    }

    async fn source(&self, routine: &str) -> Result<String> {
        let oid = sql::oid(routine)?;
        let rows = self.0.command(&[sql::source(self.0.handle, oid)]).await?;
        Ok(rows
            .first()
            .and_then(|r| r.first().cloned().flatten())
            .unwrap_or_default())
    }

    async fn step(&self, step: DebugStep) -> Result<DebugState> {
        self.0.step(step).await
    }

    async fn stack(&self) -> Result<Vec<DebugFrame>> {
        let rows = self.0.command(&[sql::stack(self.0.handle)]).await?;
        Ok(sql::frames(&rows))
    }

    async fn variables(&self, frame: u32) -> Result<Vec<DebugVariable>> {
        let rows = self
            .0
            .command(&[
                sql::select_frame(self.0.handle, frame),
                sql::variables(self.0.handle),
            ])
            .await?;
        Ok(sql::variable_rows(&rows))
    }

    async fn set_variable(
        &self,
        frame: u32,
        name: &str,
        line: i32,
        value: Option<&str>,
    ) -> Result<()> {
        let rows = self
            .0
            .command(&[
                sql::select_frame(self.0.handle, frame),
                sql::deposit(self.0.handle, name, line, value),
            ])
            .await?;
        if rows
            .first()
            .is_some_and(|r| platypus_core::TextRowExt::flag(r, 0))
        {
            Ok(())
        } else {
            Err(Error::Other(format!(
                "{name} cannot take the value {}",
                value.unwrap_or("NULL")
            )))
        }
    }

    async fn set_breakpoint(&self, routine: &str, line: i32, on: bool) -> Result<()> {
        let oid = sql::oid(routine)?;
        let rows = self
            .0
            .command(&[sql::breakpoint(self.0.handle, oid, line, on)])
            .await?;
        if on
            && !rows
                .first()
                .is_some_and(|r| platypus_core::TextRowExt::flag(r, 0))
        {
            return Err(Error::Other(format!(
                "no breakpoint can be set at line {line}"
            )));
        }
        Ok(())
    }

    async fn stop(&self) -> Result<DebugState> {
        Ok(self.0.stop().await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stopped(line: i32) -> DebugState {
        DebugState::Stopped {
            position: DebugPosition {
                routine: "1".to_owned(),
                name: "f()".to_owned(),
                line,
            },
        }
    }

    #[test]
    fn commands_run_only_while_stopped() {
        assert!(ensure_stopped(&stopped(1)).is_ok());
        assert!(ensure_stopped(&DebugState::Running).is_err());
        let finished = DebugState::Finished {
            outcome: DebugOutcome::default(),
        };
        assert!(ensure_stopped(&finished).is_err());
    }

    #[test]
    fn a_step_stops_at_a_line_or_ends() {
        let row: TextRow = vec![
            Some("1".to_owned()),
            Some("4".to_owned()),
            Some("f()".to_owned()),
        ];
        assert_eq!(after_step(Driven::Rows(vec![row])).unwrap(), stopped(4));
        let outcome = DebugOutcome {
            columns: vec!["f".to_owned()],
            rows: vec![vec![Some("10".to_owned())]],
            ..DebugOutcome::default()
        };
        assert_eq!(
            after_step(Driven::Ended(outcome.clone())).unwrap(),
            DebugState::Finished { outcome }
        );
        assert!(after_step(Driven::Rows(Vec::new())).is_err());
    }

    #[tokio::test]
    async fn a_call_keeps_its_early_messages_first() {
        let (tx, outcome) = watch::channel(None);
        let call = Call {
            outcome,
            early: Arc::default(),
        };
        call.keep_messages(vec!["NOTICE: start".to_owned()]);
        assert!(call.outcome().is_none());
        assert!(call.wait(Duration::from_millis(10)).await.is_none());
        tx.send(Some(DebugOutcome {
            messages: vec!["NOTICE: end".to_owned()],
            ..DebugOutcome::default()
        }))
        .unwrap();
        assert_eq!(
            call.ended().await.messages,
            ["NOTICE: start", "NOTICE: end"]
        );
        assert_eq!(call.outcome().unwrap().messages.len(), 2);
    }
}
