//! `CREATE` statements for the objects the table editor does not cover: views, sequences,
//! functions and triggers. The object creation dialog fills an [`ObjectForm`]; [`create_object`]
//! turns it into the statements of the connection's dialect, or says in plain English why that
//! database cannot do it. Nothing here touches a database.

use platypus_sql::Dialect;
use serde::{Deserialize, Serialize};

/// What the creation dialog collects, one variant per kind of object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObjectForm {
    View {
        name: String,
        /// The `SELECT` the view is defined by.
        query: String,
        /// Replace a view of that name instead of failing.
        #[serde(default)]
        replace: bool,
        #[serde(default)]
        materialized: bool,
    },
    Sequence {
        name: String,
        #[serde(default)]
        start: Option<i64>,
        #[serde(default)]
        increment: Option<i64>,
        #[serde(default)]
        min: Option<i64>,
        #[serde(default)]
        max: Option<i64>,
        #[serde(default)]
        cycle: bool,
        #[serde(default)]
        cache: Option<i64>,
    },
    Function {
        name: String,
        #[serde(default)]
        params: Vec<Param>,
        /// The return type; for DuckDB macros, which have none, it is ignored.
        returns: String,
        /// PostgreSQL only; `plpgsql` when left out.
        #[serde(default)]
        language: Option<String>,
        body: String,
        #[serde(default)]
        replace: bool,
    },
    Trigger {
        name: String,
        table: String,
        timing: Timing,
        events: Vec<Event>,
        #[serde(default)]
        for_each_row: bool,
        /// A condition, without the `WHEN` keyword.
        #[serde(default)]
        when: Option<String>,
        /// The statements the trigger runs, without `BEGIN`/`END` unless it needs its own block.
        body: String,
        #[serde(default)]
        replace: bool,
    },
}

/// A function parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub data_type: String,
    /// `IN`, `OUT` or `INOUT`; `None` for the database's default.
    #[serde(default)]
    pub mode: Option<String>,
}

/// When a trigger fires relative to the statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Timing {
    Before,
    After,
    InsteadOf,
}

impl Timing {
    fn sql(self) -> &'static str {
        match self {
            Self::Before => "BEFORE",
            Self::After => "AFTER",
            Self::InsteadOf => "INSTEAD OF",
        }
    }
}

/// The statement that fires a trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Insert,
    Update,
    Delete,
}

impl Event {
    fn sql(self) -> &'static str {
        match self {
            Self::Insert => "INSERT",
            Self::Update => "UPDATE",
            Self::Delete => "DELETE",
        }
    }
}

const UNSUPPORTED: &str = "Not supported for this database.";

/// The statements, without a final `;`, that create the object in `schema`, to run in order.
/// Oracle's PL/SQL blocks keep their own closing `;`, which that dialect needs.
///
/// # Errors
/// A message for the user when the form is incomplete or the database cannot do what it asks.
pub fn create_object(
    dialect: Dialect,
    schema: &str,
    form: &ObjectForm,
) -> Result<Vec<String>, String> {
    validate(form)?;
    match form {
        ObjectForm::View {
            name,
            query,
            replace,
            materialized,
        } => view(dialect, schema, name.trim(), query, *replace, *materialized),
        ObjectForm::Sequence {
            name,
            start,
            increment,
            min,
            max,
            cycle,
            cache,
        } => {
            let options = SequenceOptions {
                start: *start,
                increment: *increment,
                min: *min,
                max: *max,
                cycle: *cycle,
                cache: *cache,
            };
            sequence(dialect, schema, name.trim(), &options)
        }
        ObjectForm::Function {
            name,
            params,
            returns,
            language,
            body,
            replace,
        } => {
            let function = FunctionSpec {
                name: name.trim(),
                params,
                returns: returns.trim(),
                language: language.as_deref().map(str::trim).filter(|l| !l.is_empty()),
                body,
                replace: *replace,
            };
            function_sql(dialect, schema, &function)
        }
        ObjectForm::Trigger {
            name,
            table,
            timing,
            events,
            for_each_row,
            when,
            body,
            replace,
        } => {
            let trigger = TriggerSpec {
                name: name.trim(),
                table: table.trim(),
                timing: *timing,
                events,
                for_each_row: *for_each_row,
                when: when.as_deref().map(str::trim).filter(|w| !w.is_empty()),
                body,
                replace: *replace,
            };
            trigger_sql(dialect, schema, &trigger)
        }
    }
}

fn validate(form: &ObjectForm) -> Result<(), String> {
    match form {
        ObjectForm::View { name, query, .. } => {
            need(name, "A view needs a name.")?;
            need(&strip_semicolons(query), "A view needs a query.")
        }
        ObjectForm::Sequence {
            name,
            increment,
            cache,
            ..
        } => {
            need(name, "A sequence needs a name.")?;
            if *increment == Some(0) {
                return Err("A sequence cannot increment by 0.".into());
            }
            if cache.is_some_and(|c| c < 1) {
                return Err("The cache size must be at least 1.".into());
            }
            Ok(())
        }
        ObjectForm::Function {
            name, params, body, ..
        } => {
            need(name, "A function needs a name.")?;
            need(&strip_semicolons(body), "A function needs a body.")?;
            for p in params {
                need(&p.name, "Every parameter needs a name.")?;
            }
            Ok(())
        }
        ObjectForm::Trigger {
            name,
            table,
            events,
            body,
            ..
        } => {
            need(name, "A trigger needs a name.")?;
            need(table, "A trigger needs a table.")?;
            if events.is_empty() {
                return Err("A trigger needs at least one event.".into());
            }
            need(&strip_semicolons(body), "A trigger needs a body.")
        }
    }
}

fn need(value: &str, message: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(message.into())
    } else {
        Ok(())
    }
}

/// The text without surrounding blanks and without a final `;`, which the statement adds
/// itself where the dialect wants one.
fn strip_semicolons(sql: &str) -> String {
    let mut text = sql.trim();
    while let Some(rest) = text.strip_suffix(';') {
        text = rest.trim_end();
    }
    text.to_string()
}

/// Whether `text` starts with the word `word`, ignoring case.
fn starts_with_word(text: &str, word: &str) -> bool {
    let text = text.trim_start();
    text.get(..word.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(word))
        && text[word.len()..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
}

/// A procedural body as a block ending in `END` (no `;`): a body that already starts with
/// `BEGIN` or `DECLARE` is kept, anything else is wrapped.
fn block(body: &str) -> String {
    let body = strip_semicolons(body);
    if starts_with_word(&body, "BEGIN") || starts_with_word(&body, "DECLARE") {
        body
    } else {
        format!("BEGIN\n{body};\nEND")
    }
}

fn view(
    dialect: Dialect,
    schema: &str,
    name: &str,
    query: &str,
    replace: bool,
    materialized: bool,
) -> Result<Vec<String>, String> {
    let target = dialect.qualified(Some(schema), name);
    let query = strip_semicolons(query);
    if materialized {
        return materialized_view(dialect, &target, &query, replace);
    }
    let create = match dialect {
        Dialect::Postgres | Dialect::MySql | Dialect::DuckDb | Dialect::Oracle => {
            let or_replace = if replace { "OR REPLACE " } else { "" };
            format!("CREATE {or_replace}VIEW {target} AS {query}")
        }
        Dialect::SqlServer => {
            let or_alter = if replace { "OR ALTER " } else { "" };
            format!("CREATE {or_alter}VIEW {target} AS {query}")
        }
        Dialect::Sqlite => {
            let mut statements = Vec::new();
            if replace {
                statements.push(format!("DROP VIEW IF EXISTS {target}"));
            }
            statements.push(format!("CREATE VIEW {target} AS {query}"));
            return Ok(statements);
        }
        _ => return Err(UNSUPPORTED.into()),
    };
    Ok(vec![create])
}

fn materialized_view(
    dialect: Dialect,
    target: &str,
    query: &str,
    replace: bool,
) -> Result<Vec<String>, String> {
    let create = format!("CREATE MATERIALIZED VIEW {target} AS {query}");
    match dialect {
        // PostgreSQL has no OR REPLACE for these, so the old one is dropped first.
        Dialect::Postgres if replace => Ok(vec![
            format!("DROP MATERIALIZED VIEW IF EXISTS {target}"),
            create,
        ]),
        Dialect::Oracle if replace => {
            Err("Oracle cannot replace a materialized view; drop it first.".into())
        }
        Dialect::Postgres | Dialect::Oracle => Ok(vec![create]),
        Dialect::MySql | Dialect::Sqlite | Dialect::DuckDb | Dialect::SqlServer => {
            Err("This database has no materialized views.".into())
        }
        _ => Err(UNSUPPORTED.into()),
    }
}

struct SequenceOptions {
    start: Option<i64>,
    increment: Option<i64>,
    min: Option<i64>,
    max: Option<i64>,
    cycle: bool,
    cache: Option<i64>,
}

fn sequence(
    dialect: Dialect,
    schema: &str,
    name: &str,
    options: &SequenceOptions,
) -> Result<Vec<String>, String> {
    match dialect {
        Dialect::Sqlite => return Err("SQLite has no sequences.".into()),
        Dialect::Postgres
        | Dialect::SqlServer
        | Dialect::Oracle
        | Dialect::DuckDb
        | Dialect::MySql => {}
        _ => return Err(UNSUPPORTED.into()),
    }
    if dialect == Dialect::DuckDb && options.cache.is_some() {
        return Err("DuckDB sequences have no cache option.".into());
    }
    let mut sql = format!("CREATE SEQUENCE {}", dialect.qualified(Some(schema), name));
    let numbers = [
        (" START WITH ", options.start),
        (" INCREMENT BY ", options.increment),
        (" MINVALUE ", options.min),
        (" MAXVALUE ", options.max),
    ];
    for (keyword, value) in numbers {
        if let Some(value) = value {
            sql.push_str(keyword);
            sql.push_str(&value.to_string());
        }
    }
    if options.cycle {
        sql.push_str(" CYCLE");
    }
    if let Some(cache) = options.cache {
        sql.push_str(" CACHE ");
        sql.push_str(&cache.to_string());
    }
    Ok(vec![sql])
}

struct FunctionSpec<'a> {
    name: &'a str,
    params: &'a [Param],
    returns: &'a str,
    language: Option<&'a str>,
    body: &'a str,
    replace: bool,
}

fn function_sql(
    dialect: Dialect,
    schema: &str,
    function: &FunctionSpec,
) -> Result<Vec<String>, String> {
    let target = dialect.qualified(Some(schema), function.name);
    if dialect != Dialect::DuckDb {
        need(function.returns, "A function needs a return type.")?;
    }
    let ident = |n: &str| dialect.quote_ident(n).into_owned();
    match dialect {
        Dialect::Postgres => {
            let params = typed_params(function.params, ident, true)?;
            Ok(vec![postgres_function(&target, &params, function)])
        }
        Dialect::MySql => {
            let params = typed_params(function.params, ident, false)?;
            Ok(mysql_function(&target, &params, function))
        }
        Dialect::SqlServer => {
            let at = |n: &str| format!("@{}", n.trim_start_matches('@'));
            let params = typed_params(function.params, at, false)?;
            Ok(sqlserver_function(&target, &params, function))
        }
        Dialect::Oracle => {
            let params = typed_params(function.params, ident, false)?;
            Ok(vec![oracle_function(&target, &params, function)])
        }
        Dialect::DuckDb => Ok(duckdb_macro(&target, function)),
        Dialect::Sqlite => Err("SQLite has no stored functions.".into()),
        _ => Err(UNSUPPORTED.into()),
    }
}

fn typed_params(
    params: &[Param],
    name_of: impl Fn(&str) -> String,
    mode_first: bool,
) -> Result<String, String> {
    let mut parts = Vec::with_capacity(params.len());
    for p in params {
        need(&p.data_type, "Every parameter needs a type.")?;
        let name = name_of(p.name.trim());
        let ty = p.data_type.trim();
        let mode = p.mode.as_deref().map(str::trim).filter(|m| !m.is_empty());
        let mode = match mode {
            Some(m) if !matches!(m.to_ascii_uppercase().as_str(), "IN" | "OUT" | "INOUT") => {
                return Err(format!("Unknown parameter mode \"{m}\"."));
            }
            Some(m) => Some(m.to_ascii_uppercase()),
            None => None,
        };
        parts.push(match (mode, mode_first) {
            (Some(m), true) => format!("{m} {name} {ty}"),
            (Some(m), false) => format!("{name} {m} {ty}"),
            (None, _) => format!("{name} {ty}"),
        });
    }
    Ok(parts.join(", "))
}

/// A dollar-quote tag that does not occur in `body`, so that the body cannot end the string.
fn dollar_tag(body: &str) -> String {
    let mut tag = "$body$".to_string();
    let mut n = 0;
    while body.contains(&tag) {
        n += 1;
        tag = format!("$body{n}$");
    }
    tag
}

fn postgres_function(target: &str, params: &str, function: &FunctionSpec) -> String {
    let language = function.language.unwrap_or("plpgsql");
    let body = if language.eq_ignore_ascii_case("plpgsql") {
        format!("{};", block(function.body))
    } else {
        strip_semicolons(function.body)
    };
    let tag = dollar_tag(&body);
    let or_replace = if function.replace { "OR REPLACE " } else { "" };
    format!(
        "CREATE {or_replace}FUNCTION {target}({params}) RETURNS {} LANGUAGE {language} AS {tag}\n{body}\n{tag}",
        function.returns
    )
}

fn mysql_function(target: &str, params: &str, function: &FunctionSpec) -> Vec<String> {
    let mut statements = Vec::new();
    if function.replace {
        statements.push(format!("DROP FUNCTION IF EXISTS {target}"));
    }
    statements.push(format!(
        "CREATE FUNCTION {target}({params}) RETURNS {} DETERMINISTIC\n{}",
        function.returns,
        block(function.body)
    ));
    statements
}

fn sqlserver_function(target: &str, params: &str, function: &FunctionSpec) -> Vec<String> {
    let or_alter = if function.replace { "OR ALTER " } else { "" };
    vec![format!(
        "CREATE {or_alter}FUNCTION {target}({params}) RETURNS {} AS\n{}",
        function.returns,
        block(function.body)
    )]
}

fn oracle_function(target: &str, params: &str, function: &FunctionSpec) -> String {
    let or_replace = if function.replace { "OR REPLACE " } else { "" };
    // Oracle writes `(a IN NUMBER)` and a bare `RETURN` for the type; with no parameters the
    // parentheses must go.
    let params = if params.is_empty() {
        String::new()
    } else {
        format!("({params})")
    };
    format!(
        "CREATE {or_replace}FUNCTION {target}{params} RETURN {} AS\n{};",
        function.returns,
        block(function.body)
    )
}

fn duckdb_macro(target: &str, function: &FunctionSpec) -> Vec<String> {
    let names: Vec<String> = function
        .params
        .iter()
        .map(|p| Dialect::DuckDb.quote_ident(p.name.trim()).into_owned())
        .collect();
    let or_replace = if function.replace { "OR REPLACE " } else { "" };
    vec![format!(
        "CREATE {or_replace}MACRO {target}({}) AS {}",
        names.join(", "),
        strip_semicolons(function.body)
    )]
}

struct TriggerSpec<'a> {
    name: &'a str,
    table: &'a str,
    timing: Timing,
    events: &'a [Event],
    for_each_row: bool,
    when: Option<&'a str>,
    body: &'a str,
    replace: bool,
}

impl TriggerSpec<'_> {
    /// The events once each, in a fixed order, whatever order the form lists them in.
    fn sorted_events(&self) -> Vec<Event> {
        [Event::Insert, Event::Update, Event::Delete]
            .into_iter()
            .filter(|e| self.events.contains(e))
            .collect()
    }

    fn event_list(&self, separator: &str) -> String {
        let events: Vec<&str> = self.sorted_events().iter().map(|e| e.sql()).collect();
        events.join(separator)
    }

    /// The only event of a dialect whose triggers fire on one.
    fn single_event(&self, who: &str) -> Result<Event, String> {
        match self.sorted_events().as_slice() {
            [one] => Ok(*one),
            _ => Err(format!("{who} triggers fire on one event.")),
        }
    }
}

fn trigger_sql(
    dialect: Dialect,
    schema: &str,
    trigger: &TriggerSpec,
) -> Result<Vec<String>, String> {
    match dialect {
        Dialect::Postgres => Ok(postgres_trigger(schema, trigger)),
        Dialect::MySql => mysql_trigger(schema, trigger),
        Dialect::Sqlite => sqlite_trigger(schema, trigger),
        Dialect::SqlServer => sqlserver_trigger(schema, trigger),
        Dialect::Oracle => oracle_trigger(schema, trigger),
        Dialect::DuckDb => Err("DuckDB has no triggers.".into()),
        _ => Err(UNSUPPORTED.into()),
    }
}

fn postgres_trigger(schema: &str, trigger: &TriggerSpec) -> Vec<String> {
    let d = Dialect::Postgres;
    let function = d.qualified(Some(schema), &format!("{}_fn", trigger.name));
    let events = trigger.sorted_events();
    let body = strip_semicolons(trigger.body);
    // An early RETURN inside an IF still needs a final one: only a body ending in RETURN
    // goes without.
    let last = body.rsplit(';').next().unwrap_or_default().trim();
    let ret = if starts_with_word(last, "RETURN") {
        String::new()
    } else if !trigger.for_each_row {
        // A statement-level trigger has no row; its result is ignored.
        "\nRETURN NULL;".to_string()
    } else if events == [Event::Delete] {
        "\nRETURN OLD;".to_string()
    } else {
        "\nRETURN NEW;".to_string()
    };
    let tag = dollar_tag(&body);
    // Without Replace, a function of that name already there fails the script rather than
    // being overwritten.
    let or_replace = if trigger.replace { "OR REPLACE " } else { "" };
    let function_sql = format!(
        "CREATE {or_replace}FUNCTION {function}() RETURNS trigger LANGUAGE plpgsql AS {tag}\nBEGIN\n{body};{ret}\nEND\n{tag}"
    );
    let each = if trigger.for_each_row {
        "ROW"
    } else {
        "STATEMENT"
    };
    let when = trigger
        .when
        .map_or(String::new(), |w| format!(" WHEN ({w})"));
    let create = format!(
        "CREATE {or_replace}TRIGGER {} {} {} ON {} FOR EACH {each}{when} EXECUTE FUNCTION {function}()",
        d.quote_ident(trigger.name),
        trigger.timing.sql(),
        trigger.event_list(" OR "),
        d.qualified(Some(schema), trigger.table),
    );
    vec![function_sql, create]
}

fn mysql_trigger(schema: &str, trigger: &TriggerSpec) -> Result<Vec<String>, String> {
    let d = Dialect::MySql;
    let event = trigger.single_event("MySQL")?;
    if trigger.timing == Timing::InsteadOf {
        return Err("MySQL triggers have no INSTEAD OF timing.".into());
    }
    if trigger.when.is_some() {
        return Err("MySQL triggers have no WHEN condition; use IF in the body.".into());
    }
    let target = d.qualified(Some(schema), trigger.name);
    let mut statements = Vec::new();
    if trigger.replace {
        statements.push(format!("DROP TRIGGER IF EXISTS {target}"));
    }
    statements.push(format!(
        "CREATE TRIGGER {target} {} {} ON {} FOR EACH ROW\n{}",
        trigger.timing.sql(),
        event.sql(),
        d.qualified(Some(schema), trigger.table),
        block(trigger.body)
    ));
    Ok(statements)
}

fn sqlite_trigger(schema: &str, trigger: &TriggerSpec) -> Result<Vec<String>, String> {
    let d = Dialect::Sqlite;
    let event = trigger.single_event("SQLite")?;
    let target = d.qualified(Some(schema), trigger.name);
    let mut statements = Vec::new();
    if trigger.replace {
        statements.push(format!("DROP TRIGGER IF EXISTS {target}"));
    }
    let each = if trigger.for_each_row {
        " FOR EACH ROW"
    } else {
        ""
    };
    let when = trigger.when.map_or(String::new(), |w| format!(" WHEN {w}"));
    // SQLite always wants a block, so a body is never taken as one already.
    let body = strip_semicolons(trigger.body);
    statements.push(format!(
        "CREATE TRIGGER {target} {} {} ON {}{each}{when}\nBEGIN\n{body};\nEND",
        trigger.timing.sql(),
        event.sql(),
        d.quote_ident(trigger.table),
    ));
    Ok(statements)
}

fn sqlserver_trigger(schema: &str, trigger: &TriggerSpec) -> Result<Vec<String>, String> {
    let d = Dialect::SqlServer;
    if trigger.timing == Timing::Before {
        return Err("SQL Server triggers have no BEFORE timing; use INSTEAD OF.".into());
    }
    if trigger.when.is_some() {
        return Err("SQL Server triggers have no WHEN condition; use IF in the body.".into());
    }
    let or_alter = if trigger.replace { "OR ALTER " } else { "" };
    Ok(vec![format!(
        "CREATE {or_alter}TRIGGER {} ON {} {} {} AS\n{}",
        d.qualified(Some(schema), trigger.name),
        d.qualified(Some(schema), trigger.table),
        trigger.timing.sql(),
        trigger.event_list(", "),
        block(trigger.body)
    )])
}

fn oracle_trigger(schema: &str, trigger: &TriggerSpec) -> Result<Vec<String>, String> {
    let d = Dialect::Oracle;
    if trigger.when.is_some() && !trigger.for_each_row {
        return Err("An Oracle trigger needs FOR EACH ROW to have a WHEN condition.".into());
    }
    let or_replace = if trigger.replace { "OR REPLACE " } else { "" };
    let each = if trigger.for_each_row {
        "\nFOR EACH ROW"
    } else {
        ""
    };
    let when = trigger
        .when
        .map_or(String::new(), |w| format!("\nWHEN ({w})"));
    Ok(vec![format!(
        "CREATE {or_replace}TRIGGER {} {} {} ON {}{each}{when}\n{};",
        d.qualified(Some(schema), trigger.name),
        trigger.timing.sql(),
        trigger.event_list(" OR "),
        d.qualified(Some(schema), trigger.table),
        block(trigger.body)
    )])
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Dialect; 6] = [
        Dialect::Postgres,
        Dialect::MySql,
        Dialect::Sqlite,
        Dialect::SqlServer,
        Dialect::Oracle,
        Dialect::DuckDb,
    ];

    fn view_form(replace: bool, materialized: bool) -> ObjectForm {
        ObjectForm::View {
            name: "v".into(),
            query: "SELECT 1;".into(),
            replace,
            materialized,
        }
    }

    fn seq_form() -> ObjectForm {
        ObjectForm::Sequence {
            name: "q".into(),
            start: Some(10),
            increment: Some(5),
            min: Some(1),
            max: Some(1000),
            cycle: true,
            cache: Some(20),
        }
    }

    fn param(name: &str, ty: &str, mode: Option<&str>) -> Param {
        Param {
            name: name.into(),
            data_type: ty.into(),
            mode: mode.map(Into::into),
        }
    }

    fn func(params: Vec<Param>, returns: &str, body: &str, replace: bool) -> ObjectForm {
        ObjectForm::Function {
            name: "f".into(),
            params,
            returns: returns.into(),
            language: None,
            body: body.into(),
            replace,
        }
    }

    fn trig(timing: Timing, events: &[Event], when: Option<&str>, replace: bool) -> ObjectForm {
        ObjectForm::Trigger {
            name: "t".into(),
            table: "tbl".into(),
            timing,
            events: events.to_vec(),
            for_each_row: true,
            when: when.map(Into::into),
            body: "UPDATE log SET n = n + 1;".into(),
            replace,
        }
    }

    fn run(d: Dialect, form: &ObjectForm) -> Result<Vec<String>, String> {
        create_object(d, "s", form)
    }

    fn one(d: Dialect, form: &ObjectForm) -> String {
        let mut sql = run(d, form).unwrap();
        assert_eq!(sql.len(), 1, "{sql:?}");
        sql.remove(0)
    }

    #[test]
    fn views_per_dialect() {
        let plain = view_form(false, false);
        assert_eq!(
            one(Dialect::Postgres, &plain),
            "CREATE VIEW s.v AS SELECT 1"
        );
        assert_eq!(
            one(Dialect::SqlServer, &plain),
            "CREATE VIEW s.v AS SELECT 1"
        );
        let replace = view_form(true, false);
        for d in [Dialect::Postgres, Dialect::MySql, Dialect::DuckDb] {
            assert_eq!(one(d, &replace), "CREATE OR REPLACE VIEW s.v AS SELECT 1");
        }
        assert_eq!(
            one(Dialect::Oracle, &replace),
            format!(
                "CREATE OR REPLACE VIEW {} AS SELECT 1",
                Dialect::Oracle.qualified(Some("s"), "v")
            )
        );
        assert_eq!(
            one(Dialect::SqlServer, &replace),
            "CREATE OR ALTER VIEW s.v AS SELECT 1"
        );
        assert_eq!(
            run(Dialect::Sqlite, &replace).unwrap(),
            ["DROP VIEW IF EXISTS s.v", "CREATE VIEW s.v AS SELECT 1"]
        );
        assert_eq!(
            run(Dialect::Sqlite, &plain).unwrap(),
            ["CREATE VIEW s.v AS SELECT 1"]
        );
    }

    #[test]
    fn materialized_views() {
        let mv = view_form(false, true);
        assert_eq!(
            one(Dialect::Postgres, &mv),
            "CREATE MATERIALIZED VIEW s.v AS SELECT 1"
        );
        let replaced = run(Dialect::Postgres, &view_form(true, true)).unwrap();
        assert_eq!(replaced[0], "DROP MATERIALIZED VIEW IF EXISTS s.v");
        assert_eq!(replaced[1], "CREATE MATERIALIZED VIEW s.v AS SELECT 1");
        assert!(one(Dialect::Oracle, &mv).starts_with("CREATE MATERIALIZED VIEW "));
        assert!(
            run(Dialect::Oracle, &view_form(true, true))
                .unwrap_err()
                .contains("drop it first")
        );
        for d in [
            Dialect::MySql,
            Dialect::Sqlite,
            Dialect::DuckDb,
            Dialect::SqlServer,
        ] {
            assert_eq!(
                run(d, &mv).unwrap_err(),
                "This database has no materialized views."
            );
        }
    }

    #[test]
    fn view_errors_and_quoting() {
        let empty = ObjectForm::View {
            name: "v".into(),
            query: " ; ".into(),
            replace: false,
            materialized: false,
        };
        for d in ALL {
            assert_eq!(run(d, &empty).unwrap_err(), "A view needs a query.");
        }
        let nameless = ObjectForm::View {
            name: "  ".into(),
            query: "SELECT 1".into(),
            replace: false,
            materialized: false,
        };
        assert_eq!(
            run(Dialect::Postgres, &nameless).unwrap_err(),
            "A view needs a name."
        );
        let odd = ObjectForm::View {
            name: "my \"view\"".into(),
            query: "SELECT 1".into(),
            replace: false,
            materialized: false,
        };
        assert_eq!(
            one(Dialect::Postgres, &odd),
            "CREATE VIEW s.\"my \"\"view\"\"\" AS SELECT 1"
        );
        assert_eq!(
            run(Dialect::ClickHouse, &view_form(false, false)).unwrap_err(),
            UNSUPPORTED
        );
    }

    #[test]
    fn sequences() {
        let full = " START WITH 10 INCREMENT BY 5 MINVALUE 1 MAXVALUE 1000 CYCLE CACHE 20";
        for d in [Dialect::Postgres, Dialect::SqlServer, Dialect::MySql] {
            assert_eq!(one(d, &seq_form()), format!("CREATE SEQUENCE s.q{full}"));
        }
        assert!(one(Dialect::Oracle, &seq_form()).ends_with(full));
        let bare = ObjectForm::Sequence {
            name: "q".into(),
            start: None,
            increment: None,
            min: None,
            max: None,
            cycle: false,
            cache: None,
        };
        assert_eq!(one(Dialect::Postgres, &bare), "CREATE SEQUENCE s.q");
        assert_eq!(
            run(Dialect::Sqlite, &bare).unwrap_err(),
            "SQLite has no sequences."
        );
        assert_eq!(run(Dialect::ClickHouse, &bare).unwrap_err(), UNSUPPORTED);
        assert_eq!(
            run(Dialect::DuckDb, &seq_form()).unwrap_err(),
            "DuckDB sequences have no cache option."
        );
        let duck = ObjectForm::Sequence {
            name: "q".into(),
            start: Some(1),
            increment: None,
            min: None,
            max: None,
            cycle: false,
            cache: None,
        };
        assert_eq!(
            one(Dialect::DuckDb, &duck),
            "CREATE SEQUENCE s.q START WITH 1"
        );
        let zero = ObjectForm::Sequence {
            name: "q".into(),
            start: None,
            increment: Some(0),
            min: None,
            max: None,
            cycle: false,
            cache: None,
        };
        assert_eq!(
            run(Dialect::Postgres, &zero).unwrap_err(),
            "A sequence cannot increment by 0."
        );
        let negative = ObjectForm::Sequence {
            name: "q".into(),
            start: None,
            increment: Some(-1),
            min: None,
            max: None,
            cycle: false,
            cache: Some(0),
        };
        assert!(run(Dialect::Postgres, &negative).is_err());
    }

    #[test]
    fn postgres_function_uses_dollar_quoting() {
        let form = func(
            vec![
                param("a", "integer", None),
                param("b", "integer", Some("in")),
            ],
            "integer",
            "RETURN a + b;",
            true,
        );
        assert_eq!(
            one(Dialect::Postgres, &form),
            "CREATE OR REPLACE FUNCTION s.f(a integer, IN b integer) RETURNS integer LANGUAGE plpgsql AS $body$\nBEGIN\nRETURN a + b;\nEND;\n$body$"
        );
    }

    #[test]
    fn postgres_function_language_and_tag() {
        let form = ObjectForm::Function {
            name: "f".into(),
            params: vec![],
            returns: "text".into(),
            language: Some("sql".into()),
            body: "SELECT '$body$'".into(),
            replace: false,
        };
        assert_eq!(
            one(Dialect::Postgres, &form),
            "CREATE FUNCTION s.f() RETURNS text LANGUAGE sql AS $body1$\nSELECT '$body$'\n$body1$"
        );
        assert_eq!(dollar_tag("$body$ $body1$"), "$body2$");
        assert_eq!(dollar_tag("x"), "$body$");
    }

    #[test]
    fn mysql_function_wraps_body() {
        let form = func(vec![param("a", "INT", None)], "INT", "RETURN a * 2;", true);
        assert_eq!(
            run(Dialect::MySql, &form).unwrap(),
            [
                "DROP FUNCTION IF EXISTS s.f",
                "CREATE FUNCTION s.f(a INT) RETURNS INT DETERMINISTIC\nBEGIN\nRETURN a * 2;\nEND"
            ]
        );
        let begun = func(vec![], "INT", "begin RETURN 1; end;", false);
        assert_eq!(
            run(Dialect::MySql, &begun).unwrap(),
            ["CREATE FUNCTION s.f() RETURNS INT DETERMINISTIC\nbegin RETURN 1; end"]
        );
    }

    #[test]
    fn sqlserver_function_prefixes_params() {
        let form = func(
            vec![param("a", "int", None), param("@b", "int", None)],
            "int",
            "RETURN @a + @b;",
            true,
        );
        assert_eq!(
            one(Dialect::SqlServer, &form),
            "CREATE OR ALTER FUNCTION s.f(@a int, @b int) RETURNS int AS\nBEGIN\nRETURN @a + @b;\nEND"
        );
    }

    #[test]
    fn oracle_function_keeps_final_semicolon() {
        let form = func(
            vec![param("A", "NUMBER", Some("IN"))],
            "NUMBER",
            "RETURN A;",
            true,
        );
        assert_eq!(
            one(Dialect::Oracle, &form),
            "CREATE OR REPLACE FUNCTION \"s\".\"f\"(A IN NUMBER) RETURN NUMBER AS\nBEGIN\nRETURN A;\nEND;"
        );
        let none = func(vec![], "NUMBER", "RETURN 1;", false);
        assert!(
            one(Dialect::Oracle, &none).starts_with("CREATE FUNCTION \"s\".\"f\" RETURN NUMBER AS")
        );
    }

    #[test]
    fn duckdb_macro_ignores_types() {
        let form = func(
            vec![param("a", "INT", None), param("b", "INT", None)],
            "",
            "a + b;",
            true,
        );
        assert_eq!(
            one(Dialect::DuckDb, &form),
            "CREATE OR REPLACE MACRO s.f(a, b) AS a + b"
        );
    }

    #[test]
    fn function_errors() {
        let form = func(vec![], "int", "RETURN 1", false);
        assert_eq!(
            run(Dialect::Sqlite, &form).unwrap_err(),
            "SQLite has no stored functions."
        );
        assert_eq!(run(Dialect::ClickHouse, &form).unwrap_err(), UNSUPPORTED);
        let no_return = func(vec![], " ", "RETURN 1", false);
        assert_eq!(
            run(Dialect::Postgres, &no_return).unwrap_err(),
            "A function needs a return type."
        );
        let no_body = func(vec![], "int", " ; ", false);
        assert_eq!(
            run(Dialect::Postgres, &no_body).unwrap_err(),
            "A function needs a body."
        );
        let no_type = func(vec![param("a", " ", None)], "int", "RETURN 1", false);
        assert_eq!(
            run(Dialect::Postgres, &no_type).unwrap_err(),
            "Every parameter needs a type."
        );
        let no_name = func(vec![param("", "int", None)], "int", "RETURN 1", false);
        assert_eq!(
            run(Dialect::Postgres, &no_name).unwrap_err(),
            "Every parameter needs a name."
        );
        let bad_mode = func(
            vec![param("a", "int", Some("SIDEWAYS"))],
            "int",
            "RETURN 1",
            false,
        );
        assert!(
            run(Dialect::Postgres, &bad_mode)
                .unwrap_err()
                .contains("SIDEWAYS")
        );
    }

    #[test]
    fn postgres_trigger_makes_function_and_trigger() {
        let form = trig(
            Timing::Before,
            &[Event::Update, Event::Insert],
            Some("NEW.x > 0"),
            true,
        );
        let sql = run(Dialect::Postgres, &form).unwrap();
        assert_eq!(sql.len(), 2);
        assert_eq!(
            sql[0],
            "CREATE OR REPLACE FUNCTION s.t_fn() RETURNS trigger LANGUAGE plpgsql AS $body$\nBEGIN\nUPDATE log SET n = n + 1;\nRETURN NEW;\nEND\n$body$"
        );
        assert_eq!(
            sql[1],
            "CREATE OR REPLACE TRIGGER t BEFORE INSERT OR UPDATE ON s.tbl FOR EACH ROW WHEN (NEW.x > 0) EXECUTE FUNCTION s.t_fn()"
        );
    }

    #[test]
    fn postgres_trigger_return_rules() {
        let delete = run(
            Dialect::Postgres,
            &trig(Timing::After, &[Event::Delete], None, false),
        )
        .unwrap();
        assert!(delete[0].contains("RETURN OLD;"));
        assert!(
            delete[1].starts_with("CREATE TRIGGER t AFTER DELETE ON s.tbl FOR EACH ROW EXECUTE")
        );
        let both = run(
            Dialect::Postgres,
            &trig(Timing::After, &[Event::Delete, Event::Insert], None, false),
        )
        .unwrap();
        assert!(both[0].contains("RETURN NEW;"));
        let ObjectForm::Trigger {
            name,
            table,
            timing,
            events,
            for_each_row,
            when,
            replace,
            ..
        } = trig(Timing::Before, &[Event::Insert], None, false)
        else {
            unreachable!()
        };
        let own_return = ObjectForm::Trigger {
            name,
            table,
            timing,
            events,
            for_each_row,
            when,
            body: "IF NEW.x IS NULL THEN RETURN NULL; END IF;".into(),
            replace,
        };
        let sql = run(Dialect::Postgres, &own_return).unwrap();
        // An early RETURN still needs a final one.
        assert!(sql[0].contains("END IF;\nRETURN NEW;"), "{}", sql[0]);
        // Without Replace the function is not overwritten either.
        assert!(sql[0].starts_with("CREATE FUNCTION"), "{}", sql[0]);
        let ends_in_return = ObjectForm::Trigger {
            name: "t".into(),
            table: "tbl".into(),
            timing: Timing::Before,
            events: vec![Event::Insert],
            for_each_row: true,
            when: None,
            body: "NEW.x := 1; RETURN NULL;".into(),
            replace: false,
        };
        let sql = run(Dialect::Postgres, &ends_in_return).unwrap();
        assert!(!sql[0].contains("RETURN NEW"), "{}", sql[0]);
        let statement = ObjectForm::Trigger {
            name: "t".into(),
            table: "tbl".into(),
            timing: Timing::After,
            events: vec![Event::Insert],
            for_each_row: false,
            when: None,
            body: "SELECT 1".into(),
            replace: false,
        };
        let sql = run(Dialect::Postgres, &statement).unwrap();
        assert!(sql[0].contains("RETURN NULL;"));
        assert!(sql[1].contains("FOR EACH STATEMENT"));
    }

    #[test]
    fn mysql_trigger() {
        let form = trig(Timing::After, &[Event::Insert], None, true);
        assert_eq!(
            run(Dialect::MySql, &form).unwrap(),
            [
                "DROP TRIGGER IF EXISTS s.t",
                "CREATE TRIGGER s.t AFTER INSERT ON s.tbl FOR EACH ROW\nBEGIN\nUPDATE log SET n = n + 1;\nEND"
            ]
        );
        assert_eq!(
            run(
                Dialect::MySql,
                &trig(Timing::After, &[Event::Insert, Event::Update], None, false)
            )
            .unwrap_err(),
            "MySQL triggers fire on one event."
        );
        assert!(
            run(
                Dialect::MySql,
                &trig(Timing::InsteadOf, &[Event::Insert], None, false)
            )
            .is_err()
        );
        assert!(
            run(
                Dialect::MySql,
                &trig(Timing::After, &[Event::Insert], Some("1"), false)
            )
            .is_err()
        );
    }

    #[test]
    fn sqlite_trigger() {
        let form = trig(Timing::Before, &[Event::Update], Some("NEW.x > 0"), true);
        assert_eq!(
            run(Dialect::Sqlite, &form).unwrap(),
            [
                "DROP TRIGGER IF EXISTS s.t",
                "CREATE TRIGGER s.t BEFORE UPDATE ON tbl FOR EACH ROW WHEN NEW.x > 0\nBEGIN\nUPDATE log SET n = n + 1;\nEND"
            ]
        );
        assert_eq!(
            run(Dialect::Sqlite, &trig(Timing::After, &[], None, false)).unwrap_err(),
            "A trigger needs at least one event."
        );
        assert_eq!(
            run(
                Dialect::Sqlite,
                &trig(Timing::After, &[Event::Insert, Event::Delete], None, false)
            )
            .unwrap_err(),
            "SQLite triggers fire on one event."
        );
    }

    #[test]
    fn sqlserver_trigger() {
        let form = trig(
            Timing::InsteadOf,
            &[Event::Insert, Event::Update],
            None,
            true,
        );
        assert_eq!(
            one(Dialect::SqlServer, &form),
            "CREATE OR ALTER TRIGGER s.t ON s.tbl INSTEAD OF INSERT, UPDATE AS\nBEGIN\nUPDATE log SET n = n + 1;\nEND"
        );
        assert_eq!(
            run(
                Dialect::SqlServer,
                &trig(Timing::Before, &[Event::Insert], None, false)
            )
            .unwrap_err(),
            "SQL Server triggers have no BEFORE timing; use INSTEAD OF."
        );
        assert!(
            run(
                Dialect::SqlServer,
                &trig(Timing::After, &[Event::Insert], Some("1"), false)
            )
            .is_err()
        );
    }

    #[test]
    fn oracle_trigger() {
        let form = trig(
            Timing::Before,
            &[Event::Insert, Event::Update],
            Some("NEW.x > 0"),
            true,
        );
        assert_eq!(
            one(Dialect::Oracle, &form),
            "CREATE OR REPLACE TRIGGER \"s\".\"t\" BEFORE INSERT OR UPDATE ON \"s\".\"tbl\"\nFOR EACH ROW\nWHEN (NEW.x > 0)\nBEGIN\nUPDATE log SET n = n + 1;\nEND;"
        );
        let statement = ObjectForm::Trigger {
            name: "t".into(),
            table: "tbl".into(),
            timing: Timing::After,
            events: vec![Event::Delete],
            for_each_row: false,
            when: Some("1 = 1".into()),
            body: "NULL;".into(),
            replace: false,
        };
        assert!(
            run(Dialect::Oracle, &statement)
                .unwrap_err()
                .contains("FOR EACH ROW")
        );
    }

    #[test]
    fn trigger_errors() {
        let form = trig(Timing::After, &[Event::Insert], None, false);
        assert_eq!(
            run(Dialect::DuckDb, &form).unwrap_err(),
            "DuckDB has no triggers."
        );
        assert_eq!(run(Dialect::ClickHouse, &form).unwrap_err(), UNSUPPORTED);
        let ObjectForm::Trigger {
            name,
            timing,
            events,
            for_each_row,
            when,
            replace,
            ..
        } = form
        else {
            unreachable!()
        };
        let no_table = ObjectForm::Trigger {
            name: name.clone(),
            table: " ".into(),
            timing,
            events: events.clone(),
            for_each_row,
            when: when.clone(),
            body: "SELECT 1".into(),
            replace,
        };
        assert_eq!(
            run(Dialect::Postgres, &no_table).unwrap_err(),
            "A trigger needs a table."
        );
        let no_body = ObjectForm::Trigger {
            name,
            table: "tbl".into(),
            timing,
            events,
            for_each_row,
            when,
            body: ";".into(),
            replace,
        };
        assert_eq!(
            run(Dialect::Postgres, &no_body).unwrap_err(),
            "A trigger needs a body."
        );
    }

    #[test]
    fn trigger_quotes_odd_names() {
        let form = ObjectForm::Trigger {
            name: "my trg".into(),
            table: "my table".into(),
            timing: Timing::After,
            events: vec![Event::Insert],
            for_each_row: true,
            when: None,
            body: "SELECT 1".into(),
            replace: false,
        };
        let sql = run(Dialect::Sqlite, &form).unwrap();
        assert!(
            sql[0].contains("s.\"my trg\" AFTER INSERT ON \"my table\""),
            "{}",
            sql[0]
        );
        let pg = run(Dialect::Postgres, &form).unwrap();
        assert!(pg[0].contains("s.\"my trg_fn\"()"), "{}", pg[0]);
    }

    #[test]
    fn form_round_trips_through_json() {
        let json = r#"{"kind":"trigger","name":"t","table":"x","timing":"instead_of","events":["insert"],"body":"b"}"#;
        let form: ObjectForm = serde_json::from_str(json).unwrap();
        assert!(matches!(
            form,
            ObjectForm::Trigger {
                timing: Timing::InsteadOf,
                ..
            }
        ));
    }

    #[test]
    fn word_helpers() {
        assert!(starts_with_word("  Begin x", "BEGIN"));
        assert!(!starts_with_word("beginning", "BEGIN"));
    }
}
