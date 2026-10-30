//! Debugging a routine of the database (a PL/pgSQL function) step by step: the
//! [`Debugger`] of a driver, from [`Connection::debugger`](crate::Connection::debugger),
//! starts a [`DebugSession`] on two sessions of their own: the *target*, which runs the call
//! and stops in it, and the *debugger*, which drives it. The user's sessions are never
//! used, since the target is blocked for as long as the call is stopped.

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{Connection, Result};

/// Whether the database of a session can debug routines now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DebugAvailability {
    /// Everything is in place.
    Ready,
    /// The server has the debugger, but the database lacks its extension, which
    /// [`Debugger::install`] creates (PostgreSQL's `pldbgapi`).
    MissingExtension,
    /// The server has the debugger's extension but does not load its library at start
    /// (PostgreSQL's `shared_preload_libraries`): only its administrator can change that.
    NotLoaded,
    /// The server has no debugger installed.
    NotInstalled,
}

/// How a routine takes a parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterMode {
    In,
    Out,
    InOut,
    Variadic,
    /// A column of a function's `RETURNS TABLE`: a result, never given.
    Table,
}

impl ParameterMode {
    /// Whether a call gives it a value.
    #[must_use]
    pub fn is_input(self) -> bool {
        matches!(self, Self::In | Self::InOut | Self::Variadic)
    }
}

/// A parameter of a [`DebugRoutine`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DebugParameter {
    /// `None` for a parameter without a name.
    pub name: Option<String>,
    /// As the database writes it, e.g. `integer` or `character varying`.
    pub type_name: String,
    pub mode: ParameterMode,
    /// It has a default value, so a call may leave it out when no later one is given.
    pub has_default: bool,
}

/// Which routine [`Debugger::routine`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutineRef<'a> {
    /// By the database's id for it, [`DebugRoutine::id`].
    Id(&'a str),
    /// By its schema and its name followed by its arguments, as the navigator lists
    /// routines: `add(a integer, b integer)`.
    Name { schema: &'a str, name: &'a str },
}

/// A routine to debug, as [`Debugger::routine`] reads it from the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DebugRoutine {
    /// The database's id for it (PostgreSQL's oid), which positions and frames name.
    pub id: String,
    pub schema: String,
    pub name: String,
    /// Its argument types, e.g. `integer, text`, telling it from its overloads.
    pub signature: String,
    /// The language it is written in, e.g. `plpgsql`.
    pub language: String,
    /// A procedure, run with `CALL`, rather than a function.
    pub procedure: bool,
    /// The type it returns, `None` for a procedure.
    pub returns: Option<String>,
    pub parameters: Vec<DebugParameter>,
    /// Why it cannot be debugged (another language, a trigger function); `None` when it can.
    pub refusal: Option<String>,
}

/// The value given to an input parameter of the call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum DebugArgument {
    /// Text the database reads as a value of the parameter's type, e.g. `42` or `2024-01-31`.
    Value(String),
    Null,
    /// Its default value: only for parameters that have one, all later ones left out too.
    Default,
}

/// How the debugged call goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DebugStep {
    /// To the next statement, into a routine it calls.
    Into,
    /// To the next statement of the same routine.
    Over,
    /// To the next breakpoint, or to the end.
    Continue,
}

/// Where the call stopped: a line of a routine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DebugPosition {
    /// [`DebugRoutine::id`] of the routine, which may be another than the one debugged.
    pub routine: String,
    /// The routine with its argument types, e.g. `add(integer, integer)`.
    pub name: String,
    /// The line of its source (the body only, as the debugger gets it), from 1.
    pub line: i32,
}

/// A frame of the call stack, the innermost first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DebugFrame {
    pub level: u32,
    pub position: DebugPosition,
    /// Its arguments and their values, as the database writes them: `a=1, b=x`.
    pub arguments: String,
}

/// What a variable of a frame is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VariableKind {
    Argument,
    Local,
}

/// A variable of a frame, with its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DebugVariable {
    pub name: String,
    pub kind: VariableKind,
    /// The line it is declared on, which tells apart two variables of the same name.
    pub line: i32,
    pub type_name: String,
    /// As the database writes it; PL/pgSQL writes `NULL` for a null value.
    pub value: String,
    /// Declared `CONSTANT`: it cannot be changed.
    pub constant: bool,
    pub not_null: bool,
}

/// How the debugged call ended, with what it returned.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DebugOutcome {
    pub columns: Vec<String>,
    /// At most [`DEBUG_RESULT_ROWS`] rows, each value as text.
    pub rows: Vec<Vec<Option<String>>>,
    /// There were more rows than those.
    pub truncated: bool,
    /// The error the call ended with; `None` when it returned.
    pub error: Option<String>,
    /// The messages it sent (`RAISE NOTICE`), the debugger's own left out.
    pub messages: Vec<String>,
    /// It was stopped by the user.
    pub aborted: bool,
}

/// How many rows of a debugged call's result [`DebugOutcome`] keeps.
pub const DEBUG_RESULT_ROWS: usize = 1000;

/// Where a [`DebugSession`] is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum DebugState {
    /// The call runs: a step or a continue is under way.
    Running,
    /// The call is stopped at a line, waiting for the next step.
    Stopped { position: DebugPosition },
    /// The call ended; the session is over.
    Finished { outcome: DebugOutcome },
}

/// The debugger of a database's routines.
#[async_trait]
pub trait Debugger: Send + Sync + fmt::Debug {
    /// Whether the database `conn` is on can debug routines now.
    async fn availability(&self, conn: &dyn Connection) -> Result<DebugAvailability>;

    /// Creates the debugger's extension in the database `conn` is on, when
    /// [`availability`](Self::availability) says it is missing.
    async fn install(&self, conn: &dyn Connection) -> Result<()>;

    /// The routine `routine`, with its parameters and whether it can be debugged.
    async fn routine(&self, conn: &dyn Connection, routine: RoutineRef<'_>)
    -> Result<DebugRoutine>;

    /// Calls `routine` with `arguments` (one for each input parameter, in order) on `target`
    /// and stops it at its first statement, driven from `debugger`. Both are sessions of the
    /// same database opened for this alone, closed once the session is over; `target` stays
    /// busy until then.
    async fn start(
        &self,
        target: Arc<dyn Connection>,
        debugger: Arc<dyn Connection>,
        routine: &DebugRoutine,
        arguments: &[DebugArgument],
    ) -> Result<Box<dyn DebugSession>>;
}

/// A call being debugged. Commands run one at a time, except [`stop`](Self::stop), which
/// also ends one under way. Once the call has ended, the session is
/// [`Finished`](DebugState::Finished) and every command but `stop` fails.
#[async_trait]
pub trait DebugSession: Send + Sync + fmt::Debug {
    /// Where the session is now.
    fn state(&self) -> DebugState;

    /// The source of routine `routine` ([`DebugRoutine::id`]) as the call runs it: the lines
    /// that positions count.
    async fn source(&self, routine: &str) -> Result<String>;

    /// Lets the call go on, and returns where it stopped next or how it ended.
    async fn step(&self, step: DebugStep) -> Result<DebugState>;

    /// The call stack, the innermost frame first.
    async fn stack(&self) -> Result<Vec<DebugFrame>>;

    /// The variables of frame `frame` ([`DebugFrame::level`]).
    async fn variables(&self, frame: u32) -> Result<Vec<DebugVariable>>;

    /// Gives variable `name`, declared on `line`, of frame `frame` the value `value` (`None`
    /// for NULL), which the database reads as a value of its type.
    async fn set_variable(
        &self,
        frame: u32,
        name: &str,
        line: i32,
        value: Option<&str>,
    ) -> Result<()>;

    /// Sets (`on`) or clears a breakpoint at line `line` of routine `routine`.
    async fn set_breakpoint(&self, routine: &str, line: i32, on: bool) -> Result<()>;

    /// Ends the session: the call is aborted where it is, its changes rolled back, and both
    /// sessions are let go. Never leaves the target session blocked: when it does not answer,
    /// its server process is ended. Returns the final state; once finished, it only returns
    /// it again.
    async fn stop(&self) -> Result<DebugState>;
}

/// Finds `PLDBGBREAK:<id>`, the notice of a PostgreSQL target waiting for its debugger, in a
/// message as drivers keep them (`NOTICE: PLDBGBREAK:3`); the id to attach to.
#[must_use]
pub fn debug_break_id(message: &str) -> Option<i32> {
    let rest = &message[message.find("PLDBGBREAK:")? + "PLDBGBREAK:".len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_break_notice() {
        assert_eq!(debug_break_id("NOTICE: PLDBGBREAK:3"), Some(3));
        assert_eq!(debug_break_id("PLDBGBREAK:12345"), Some(12345));
        assert_eq!(debug_break_id("NOTICE: PLDBGBREAK:"), None);
        assert_eq!(debug_break_id("NOTICE: hello"), None);
    }

    #[test]
    fn states_serialize_for_the_page() {
        let state = DebugState::Stopped {
            position: DebugPosition {
                routine: "16384".to_owned(),
                name: "f(integer)".to_owned(),
                line: 3,
            },
        };
        assert_eq!(
            serde_json::to_value(&state).unwrap(),
            serde_json::json!({
                "state": "stopped",
                "position": { "routine": "16384", "name": "f(integer)", "line": 3 }
            })
        );
        let argument: DebugArgument =
            serde_json::from_value(serde_json::json!({ "kind": "value", "value": "4" })).unwrap();
        assert_eq!(argument, DebugArgument::Value("4".to_owned()));
        let null: DebugArgument =
            serde_json::from_value(serde_json::json!({ "kind": "null" })).unwrap();
        assert_eq!(null, DebugArgument::Null);
    }
}
