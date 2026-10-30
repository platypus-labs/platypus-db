//! The PL/pgSQL debugger (`Connection::debugger`).
//!
//! The debugging itself needs PostgreSQL with pldebugger: the `postgres-pldebug` service of
//! `docker/compose.yml` (profile `pldebug`, on 55437), or `PLATYPUS_TEST_PLDEBUG_URL`, which
//! makes it required. Without it, or when its server lacks the debugger, those tests are
//! skipped. The dev PostgreSQL (55432, or `PLATYPUS_TEST_PG_URL`), which has no debugger,
//! checks what happens there: the debugger is reported missing, and a start fails without
//! leaving a session blocked.

use std::sync::Arc;
use std::time::Duration;

use platypus_core::{
    Connection, ConnectionConfig, DebugArgument, DebugAvailability, DebugRoutine, DebugSession,
    DebugState, DebugStep, Driver, ExecOptions, ExecResult, ParameterMode, RoutineRef, TextRowExt,
    query_text,
};
use platypus_driver_pg::PgDriver;

const PG_URL: &str = "postgres://platypus:platypus@localhost:55432/platypus?sslmode=disable";
const PLDEBUG_URL: &str = "postgres://platypus:platypus@localhost:55437/platypus?sslmode=disable";

/// The tests on one server share its functions: they run one at a time.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A config for the server of `env`, or of `default`; `None` (the test skipped) when the
/// default one is unreachable.
async fn config(env: &str, default: &str) -> Option<ConnectionConfig> {
    if std::env::var("PLATYPUS_TEST_ONLY").is_ok_and(|only| only != "postgres") {
        return None;
    }
    let (url, required) = match std::env::var(env) {
        Ok(url) => (url, true),
        Err(_) => (default.to_owned(), false),
    };
    let mut config = ConnectionConfig::from_url(&url).unwrap();
    config.connect_timeout = Some(Duration::from_secs(3));
    match PgDriver.connect(&config).await {
        Ok(_) => Some(config),
        Err(e) if !required => {
            eprintln!("skipping the debugger on {url}: unreachable: {e}");
            None
        }
        Err(e) => panic!("cannot connect to {env}: {e}"),
    }
}

async fn connect(config: &ConnectionConfig) -> Arc<dyn Connection> {
    Arc::from(PgDriver.connect(config).await.unwrap())
}

async fn run(conn: &dyn Connection, sql: &str) {
    match conn.execute(sql, &ExecOptions::default()).await {
        Ok(ExecResult::Rows(_) | ExecResult::Affected { .. }) => {}
        Err(e) => panic!("{sql}: {e}"),
    }
}

/// A function with a loop that calls another, and a procedure: their sources start on the
/// line after `$$`, the first line of what the debugger shows.
const FUNCTIONS: &[&str] = &[
    "CREATE OR REPLACE FUNCTION conf_dbg_double(x integer) RETURNS integer
     LANGUAGE plpgsql AS $$
BEGIN
    RETURN x * 2;
END
$$",
    "CREATE OR REPLACE FUNCTION conf_dbg_sum(n integer, start integer DEFAULT 0)
     RETURNS integer LANGUAGE plpgsql AS $$
DECLARE
    total integer := start;
BEGIN
    FOR i IN 1..n LOOP
        total := total + conf_dbg_double(i);
    END LOOP;
    RAISE NOTICE 'total %', total;
    RETURN total;
END
$$",
    "CREATE OR REPLACE FUNCTION conf_dbg_spin() RETURNS void LANGUAGE plpgsql AS $$
BEGIN
    LOOP
        PERFORM pg_sleep(0.05);
    END LOOP;
END
$$",
    "CREATE OR REPLACE FUNCTION conf_dbg_sql(x integer) RETURNS integer
     LANGUAGE sql AS 'SELECT x + 1'",
    "CREATE OR REPLACE PROCEDURE conf_dbg_proc(a integer, INOUT b integer)
     LANGUAGE plpgsql AS $$
BEGIN
    b := a + b;
END
$$",
];

async fn create_functions(conn: &dyn Connection) {
    for sql in FUNCTIONS {
        run(conn, sql).await;
    }
}

async fn routine(conn: &dyn Connection, name: &str) -> DebugRoutine {
    conn.debugger()
        .unwrap()
        .routine(
            conn,
            RoutineRef::Name {
                schema: "public",
                name,
            },
        )
        .await
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The statements still running a debugged call, other than `own`'s.
async fn running_calls(conn: &dyn Connection) -> u64 {
    let rows = query_text(
        conn,
        "SELECT count(*) FROM pg_stat_activity \
         WHERE state = 'active' AND query LIKE '%conf_dbg_%' AND pid <> pg_backend_pid()",
    )
    .await
    .unwrap();
    rows[0].number(0).unwrap()
}

/// Waits up to ten seconds for no debugged call to run any more.
async fn no_call_left(conn: &dyn Connection) {
    for _ in 0..100 {
        if running_calls(conn).await == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("a debugged call is still running");
}

/// The dev server has no debugger: it says so, the routines read all the same, and a start
/// fails without blocking anything.
#[tokio::test]
async fn reports_a_server_without_debugger() {
    let _serial = SERIAL.lock().await;
    let Some(config) = config("PLATYPUS_TEST_PG_URL", PG_URL).await else {
        return;
    };
    let conn = connect(&config).await;
    assert!(conn.capabilities().debugger);
    create_functions(&*conn).await;
    let debugger = conn.debugger().unwrap();
    let availability = debugger.availability(&*conn).await.unwrap();

    let sum = routine(&*conn, "conf_dbg_sum(n integer, start integer)").await;
    assert_eq!(sum.language, "plpgsql");
    assert_eq!(sum.returns.as_deref(), Some("integer"));
    assert!(!sum.procedure && sum.refusal.is_none());
    let names: Vec<_> = sum.parameters.iter().map(|p| p.name.as_deref()).collect();
    assert_eq!(names, [Some("n"), Some("start")]);
    let defaults: Vec<_> = sum.parameters.iter().map(|p| p.has_default).collect();
    assert_eq!(defaults, [false, true]);
    let by_id = debugger
        .routine(&*conn, RoutineRef::Id(&sum.id))
        .await
        .unwrap();
    assert_eq!(by_id, sum);

    let proc = routine(&*conn, "conf_dbg_proc(IN a integer, INOUT b integer)").await;
    assert!(proc.procedure && proc.returns.is_none());
    assert_eq!(proc.parameters[1].mode, ParameterMode::InOut);
    let sql = routine(&*conn, "conf_dbg_sql(x integer)").await;
    assert!(sql.refusal.is_some());
    assert!(
        debugger
            .routine(
                &*conn,
                RoutineRef::Name {
                    schema: "public",
                    name: "conf_dbg_missing(x integer)"
                }
            )
            .await
            .is_err()
    );

    if availability == DebugAvailability::Ready {
        eprintln!("the dev PostgreSQL has the debugger: its tests run in debugs_a_function");
        return;
    }
    let target = connect(&config).await;
    let start = debugger
        .start(
            Arc::clone(&target),
            connect(&config).await,
            &sum,
            &[DebugArgument::Value("3".to_owned()), DebugArgument::Default],
        )
        .await;
    assert!(start.is_err(), "no debugger, no session");
    // The target is free again.
    run(&*target, "SELECT 1").await;
    no_call_left(&*conn).await;
}

/// A session on the server with the debugger, its extension created as the app offers to.
async fn debug_server() -> Option<(ConnectionConfig, Arc<dyn Connection>)> {
    let config = config("PLATYPUS_TEST_PLDEBUG_URL", PLDEBUG_URL).await?;
    let conn = connect(&config).await;
    let debugger = conn.debugger().unwrap();
    match debugger.availability(&*conn).await.unwrap() {
        DebugAvailability::Ready => {}
        DebugAvailability::MissingExtension => {
            debugger.install(&*conn).await.unwrap();
            assert_eq!(
                debugger.availability(&*conn).await.unwrap(),
                DebugAvailability::Ready
            );
        }
        other => {
            assert!(
                std::env::var("PLATYPUS_TEST_PLDEBUG_URL").is_err(),
                "PLATYPUS_TEST_PLDEBUG_URL has no debugger: {other:?}"
            );
            eprintln!("skipping the debugger: the server says {other:?}");
            return None;
        }
    }
    create_functions(&*conn).await;
    Some((config, conn))
}

async fn start(
    config: &ConnectionConfig,
    routine: &DebugRoutine,
    arguments: &[DebugArgument],
) -> Box<dyn DebugSession> {
    let target = connect(config).await;
    target
        .debugger()
        .unwrap()
        .start(
            Arc::clone(&target),
            connect(config).await,
            routine,
            arguments,
        )
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", routine.name))
}

fn stopped(state: &DebugState) -> (&str, i32) {
    match state {
        DebugState::Stopped { position } => (position.routine.as_str(), position.line),
        other => panic!("not stopped: {other:?}"),
    }
}

/// The line of `source` (from 1) that contains `text`.
fn line_of(source: &str, text: &str) -> i32 {
    let index = source
        .lines()
        .position(|l| l.contains(text))
        .unwrap_or_else(|| panic!("{text} not in {source}"));
    i32::try_from(index + 1).unwrap()
}

/// The value of variable `name` of frame `frame`.
async fn value(session: &dyn DebugSession, frame: u32, name: &str) -> String {
    let variables = session.variables(frame).await.unwrap();
    variables
        .iter()
        .find(|v| v.name == name)
        .unwrap_or_else(|| panic!("no {name} in {variables:?}"))
        .value
        .clone()
}

/// Breakpoints, steps into and over a nested call, the stack and its variables, a variable
/// changed, and the call run on to its result.
#[tokio::test]
async fn debugs_a_function() {
    let _serial = SERIAL.lock().await;
    let Some((config, conn)) = debug_server().await else {
        return;
    };
    let sum = routine(&*conn, "conf_dbg_sum(n integer, start integer)").await;
    let double = routine(&*conn, "conf_dbg_double(x integer)").await;
    let session = start(
        &config,
        &sum,
        &[DebugArgument::Value("3".to_owned()), DebugArgument::Default],
    )
    .await;
    let first = session.state();
    assert_eq!(stopped(&first).0, sum.id);

    let source = session.source(&sum.id).await.unwrap();
    let body = line_of(&source, "total := total + conf_dbg_double(i)");
    session.set_breakpoint(&sum.id, body, true).await.unwrap();
    let state = session.step(DebugStep::Continue).await.unwrap();
    assert_eq!(stopped(&state), (sum.id.as_str(), body));
    assert_eq!(value(&*session, 0, "n").await, "3");
    assert_eq!(value(&*session, 0, "i").await, "1");
    assert_eq!(value(&*session, 0, "total").await, "0");
    let n = session
        .variables(0)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.name == "n")
        .unwrap();
    assert_eq!(n.kind, platypus_core::VariableKind::Argument);

    // Into the nested function: two frames, each with its own variables.
    let state = session.step(DebugStep::Into).await.unwrap();
    assert_eq!(stopped(&state).0, double.id);
    assert!(session.source(&double.id).await.unwrap().contains("x * 2"));
    let stack = session.stack().await.unwrap();
    assert_eq!(stack.len(), 2, "{stack:?}");
    let outer = stack
        .iter()
        .find(|f| f.position.routine == sum.id)
        .expect("the caller's frame");
    assert_eq!(outer.position.line, body);
    assert_eq!(value(&*session, outer.level, "total").await, "0");
    let inner = stack
        .iter()
        .find(|f| f.position.routine == double.id)
        .unwrap();
    assert_eq!(value(&*session, inner.level, "x").await, "1");

    // Over the rest of it, back in the loop for the next value.
    let mut state = session.step(DebugStep::Over).await.unwrap();
    for _ in 0..5 {
        if stopped(&state).0 == sum.id {
            break;
        }
        state = session.step(DebugStep::Over).await.unwrap();
    }
    assert_eq!(stopped(&state), (sum.id.as_str(), body));
    assert_eq!(value(&*session, 0, "i").await, "2");
    assert_eq!(value(&*session, 0, "total").await, "2");

    // A changed variable carries on: 100, then + 4 + 6.
    let total = session
        .variables(0)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.name == "total")
        .unwrap();
    session
        .set_variable(0, "total", total.line, Some("100"))
        .await
        .unwrap();
    assert_eq!(value(&*session, 0, "total").await, "100");
    assert!(
        session
            .set_variable(0, "total", total.line, Some("not a number"))
            .await
            .is_err()
    );

    session.set_breakpoint(&sum.id, body, false).await.unwrap();
    let DebugState::Finished { outcome } = session.step(DebugStep::Continue).await.unwrap() else {
        panic!("not finished");
    };
    assert_eq!(outcome.error, None);
    assert!(!outcome.aborted);
    assert_eq!(outcome.rows, [[Some("110".to_owned())]]);
    assert!(
        outcome.messages.iter().any(|m| m.contains("total 110")),
        "{:?}",
        outcome.messages
    );
    // Over: commands fail, stop says how it ended.
    assert!(session.step(DebugStep::Over).await.is_err());
    assert!(matches!(
        session.stop().await.unwrap(),
        DebugState::Finished { outcome } if !outcome.aborted
    ));
    no_call_left(&*conn).await;
}

/// A procedure runs with `CALL` and returns its `INOUT` parameter; stepping past its last
/// line ends the call.
#[tokio::test]
async fn debugs_a_procedure_to_its_end() {
    let _serial = SERIAL.lock().await;
    let Some((config, conn)) = debug_server().await else {
        return;
    };
    let proc = routine(&*conn, "conf_dbg_proc(IN a integer, INOUT b integer)").await;
    let session = start(
        &config,
        &proc,
        &[
            DebugArgument::Value("2".to_owned()),
            DebugArgument::Value("40".to_owned()),
        ],
    )
    .await;
    let mut state = session.state();
    for _ in 0..5 {
        if matches!(state, DebugState::Finished { .. }) {
            break;
        }
        state = session.step(DebugStep::Over).await.unwrap();
    }
    let DebugState::Finished { outcome } = state else {
        panic!("not finished: {state:?}");
    };
    assert_eq!(outcome.columns, ["b"]);
    assert_eq!(outcome.rows, [[Some("42".to_owned())]]);
    no_call_left(&*conn).await;
}

/// Stopping mid-way aborts the call, whether it is stopped at a line, running on to a
/// breakpoint it never reaches, or left behind by a dropped session: no session stays
/// blocked.
#[tokio::test]
async fn stopping_leaves_no_blocked_session() {
    let _serial = SERIAL.lock().await;
    let Some((config, conn)) = debug_server().await else {
        return;
    };
    let sum = routine(&*conn, "conf_dbg_sum(n integer, start integer)").await;
    let arguments = [
        DebugArgument::Value("1000".to_owned()),
        DebugArgument::Value("5".to_owned()),
    ];

    // Stopped at a line.
    let session = start(&config, &sum, &arguments).await;
    session.step(DebugStep::Into).await.unwrap();
    let DebugState::Finished { outcome } = session.stop().await.unwrap() else {
        panic!("not finished");
    };
    assert!(outcome.aborted);
    assert!(outcome.error.is_some(), "{outcome:?}");
    assert_eq!(outcome.rows, Vec::<Vec<Option<String>>>::new());
    assert!(session.stack().await.is_err());
    no_call_left(&*conn).await;

    // Running: an endless loop, stopped while a continue waits for it.
    let spin = routine(&*conn, "conf_dbg_spin()").await;
    let session: Arc<dyn DebugSession> = Arc::from(start(&config, &spin, &[]).await);
    let running = tokio::spawn({
        let session = Arc::clone(&session);
        async move { session.step(DebugStep::Continue).await }
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(session.state(), DebugState::Running);
    let DebugState::Finished { outcome } = session.stop().await.unwrap() else {
        panic!("not finished");
    };
    assert!(outcome.aborted);
    let continued = tokio::time::timeout(Duration::from_secs(15), running)
        .await
        .expect("the continue returns once stopped")
        .unwrap();
    assert!(!matches!(continued, Ok(DebugState::Stopped { .. })));
    no_call_left(&*conn).await;

    // Dropped without a stop.
    let session = start(&config, &sum, &arguments).await;
    drop(session);
    no_call_left(&*conn).await;
}

/// A value the parameter's type cannot read ends the call before it stops: the start fails
/// with the server's error, and nothing is left running.
#[tokio::test]
async fn a_bad_argument_fails_the_start() {
    let _serial = SERIAL.lock().await;
    let Some((config, conn)) = debug_server().await else {
        return;
    };
    let sum = routine(&*conn, "conf_dbg_sum(n integer, start integer)").await;
    let target = connect(&config).await;
    let error = target
        .debugger()
        .unwrap()
        .start(
            Arc::clone(&target),
            connect(&config).await,
            &sum,
            &[
                DebugArgument::Value("three".to_owned()),
                DebugArgument::Default,
            ],
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("integer"), "{error}");
    run(&*target, "SELECT 1").await;
    no_call_left(&*conn).await;
}
