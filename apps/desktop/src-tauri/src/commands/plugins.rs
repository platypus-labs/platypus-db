//! WebAssembly plugins (Settings › Plugins): inspecting a package before the user approves
//! its permissions, installing, turning on and off, removing, and running their actions.
//! The sandbox and the packages are in `platypus-plugins`; their drivers open through
//! [`super::connect`].
//!
//! The hooks of plugins are called from the commands that run statements, after the app's own
//! checks. Each command first asks `HOOKS.wanted(..)`, one atomic load: everything else here
//! runs only when some plugin hooks that event. A hook gets a masked copy of the statement and
//! answers allow or block; the statement that runs is the command's own.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use platypus_core::DriverKind;
use platypus_mcp::masking_seal::{MaskingRules, MaskingSeal};
use platypus_plugins::{
    ActionOutcome, FRAME_SCHEME, FrameFile, FrameRequest, GridColumn, GridRendered, GridValue,
    HOOKS, HookConnection, HookContext, HookEvent, HookFailure, HookReport, HookStatement,
    Inspected, Installed, PluginError, Plugins, QueryStatus, SettingState, frame_csp, frame_target,
};
use platypus_sql::Dialect;
use platypus_sql::security::mask_secrets;
use platypus_sql::sensitive::{NameRules, mask_literals, mask_strings};
use platypus_store::{ConnectionType, HistoryStatus};
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use super::{AppState, Session};
use crate::applog;
use crate::error::{CommandError, CommandResult};

/// Tells the page that a hook failed and the action went ahead: a [`HookFailure`].
const HOOK_FAILED: &str = "plugin-hook-failed";

fn error(e: &PluginError) -> CommandError {
    let kind = match e {
        PluginError::Denied(_) => "plugin_denied",
        PluginError::OutOfFuel | PluginError::Timeout | PluginError::Trap(_) => "plugin_stopped",
        _ => "plugin",
    };
    CommandError::new(kind, e.to_string())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a `map_err` callback"
)]
fn failed(e: PluginError) -> CommandError {
    error(&e)
}

fn plugins() -> CommandResult<Plugins> {
    Plugins::shared().map_err(failed)
}

/// The installed plugins. The page reads them at each start, so this also fills the table of
/// hooks: there is nothing else to read for them.
#[tauri::command]
pub fn plugins_list() -> CommandResult<Vec<Installed>> {
    let plugins = plugins()?;
    let list = plugins.list().map_err(failed)?;
    HOOKS.load(&plugins, &list);
    Ok(list)
}

/// Reads the plugins again for the table of hooks, after a change to them.
fn reload_hooks(plugins: &Plugins) -> CommandResult<()> {
    let list = plugins.list().map_err(failed)?;
    HOOKS.load(plugins, &list);
    Ok(())
}

/// Reads a package (a folder or a `.zip`) for the user to review its permissions.
#[tauri::command]
pub async fn plugin_inspect(path: PathBuf) -> CommandResult<Inspected> {
    plugins()?.inspect(&path).await.map_err(failed)
}

/// Installs a package the user reviewed; its permissions are approved by this call. `hash` is
/// the one `plugin_inspect` gave: a package changed since the review is refused.
#[tauri::command]
pub async fn plugin_install(path: PathBuf, hash: String) -> CommandResult<Installed> {
    let plugins = plugins()?;
    let installed = plugins.install(&path, &hash).await.map_err(failed)?;
    HOOKS.forgive(&installed.manifest.id);
    reload_hooks(&plugins)?;
    Ok(installed)
}

#[tauri::command]
pub fn plugin_set_enabled(id: String, enabled: bool) -> CommandResult<()> {
    let plugins = plugins()?;
    plugins.set_enabled(&id, enabled).map_err(failed)?;
    HOOKS.forgive(&id);
    reload_hooks(&plugins)
}

#[tauri::command]
pub fn plugin_remove(id: String) -> CommandResult<()> {
    let plugins = plugins()?;
    plugins.remove(&id).map_err(failed)?;
    HOOKS.forgive(&id);
    reload_hooks(&plugins)
}

/// What the fields of a plugin's settings page hold; a secret only says if it is set.
#[tauri::command]
pub fn plugin_settings(id: String) -> CommandResult<Vec<SettingState>> {
    plugins()?.settings_of(&id).map_err(failed)
}

/// Saves a value of a plugin's settings page; `null` goes back to the default.
#[tauri::command]
pub fn plugin_set_setting(id: String, key: String, value: Value) -> CommandResult<()> {
    plugins()?.set_setting(&id, &key, &value).map_err(failed)
}

/// Runs a plugin's action on what the page gives it (the editor's text, the grid's rows,
/// the navigator's object); the page applies the outcome.
#[tauri::command]
pub async fn plugin_run_action(
    plugin: String,
    action: String,
    context: Value,
) -> CommandResult<ActionOutcome> {
    plugins()?
        .run_action(&plugin, &action, context)
        .await
        .map_err(failed)
}

/// The root of the plugins' documents: wry serves a custom scheme this way on these systems.
fn frame_base() -> String {
    if cfg!(any(windows, target_os = "android")) {
        format!("http://{FRAME_SCHEME}.localhost")
    } else {
        format!("{FRAME_SCHEME}://localhost")
    }
}

/// Where a document of a plugin is, for its frame; refused without the `tabs` permission, or for
/// a document its manifest does not name.
#[tauri::command]
pub fn plugin_frame_url(plugin: String, document: String) -> CommandResult<String> {
    let path = plugins()?.frame_path(&plugin, &document).map_err(failed)?;
    Ok(format!("{}{path}", frame_base()))
}

/// What a plugin's frame asks, checked again here: the page may have been fooled, and this is
/// where a request becomes real. `request` is `{name, params}`.
#[tauri::command]
pub async fn plugin_frame_request(plugin: String, request: Value) -> CommandResult<Value> {
    let request = FrameRequest::parse(request).map_err(failed)?;
    plugins()?
        .frame_request(&plugin, request)
        .await
        .map_err(failed)
}

/// The values of a plugin's computed column for a batch of rows, as the grid shows them.
#[tauri::command]
pub async fn plugin_grid_column(
    plugin: String,
    extension: String,
    columns: Vec<GridColumn>,
    rows: Vec<Vec<Option<String>>>,
) -> CommandResult<Vec<GridValue>> {
    plugins()?
        .grid_column(&plugin, &extension, columns, rows)
        .await
        .map_err(failed)
}

/// How a plugin's renderer draws a batch of a column's values, as the grid shows them.
#[tauri::command]
pub async fn plugin_grid_render(
    plugin: String,
    extension: String,
    column: GridColumn,
    values: Vec<String>,
) -> CommandResult<Vec<GridRendered>> {
    plugins()?
        .grid_render(&plugin, &extension, column, values)
        .await
        .map_err(failed)
}

/// Runs a plugin's command on a cell, as the grid shows it; the page applies the outcome.
#[tauri::command]
pub async fn plugin_grid_action(
    plugin: String,
    extension: String,
    column: GridColumn,
    value: Option<String>,
) -> CommandResult<ActionOutcome> {
    plugins()?
        .grid_action(&plugin, &extension, column, value)
        .await
        .map_err(failed)
}

/// Answers the scheme of the plugins' documents, on a thread of its own. Only `GET` of a plugin's
/// own file; each answer carries the policy that sandboxes it, an error too.
pub fn frame_protocol(
    request: &tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let path = request.uri().path().to_owned();
    let get = request.method() == tauri::http::Method::GET;
    tauri::async_runtime::spawn_blocking(move || {
        responder.respond(frame_response(get, &path));
    });
}

fn frame_response(get: bool, path: &str) -> tauri::http::Response<Vec<u8>> {
    let target = frame_target(path);
    let file = match (get, target, plugins()) {
        (true, Some((id, file)), Ok(plugins)) => plugins.frame_file(id, file),
        _ => Err(PluginError::Denied("not a file of a plugin".to_owned())),
    };
    let csp = frame_csp(&frame_base(), target.map_or("none", |(id, _)| id));
    let builder = tauri::http::Response::builder()
        .header("Content-Security-Policy", csp)
        .header("X-Content-Type-Options", "nosniff")
        .header("X-DNS-Prefetch-Control", "off")
        .header("Referrer-Policy", "no-referrer")
        .header("Cache-Control", "no-store");
    let response = match file {
        Ok(FrameFile { bytes, mime }) => builder.header("Content-Type", mime).body(bytes),
        Err(e) => {
            let status = if matches!(e, PluginError::Denied(_)) {
                403
            } else {
                404
            };
            builder
                .status(status)
                .header("Content-Type", "text/plain; charset=utf-8")
                .body(e.to_string().into_bytes())
        }
    };
    response.unwrap_or_else(|_| {
        let mut empty = tauri::http::Response::new(Vec::new());
        *empty.status_mut() = tauri::http::StatusCode::INTERNAL_SERVER_ERROR;
        empty
    })
}

/// What the rules of agents say is sensitive on a connection.
struct SensitiveNames {
    rules: NameRules,
    /// The user's marks, in lower case.
    marked: HashSet<String>,
}

impl SensitiveNames {
    fn of(state: &AppState, connection: Uuid) -> CommandResult<Self> {
        let files = state.with_settings(|s| Ok(MaskingRules::of(Some(s))))?;
        let marks = state.with_catalog(|c| c.sensitive_columns(connection))?;
        let seal = state
            .agent_seal()
            .map_or(MaskingSeal::Files, |seal| seal.masking(&seal.read()));
        Ok(Self {
            rules: NameRules::new(&seal.rules(files).names),
            marked: seal
                .marks(connection, marks)
                .into_iter()
                .filter(|m| m.sensitive)
                .map(|m| m.key.column.to_lowercase())
                .collect(),
        })
    }

    fn is(&self, name: &str) -> bool {
        self.marked.contains(&name.to_lowercase()) || self.rules.matches(name)
    }
}

/// A statement as a hook may see it: without the passwords the query log hides, and with the
/// literals of sensitive columns masked as they are for agents. When the rules cannot be read
/// every string is masked. A driver that does not speak SQL has literals no one can tell, so
/// its statements give no text at all.
fn masked_statement(
    state: &AppState,
    connection: Uuid,
    driver: DriverKind,
    sql: &str,
) -> HookStatement {
    if matches!(
        driver,
        DriverKind::Mongo | DriverKind::Redis | DriverKind::Elasticsearch
    ) {
        return HookStatement::new(None);
    }
    let names = SensitiveNames::of(state, connection).ok();
    HookStatement::new(Some(masked_text(sql, driver, names.as_ref())))
}

/// `sql` without its passwords and the literals of `names`; every string without any `names`.
fn masked_text(sql: &str, driver: DriverKind, names: Option<&SensitiveNames>) -> String {
    let dialect = Dialect::from(driver);
    let sql = mask_secrets(sql);
    match names {
        Some(names) => mask_literals(&sql, dialect, &|column| names.is(column)),
        None => mask_strings(&sql, dialect),
    }
}

fn hook_connection(state: &AppState, session: &Session) -> HookConnection {
    let production = state
        .with_store(|s| {
            Ok(s.get(session.connection_id)
                .is_some_and(|c| c.connection_type == Some(ConnectionType::Prod)))
        })
        .unwrap_or(false);
    HookConnection {
        name: session.label.clone(),
        driver: serde_json::to_value(session.config.driver)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default(),
        database: session.config.database.clone(),
        production,
    }
}

/// Tells the page of the failures of a report, each once: the action they were for went ahead.
fn tell_failures(app: &AppHandle, failures: &[HookFailure]) {
    for failure in failures {
        let _ = app.emit(HOOK_FAILED, failure);
    }
}

/// The error of a hook that blocked, naming its plugin: its message is not the app's.
fn blocked(app: &AppHandle, report: &HookReport) -> CommandResult<()> {
    tell_failures(app, &report.failures);
    match &report.blocked {
        Some(block) => {
            applog::write(
                "INFO",
                &format!("the plugin {} blocked an action", block.plugin),
            );
            Err(CommandError::new(
                "plugin_blocked",
                format!(
                    "The plugin “{}” blocked this: {}",
                    block.plugin, block.message
                ),
            ))
        }
        None => Ok(()),
    }
}

/// Runs the hooks of an event and does not wait for them: for events that cannot block.
fn fire(app: AppHandle, context: HookContext) {
    tauri::async_runtime::spawn(async move {
        if let Ok(plugins) = Plugins::shared() {
            let report = HOOKS.run(&plugins, &context).await;
            tell_failures(&app, &report.failures);
        }
    });
}

/// What an `after_query` hook is told once the statement ends.
pub(crate) struct AfterQuery {
    app: AppHandle,
    connection: HookConnection,
    statement: HookStatement,
}

impl AfterQuery {
    pub(crate) fn fire(self, status: QueryStatus, elapsed: Duration, affected: Option<u64>) {
        let context = HookContext::AfterQuery {
            connection: self.connection,
            statement: self.statement,
            status,
            elapsed_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
            affected,
        };
        fire(self.app, context);
    }
}

/// A statement's end as a hook is told it: the history's status, which holds no value.
pub(crate) const fn query_status(status: HistoryStatus) -> QueryStatus {
    match status {
        HistoryStatus::Ok => QueryStatus::Ok,
        HistoryStatus::Error => QueryStatus::Error,
        HistoryStatus::Cancelled => QueryStatus::Cancelled,
    }
}

/// Whether a statement has any hook to go through: nothing to wait for and nothing allocated
/// when not, as it stands on the path of every query.
#[inline]
pub(crate) fn query_hooked() -> bool {
    HOOKS.wanted(HookEvent::BeforeQuery) || HOOKS.wanted(HookEvent::AfterQuery)
}

/// The `before_query` hooks of a statement that passed the app's checks; a hook that blocks is
/// the command's error. Answers what to tell the `after_query` hooks, when there are any.
pub(crate) async fn before_query(
    app: &AppHandle,
    state: &AppState,
    session_id: u64,
    sql: &str,
) -> CommandResult<Option<AfterQuery>> {
    // Without a data folder there are no plugins.
    let Ok(plugins) = Plugins::shared() else {
        return Ok(None);
    };
    let session = state.session(session_id)?;
    let connection = hook_connection(state, &session);
    let statement = masked_statement(state, session.connection_id, session.config.driver, sql);
    let context = HookContext::BeforeQuery {
        connection: connection.clone(),
        statement: statement.clone(),
    };
    blocked(app, &HOOKS.run(&plugins, &context).await)?;
    Ok(HOOKS.wanted(HookEvent::AfterQuery).then(|| AfterQuery {
        app: app.clone(),
        connection,
        statement,
    }))
}

/// The `on_connect` hooks of a session just opened; nothing waits for them.
pub(crate) fn on_connect(app: &AppHandle, state: &AppState, session_id: u64) {
    if let Ok(session) = state.session(session_id) {
        let context = HookContext::OnConnect {
            connection: hook_connection(state, &session),
        };
        fire(app.clone(), context);
    }
}

/// How much a save changes, as the hooks of `before_save` are told it.
pub(crate) struct SaveCounts {
    pub(crate) updates: usize,
    pub(crate) inserts: usize,
    pub(crate) deletes: usize,
}

/// The `before_save` hooks of edited rows about to be saved; a hook that blocks is the error.
pub(crate) async fn before_save(
    app: &AppHandle,
    state: &AppState,
    session_id: u64,
    (schema, table): (&str, &str),
    counts: SaveCounts,
) -> CommandResult<()> {
    let Ok(plugins) = Plugins::shared() else {
        return Ok(());
    };
    let session = state.session(session_id)?;
    let context = HookContext::BeforeSave {
        connection: hook_connection(state, &session),
        schema: (!schema.is_empty()).then(|| schema.to_owned()),
        table: table.to_owned(),
        updates: counts.updates,
        inserts: counts.inserts,
        deletes: counts.deletes,
    };
    blocked(app, &HOOKS.run(&plugins, &context).await)
}

#[cfg(test)]
mod tests {
    use platypus_sql::sensitive::DEFAULT_NAMES;

    use super::*;

    fn names() -> SensitiveNames {
        SensitiveNames {
            rules: NameRules::new(DEFAULT_NAMES),
            marked: HashSet::from(["nickname".to_owned()]),
        }
    }

    #[test]
    fn a_hook_reads_statements_as_agents_do() {
        let names = names();
        let masked = |sql: &str| masked_text(sql, DriverKind::Postgres, Some(&names));
        // A sensitive column by the rules, by a mark, and a column that is neither.
        assert_eq!(
            masked("SELECT 1 FROM t WHERE email = 'a@b.c' AND name = 'apple'"),
            "SELECT 1 FROM t WHERE email = '••••••' AND name = 'apple'"
        );
        assert_eq!(
            masked("UPDATE t SET NickName = 'x'"),
            "UPDATE t SET NickName = '••••••'"
        );
        assert_eq!(
            masked("INSERT INTO t (id, password) VALUES (1, 'p')"),
            "INSERT INTO t (id, password) VALUES (1, '••••••')"
        );
    }

    #[test]
    fn a_hook_never_reads_a_password() {
        let names = names();
        for sql in [
            "CREATE USER bob WITH PASSWORD 'hunter2'",
            "ALTER USER bob IDENTIFIED BY 'hunter2'",
        ] {
            let text = masked_text(sql, DriverKind::Postgres, Some(&names));
            assert!(!text.contains("hunter2"), "{text}");
        }
    }

    #[test]
    fn rules_that_cannot_be_read_mask_every_string() {
        let text = masked_text(
            "SELECT 1 FROM t WHERE name = 'apple'",
            DriverKind::Postgres,
            None,
        );
        assert!(!text.contains("apple"), "{text}");
    }

    #[test]
    fn the_end_of_a_statement_says_no_more_than_how_it_went() {
        assert_eq!(query_status(HistoryStatus::Ok), QueryStatus::Ok);
        assert_eq!(query_status(HistoryStatus::Error), QueryStatus::Error);
        assert_eq!(
            query_status(HistoryStatus::Cancelled),
            QueryStatus::Cancelled
        );
    }
}
