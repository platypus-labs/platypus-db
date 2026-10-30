//! The statements of the PL/pgSQL debugger (the `pldbgapi` extension of pldebugger) and
//! the reading of their rows: plain functions of their input, tested without a server.

use platypus_core::{
    DebugArgument, DebugAvailability, DebugFrame, DebugParameter, DebugPosition, DebugRoutine,
    DebugStep, DebugVariable, Error, ParameterMode, Result, RoutineRef, TextRow, TextRowExt,
    VariableKind,
};
use platypus_sql::Dialect;

/// Whether the extension is created in the database, whether the server has it, and the
/// libraries the server loads at start (no row for a user who may not read the setting).
pub(super) const AVAILABILITY: &str = "SELECT \
     EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pldbgapi'), \
     EXISTS (SELECT 1 FROM pg_available_extensions WHERE name = 'pldbgapi'), \
     (SELECT setting FROM pg_settings WHERE name = 'shared_preload_libraries')";

/// What the database `row` of [`AVAILABILITY`] describes can do.
pub(super) fn availability(row: &TextRow) -> DebugAvailability {
    let (created, available) = (row.flag(0), row.flag(1));
    // A setting the session may not read says nothing: the attempt will tell.
    let loaded = row.opt(2).is_none_or(|libraries| {
        libraries.split(',').any(|library| {
            let library = library.trim().trim_matches('"');
            library == "plugin_debugger" || library.ends_with("/plugin_debugger")
        })
    });
    match (created || available, loaded, created) {
        (false, _, _) => DebugAvailability::NotInstalled,
        (true, false, _) => DebugAvailability::NotLoaded,
        (true, true, false) => DebugAvailability::MissingExtension,
        (true, true, true) => DebugAvailability::Ready,
    }
}

pub(super) const INSTALL: &str = "CREATE EXTENSION IF NOT EXISTS pldbgapi";

/// What the target session sets before the call: no time limit, and its notices sent.
pub(super) const TARGET_SETTINGS: &[&str] = &[
    "SET statement_timeout = 0",
    "SET client_min_messages = notice",
];

/// What the debugger session sets: a continue waits as long as the call runs.
pub(super) const DEBUGGER_SETTINGS: &str = "SET statement_timeout = 0";

/// The oid `id`, refused when it is not one: it goes into the SQL.
pub(super) fn oid(id: &str) -> Result<u32> {
    id.trim()
        .parse()
        .map_err(|_| Error::Other(format!("`{id}` is not a routine id")))
}

/// The condition on `pg_proc p` that picks `routine`.
fn routine_filter(routine: RoutineRef<'_>) -> Result<String> {
    Ok(match routine {
        RoutineRef::Id(id) => format!("p.oid = {}", oid(id)?),
        // As the navigator lists it, `add(a integer, b integer)`: the argument names are not
        // a signature `regprocedure` reads, so the text is compared, as the DDL does.
        RoutineRef::Name { schema, name } => format!(
            "n.nspname = {} AND p.proname || '(' || pg_get_function_identity_arguments(p.oid) \
             || ')' = {}",
            Dialect::Postgres.quote_literal(schema),
            Dialect::Postgres.quote_literal(name)
        ),
    })
}

/// The routine `routine` picks: oid, schema, name, argument types, language, whether a
/// procedure, the type it returns, whether a trigger function, and how many of its input
/// parameters have a default.
pub(super) fn routine_sql(routine: RoutineRef<'_>) -> Result<String> {
    Ok(format!(
        "SELECT p.oid::text, n.nspname, p.proname, pg_get_function_identity_arguments(p.oid), \
         l.lanname, p.prokind = 'p', \
         CASE WHEN p.prokind = 'p' THEN NULL ELSE format_type(p.prorettype, NULL) END, \
         p.prorettype IN ('trigger'::regtype, 'event_trigger'::regtype), p.pronargdefaults \
         FROM pg_proc p \
         JOIN pg_namespace n ON n.oid = p.pronamespace \
         JOIN pg_language l ON l.oid = p.prolang \
         WHERE {}",
        routine_filter(routine)?
    ))
}

/// The parameters of routine `oid`, in order: name, type and mode.
pub(super) fn parameters_sql(oid: u32) -> String {
    format!(
        "SELECT coalesce(p.proargnames[a.n], ''), format_type(a.t, NULL), \
         coalesce(p.proargmodes[a.n]::text, 'i') \
         FROM pg_proc p, \
         unnest(coalesce(p.proallargtypes, p.proargtypes::oid[])) WITH ORDINALITY AS a(t, n) \
         WHERE p.oid = {oid} ORDER BY a.n"
    )
}

/// The routine of `row` ([`routine_sql`]) with its `parameters` ([`parameters_sql`]).
pub(super) fn routine(row: &TextRow, parameters: &[TextRow]) -> DebugRoutine {
    let defaults = usize::try_from(row.number(8).unwrap_or(0)).unwrap_or(0);
    let mut parameters: Vec<DebugParameter> = parameters
        .iter()
        .map(|p| DebugParameter {
            name: p.opt(0).filter(|n| !n.is_empty()),
            type_name: p.text(1),
            mode: match p.text(2).as_str() {
                "o" => ParameterMode::Out,
                "b" => ParameterMode::InOut,
                "v" => ParameterMode::Variadic,
                "t" => ParameterMode::Table,
                _ => ParameterMode::In,
            },
            has_default: false,
        })
        .collect();
    // The defaults are those of the last input parameters.
    for p in parameters
        .iter_mut()
        .filter(|p| p.mode.is_input())
        .rev()
        .take(defaults)
    {
        p.has_default = true;
    }
    let language = row.text(4);
    let trigger = row.flag(7);
    let refusal = if trigger {
        Some("a trigger function runs only when its trigger fires".to_owned())
    } else if language != "plpgsql" {
        Some(format!(
            "only PL/pgSQL routines can be debugged, not {language}"
        ))
    } else {
        None
    };
    DebugRoutine {
        id: row.text(0),
        schema: row.text(1),
        name: row.text(2),
        signature: row.text(3),
        language,
        procedure: row.flag(5),
        returns: row.opt(6),
        parameters,
        refusal,
    }
}

/// The statement that calls `routine` with `arguments`, one for each input parameter: a
/// function from a `SELECT`, which returns its rows, a procedure with `CALL`.
pub(super) fn call_sql(routine: &DebugRoutine, arguments: &[DebugArgument]) -> Result<String> {
    let inputs = routine
        .parameters
        .iter()
        .filter(|p| p.mode.is_input())
        .count();
    if arguments.len() != inputs {
        return Err(Error::Other(format!(
            "{} takes {inputs} arguments, not {}",
            routine.name,
            arguments.len()
        )));
    }
    // A default value can only be left out at the end: a call has no other way to skip one.
    let given = arguments
        .iter()
        .rposition(|a| *a != DebugArgument::Default)
        .map_or(0, |i| i + 1);
    let mut values = Vec::new();
    let mut arguments = arguments.iter();
    for p in &routine.parameters {
        if !p.mode.is_input() {
            // A procedure's OUT parameters take a placeholder; a function's are results.
            if routine.procedure && p.mode == ParameterMode::Out && values.len() < given {
                values.push(format!("NULL::{}", p.type_name));
            }
            continue;
        }
        let Some(argument) = arguments.next() else {
            break;
        };
        let value = match argument {
            DebugArgument::Value(text) => {
                format!("{}::{}", Dialect::Postgres.quote_literal(text), p.type_name)
            }
            DebugArgument::Null => format!("NULL::{}", p.type_name),
            DebugArgument::Default if !p.has_default => {
                return Err(Error::Other(format!(
                    "{} has no default value",
                    parameter_label(p)
                )));
            }
            DebugArgument::Default if values.len() < given => {
                return Err(Error::Other(format!(
                    "{} can keep its default value only if every later argument does too",
                    parameter_label(p)
                )));
            }
            DebugArgument::Default => continue,
        };
        let variadic = if p.mode == ParameterMode::Variadic {
            "VARIADIC "
        } else {
            ""
        };
        values.push(format!("{variadic}{value}"));
    }
    let name = Dialect::Postgres.qualified(Some(&routine.schema), &routine.name);
    let list = values.join(", ");
    Ok(if routine.procedure {
        format!("CALL {name}({list})")
    } else {
        format!("SELECT * FROM {name}({list})")
    })
}

/// How a parameter is named in messages.
fn parameter_label(p: &DebugParameter) -> String {
    p.name
        .as_deref()
        .map_or_else(|| format!("the {} parameter", p.type_name), str::to_owned)
}

/// Marks routine `oid` to stop at its first statement, in the session that runs it.
pub(super) fn break_on(oid: u32) -> String {
    format!("SELECT pldbg_oid_debug({oid})")
}

/// Attaches the debugger session to target `id` (from its `PLDBGBREAK` notice); returns the
/// handle the other statements take.
pub(super) fn attach(id: i32) -> String {
    format!("SELECT pldbg_attach_to_port({id})")
}

/// The columns of a `breakpoint` row: routine oid, line, routine name.
const POSITION: &str = "func::text, linenumber, targetname";

pub(super) fn first_stop(handle: i32) -> String {
    format!("SELECT {POSITION} FROM pldbg_wait_for_breakpoint({handle})")
}

pub(super) fn step(handle: i32, step: DebugStep) -> String {
    let function = match step {
        DebugStep::Into => "pldbg_step_into",
        DebugStep::Over => "pldbg_step_over",
        DebugStep::Continue => "pldbg_continue",
    };
    format!("SELECT {POSITION} FROM {function}({handle})")
}

pub(super) fn select_frame(handle: i32, frame: u32) -> String {
    format!("SELECT {POSITION} FROM pldbg_select_frame({handle}, {frame})")
}

/// The position of the first of `rows`, from a statement that returns a `breakpoint`.
pub(super) fn position(rows: &[TextRow]) -> Result<DebugPosition> {
    let row = rows
        .first()
        .filter(|r| r.opt(0).is_some())
        .ok_or_else(|| Error::Other("the debugger answered no position".to_owned()))?;
    Ok(DebugPosition {
        routine: row.text(0),
        name: row.text(2),
        line: line(row, 1),
    })
}

fn line(row: &TextRow, i: usize) -> i32 {
    row.opt(i).and_then(|l| l.trim().parse().ok()).unwrap_or(0)
}

pub(super) fn stack(handle: i32) -> String {
    format!("SELECT level, targetname, func::text, linenumber, args FROM pldbg_get_stack({handle})")
}

pub(super) fn frames(rows: &[TextRow]) -> Vec<DebugFrame> {
    rows.iter()
        .map(|r| DebugFrame {
            level: r.number(0).and_then(|l| u32::try_from(l).ok()).unwrap_or(0),
            position: DebugPosition {
                routine: r.text(2),
                name: r.text(1),
                line: line(r, 3),
            },
            arguments: r.text(4),
        })
        .collect()
}

pub(super) fn variables(handle: i32) -> String {
    format!(
        "SELECT name, varclass::text, linenumber, format_type(dtype, NULL), value, isconst, \
         isnotnull FROM pldbg_get_variables({handle})"
    )
}

pub(super) fn variable_rows(rows: &[TextRow]) -> Vec<DebugVariable> {
    rows.iter()
        .map(|r| DebugVariable {
            name: r.text(0),
            kind: if r.text(1) == "A" {
                VariableKind::Argument
            } else {
                VariableKind::Local
            },
            line: line(r, 2),
            type_name: r.text(3),
            value: r.text(4),
            constant: r.flag(5),
            not_null: r.flag(6),
        })
        .collect()
}

/// Gives variable `name` (declared on `line`) of the selected frame the value `value`:
/// PL/pgSQL evaluates `SELECT <value>`, so the text goes as a literal, cast on assignment.
pub(super) fn deposit(handle: i32, name: &str, line: i32, value: Option<&str>) -> String {
    let value = value.map_or_else(|| "NULL".to_owned(), |v| Dialect::Postgres.quote_literal(v));
    format!(
        "SELECT pldbg_deposit_value({handle}, {}, {line}, {})",
        Dialect::Postgres.quote_literal(name),
        Dialect::Postgres.quote_literal(&value)
    )
}

pub(super) fn breakpoint(handle: i32, oid: u32, line: i32, on: bool) -> String {
    let function = if on {
        "pldbg_set_breakpoint"
    } else {
        "pldbg_drop_breakpoint"
    };
    format!("SELECT {function}({handle}, {oid}, {line})")
}

pub(super) fn source(handle: i32, oid: u32) -> String {
    format!("SELECT pldbg_get_source({handle}, {oid})")
}

/// Makes the target end its call with an error, `canceling statement due to user request`.
pub(super) fn abort(handle: i32) -> String {
    format!("SELECT * FROM pldbg_abort_target({handle})")
}

/// The messages of a debugged call: the debugger's own notice left out.
pub(super) fn own_messages(messages: Vec<String>) -> Vec<String> {
    messages
        .into_iter()
        .filter(|m| platypus_core::debug_break_id(m).is_none())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(values: &[Option<&str>]) -> TextRow {
        values.iter().map(|v| v.map(str::to_owned)).collect()
    }

    #[test]
    fn tells_what_the_server_lacks() {
        let at = |created, available, preload: Option<&str>| {
            availability(&row(&[Some(created), Some(available), preload]))
        };
        assert_eq!(
            at("t", "t", Some("plugin_debugger")),
            DebugAvailability::Ready
        );
        assert_eq!(
            at("f", "t", Some("pg_stat_statements, plugin_debugger")),
            DebugAvailability::MissingExtension
        );
        assert_eq!(
            at("f", "t", Some("$libdir/plugin_debugger")),
            DebugAvailability::MissingExtension
        );
        assert_eq!(
            at("t", "t", Some("pg_stat_statements")),
            DebugAvailability::NotLoaded
        );
        assert_eq!(at("f", "f", Some("")), DebugAvailability::NotInstalled);
        // A setting the user may not read: the extension decides.
        assert_eq!(at("t", "t", None), DebugAvailability::Ready);
    }

    #[test]
    fn picks_routines_by_id_or_by_name() {
        assert!(
            routine_sql(RoutineRef::Id("16384"))
                .unwrap()
                .ends_with("p.oid = 16384")
        );
        assert!(routine_sql(RoutineRef::Id("1; DROP TABLE t")).is_err());
        let sql = routine_sql(RoutineRef::Name {
            schema: "Sales",
            name: "it's(a integer)",
        })
        .unwrap();
        assert!(
            sql.ends_with(
                "n.nspname = 'Sales' AND p.proname || '(' || \
                 pg_get_function_identity_arguments(p.oid) || ')' = 'it''s(a integer)'"
            ),
            "{sql}"
        );
    }

    fn function(parameters: &[(&str, &str, &str)], defaults: &str) -> DebugRoutine {
        let rows: Vec<TextRow> = parameters
            .iter()
            .map(|(n, t, m)| row(&[Some(n), Some(t), Some(m)]))
            .collect();
        routine(
            &row(&[
                Some("16384"),
                Some("public"),
                Some("f"),
                Some(""),
                Some("plpgsql"),
                Some("f"),
                Some("integer"),
                Some("f"),
                Some(defaults),
            ]),
            &rows,
        )
    }

    #[test]
    fn reads_parameters_and_their_defaults() {
        let f = function(
            &[
                ("a", "integer", "i"),
                ("", "text", "i"),
                ("c", "integer", "i"),
                ("r", "integer", "o"),
            ],
            "1",
        );
        let defaults: Vec<bool> = f.parameters.iter().map(|p| p.has_default).collect();
        assert_eq!(defaults, [false, false, true, false]);
        assert_eq!(f.parameters[1].name, None);
        assert_eq!(f.parameters[3].mode, ParameterMode::Out);
        assert_eq!(f.refusal, None);

        let sql = routine(
            &row(&[
                Some("1"),
                Some("public"),
                Some("g"),
                Some(""),
                Some("sql"),
                Some("f"),
                Some("integer"),
                Some("f"),
                Some("0"),
            ]),
            &[],
        );
        assert!(sql.refusal.unwrap().contains("not sql"));
        let trigger = routine(
            &row(&[
                Some("1"),
                Some("public"),
                Some("t"),
                Some(""),
                Some("plpgsql"),
                Some("f"),
                Some("trigger"),
                Some("t"),
                Some("0"),
            ]),
            &[],
        );
        assert!(trigger.refusal.unwrap().contains("trigger"));
    }

    #[test]
    fn writes_the_call() {
        let f = function(
            &[
                ("a", "integer", "i"),
                ("b", "text", "i"),
                ("c", "integer", "i"),
                ("r", "integer", "o"),
            ],
            "2",
        );
        let value = |v: &str| DebugArgument::Value(v.to_owned());
        assert_eq!(
            call_sql(&f, &[value("1"), value("it's"), DebugArgument::Null]).unwrap(),
            "SELECT * FROM public.f('1'::integer, 'it''s'::text, NULL::integer)"
        );
        assert_eq!(
            call_sql(
                &f,
                &[value("1"), DebugArgument::Default, DebugArgument::Default]
            )
            .unwrap(),
            "SELECT * FROM public.f('1'::integer)"
        );
        // A default in the middle, or where there is none, cannot be left out.
        assert!(call_sql(&f, &[value("1"), DebugArgument::Default, value("3")]).is_err());
        assert!(
            call_sql(
                &f,
                &[
                    DebugArgument::Default,
                    DebugArgument::Default,
                    DebugArgument::Default
                ]
            )
            .is_err()
        );
        assert!(call_sql(&f, &[value("1")]).is_err());

        let mut p = function(
            &[
                ("a", "integer", "i"),
                ("r", "integer", "o"),
                ("v", "integer[]", "v"),
            ],
            "0",
        );
        p.procedure = true;
        assert_eq!(
            call_sql(&p, &[value("1"), value("{2,3}")]).unwrap(),
            "CALL public.f('1'::integer, NULL::integer, VARIADIC '{2,3}'::integer[])"
        );
    }

    #[test]
    fn reads_positions_frames_and_variables() {
        let stop = position(&[row(&[Some("16384"), Some("7"), Some("f(integer)")])]).unwrap();
        assert_eq!(
            stop,
            DebugPosition {
                routine: "16384".to_owned(),
                name: "f(integer)".to_owned(),
                line: 7
            }
        );
        assert!(position(&[]).is_err());
        assert!(position(&[row(&[None, None, None])]).is_err());

        let frames = frames(&[
            row(&[
                Some("0"),
                Some("g(integer)"),
                Some("2"),
                Some("3"),
                Some("x=1"),
            ]),
            row(&[
                Some("1"),
                Some("f(integer)"),
                Some("1"),
                Some("9"),
                Some("n=4"),
            ]),
        ]);
        assert_eq!(frames[1].level, 1);
        assert_eq!(frames[1].position.line, 9);
        assert_eq!(frames[0].arguments, "x=1");

        let vars = variable_rows(&[row(&[
            Some("n"),
            Some("A"),
            Some("0"),
            Some("integer"),
            Some("4"),
            Some("f"),
            Some("t"),
        ])]);
        assert_eq!(vars[0].kind, VariableKind::Argument);
        assert!(vars[0].not_null && !vars[0].constant);
    }

    #[test]
    fn writes_debugger_statements() {
        assert_eq!(
            step(3, DebugStep::Over),
            "SELECT func::text, linenumber, targetname FROM pldbg_step_over(3)"
        );
        assert_eq!(
            deposit(3, "total", 4, Some("it's")),
            "SELECT pldbg_deposit_value(3, 'total', 4, '''it''''s''')"
        );
        assert_eq!(
            deposit(3, "total", -1, None),
            "SELECT pldbg_deposit_value(3, 'total', -1, 'NULL')"
        );
        assert_eq!(
            breakpoint(3, 16384, 5, false),
            "SELECT pldbg_drop_breakpoint(3, 16384, 5)"
        );
        assert_eq!(
            own_messages(vec![
                "NOTICE: PLDBGBREAK:4".to_owned(),
                "NOTICE: i = 1".to_owned()
            ]),
            ["NOTICE: i = 1"]
        );
    }
}
