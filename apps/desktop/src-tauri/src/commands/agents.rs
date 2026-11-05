//! Setting external agents up: the commands that register the app, started with `--mcp`, as
//! an MCP server with Claude Code and Codex; and the log of the calls agents made to its
//! tools.

use std::collections::HashMap;
use std::path::PathBuf;

use platypus_core::DriverKind;
use platypus_mcp::AgentBlocker;
use platypus_mcp::profile::AGENT_CALLS_FILE;
use platypus_sql::proposed::{self, Edits};
use platypus_store::{
    AGENT_CALLS_LIMIT, AgentCall, AgentCallQuery, AgentCallStore, AgentProposal, AgentSession,
    agent_calls_csv, toml,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

mod clients;

use super::{AppState, lock};
use crate::MCP_FLAG;
use crate::error::{CommandError, CommandResult};

/// The environment variable that moves the profile, passed on to the server when the app
/// uses one.
const DATA_DIR_ENV: &str = "PLATYPUS_DATA_DIR";

/// The home folder the clients' files are looked up in, instead of the user's.
const CLIENTS_HOME_ENV: &str = "PLATYPUS_CLIENTS_HOME";

/// The MCP server earlier versions installed beside the app, before `--mcp`.
const OLD_SERVER: &str = "platypus-mcp";

#[derive(Debug, Serialize)]
pub struct AgentSetup {
    /// The program agents start: the app's executable, or `flatpak` for the Flatpak.
    mcp_path: String,
    /// `claude mcp add …`, for Claude Code.
    claude_command: String,
    /// The `[mcp_servers.platypus]` table of Codex's `config.toml`.
    codex_config: String,
}

/// How agents start the server: the program and its arguments, ending with `--mcp`.
#[derive(Debug, PartialEq, Eq)]
struct Launch {
    command: String,
    args: Vec<String>,
}

impl Launch {
    /// The executable at `path`, with `--mcp`.
    fn app(path: &str) -> Self {
        Self {
            command: path.to_owned(),
            args: vec![MCP_FLAG.to_owned()],
        }
    }

    /// The app of the Flatpak `id`: its executable is inside the sandbox, so agents on the
    /// host go through `flatpak run`.
    fn flatpak(id: &str) -> Self {
        Self {
            command: "flatpak".to_owned(),
            args: vec![
                "run".to_owned(),
                "--command=platypus-db".to_owned(),
                id.to_owned(),
                MCP_FLAG.to_owned(),
            ],
        }
    }
}

/// `claude mcp add -s user platypus -- "<path>" --mcp`, with the profile's folder when the app uses
/// one.
fn claude_command(launch: &Launch, data_dir: Option<&str>) -> String {
    let env = data_dir
        .map(|dir| format!("-e \"{DATA_DIR_ENV}={dir}\" "))
        .unwrap_or_default();
    format!(
        "claude mcp add -s user platypus {env}-- \"{}\" {}",
        launch.command,
        launch.args.join(" ")
    )
}

/// The Codex table for the server, as TOML.
fn codex_config(launch: &Launch, data_dir: Option<&str>) -> String {
    let mut server = toml::Table::new();
    server.insert("command".to_owned(), launch.command.clone().into());
    server.insert("args".to_owned(), launch.args.clone().into());
    if let Some(dir) = data_dir {
        let mut env = toml::Table::new();
        env.insert(DATA_DIR_ENV.to_owned(), dir.into());
        server.insert("env".to_owned(), env.into());
    }
    let mut servers = toml::Table::new();
    servers.insert("platypus".to_owned(), server.into());
    let mut root = toml::Table::new();
    root.insert("mcp_servers".to_owned(), servers.into());
    toml::to_string(&root).unwrap_or_default()
}

fn setup(launch: &Launch, data_dir: Option<&str>) -> AgentSetup {
    AgentSetup {
        claude_command: claude_command(launch, data_dir),
        codex_config: codex_config(launch, data_dir),
        mcp_path: launch.command.clone(),
    }
}

/// How agents start this app as a server. An `AppImage` runs from a folder mounted anew at
/// each start: the `AppImage` file itself stays put.
fn launch() -> CommandResult<Launch> {
    if let Some(id) = std::env::var_os("FLATPAK_ID") {
        return Ok(Launch::flatpak(&id.to_string_lossy()));
    }
    if let Some(image) = std::env::var_os("APPIMAGE") {
        return Ok(Launch::app(&image.to_string_lossy()));
    }
    std::env::current_exe()
        .map(|exe| Launch::app(&exe.display().to_string()))
        .map_err(|e| CommandError::new("other", e.to_string()))
}

#[tauri::command]
pub fn agent_setup() -> CommandResult<AgentSetup> {
    let data_dir = std::env::var(DATA_DIR_ENV).ok();
    Ok(setup(&launch()?, data_dir.as_deref()))
}

/// Why agents cannot open each connection that has a reason (see
/// [`platypus_mcp::agent_blocker`]), by connection id.
#[tauri::command]
pub fn agent_blockers(state: State<'_, AppState>) -> CommandResult<HashMap<Uuid, AgentBlocker>> {
    let vault = lock(&state.vault).is_some();
    state.with_store(|store| {
        Ok(store
            .list()
            .iter()
            .filter_map(|saved| {
                platypus_mcp::agent_blocker(&store.effective(saved), vault)
                    .map(|blocker| (saved.id, blocker))
            })
            .collect())
    })
}

/// An agent registration that still starts `platypus-mcp`, the server earlier versions
/// installed beside the app: the installer removes it, so the registration is stale.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct StaleRegistration {
    /// `Claude Code` or `Codex`.
    agent: &'static str,
    /// The server's name in the agent's configuration.
    name: String,
    /// The program it starts.
    command: String,
    /// The project folder of a Claude Code registration local to a project.
    project: Option<String>,
}

/// Whether `command` is an old `platypus-mcp` server, a Windows or a Unix path.
fn is_old_server(command: &str) -> bool {
    let file = command.rsplit(['/', '\\']).next().unwrap_or(command);
    let file = file.to_ascii_lowercase();
    file.strip_suffix(".exe").unwrap_or(&file) == OLD_SERVER
}

/// The registration `name` of `agent`, when the `command` it starts is the old server.
fn stale(
    agent: &'static str,
    name: &str,
    command: Option<&str>,
    project: Option<&str>,
) -> Option<StaleRegistration> {
    let command = command.filter(|c| is_old_server(c))?;
    Some(StaleRegistration {
        agent,
        name: name.to_owned(),
        command: command.to_owned(),
        project: project.map(str::to_owned),
    })
}

/// The registrations in Claude Code's `~/.claude.json`: user ones at the top, local ones
/// under each project.
fn claude_stale(config: &serde_json::Value) -> Vec<StaleRegistration> {
    let servers = |value: &serde_json::Value, project: Option<&str>| {
        value
            .get("mcpServers")
            .and_then(serde_json::Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(move |(name, server)| {
                let command = server.get("command")?.as_str();
                stale("Claude Code", name, command, project)
            })
            .collect::<Vec<_>>()
    };
    let mut stale = servers(config, None);
    if let Some(projects) = config
        .get("projects")
        .and_then(serde_json::Value::as_object)
    {
        for (path, project) in projects {
            stale.extend(servers(project, Some(path)));
        }
    }
    stale
}

/// The registrations in Codex's `~/.codex/config.toml`.
fn codex_stale(config: &toml::Table) -> Vec<StaleRegistration> {
    config
        .get("mcp_servers")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
        .filter_map(|(name, server)| stale("Codex", name, server.get("command")?.as_str(), None))
        .collect()
}

/// The agent registrations that still start the old `platypus-mcp`: they must be added
/// again with the command the Agents page shows.
#[tauri::command]
pub fn stale_agent_registrations() -> Vec<StaleRegistration> {
    let Some(home) = std::env::home_dir() else {
        return Vec::new();
    };
    let mut stale = std::fs::read_to_string(home.join(".claude.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .map(|config| claude_stale(&config))
        .unwrap_or_default();
    if let Some(config) = std::fs::read_to_string(home.join(".codex").join("config.toml"))
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
    {
        stale.extend(codex_stale(&config));
    }
    stale
}

/// The server entry the clients get: this app with `--mcp`, and the profile's folder when
/// the app uses one.
fn server_entry() -> CommandResult<clients::ServerEntry> {
    let Launch { command, args } = launch()?;
    Ok(clients::ServerEntry {
        command,
        args,
        env: std::env::var(DATA_DIR_ENV)
            .ok()
            .map(|dir| (DATA_DIR_ENV.to_owned(), dir))
            .into_iter()
            .collect(),
    })
}

/// The home folder and the OS's configuration folder, where the clients keep their files.
fn client_dirs() -> CommandResult<(PathBuf, PathBuf)> {
    // The end-to-end tests point the clients at a throwaway home folder: moving the real
    // one would move the web view's too.
    if let Some(home) = std::env::var_os(CLIENTS_HOME_ENV).map(PathBuf::from) {
        let config = if cfg!(windows) {
            home.join("AppData").join("Roaming")
        } else {
            home.join(".config")
        };
        return Ok((home, config));
    }
    let home = std::env::home_dir()
        .ok_or_else(|| CommandError::new("other", "cannot find the home folder".to_owned()))?;
    let config = if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map_or_else(|| home.join("AppData").join("Roaming"), PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from)
    };
    Ok((home, config))
}

/// The MCP clients PlatypusDB can be installed in, with their file and whether it is there.
#[tauri::command]
pub fn agent_clients() -> CommandResult<Vec<clients::AgentClient>> {
    let (home, config) = client_dirs()?;
    Ok(clients::list(&home, &config, &server_entry()?))
}

/// Writes the server in client `id`'s file, after a backup of it.
#[tauri::command]
pub async fn install_agent_client(id: String) -> CommandResult<clients::Installed> {
    let (home, config) = client_dirs()?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let entry = server_entry()?;
    tauri::async_runtime::spawn_blocking(move || {
        clients::install(&id, &home, &config, &entry, &stamp)
    })
    .await
    .map_err(|e| CommandError::new("other", e.to_string()))?
}

/// Calls a page of the Activity view shows at most.
const CALL_PAGE: usize = 500;

/// The agents' call log the server writes, in the data folder (`PLATYPUS_DATA_DIR` when
/// set): opened on first use, when its oldest calls beyond [`AGENT_CALLS_LIMIT`] are
/// removed, then kept open.
static AGENT_CALLS: std::sync::Mutex<Option<AgentCallStore>> = std::sync::Mutex::new(None);

/// Runs `f` on the call log, off the main thread: the server may hold the file for a moment.
async fn with_agent_calls<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&AgentCallStore) -> Result<T, platypus_store::StoreError> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut store = lock(&AGENT_CALLS);
        if store.is_none() {
            let dir = match std::env::var_os(DATA_DIR_ENV)
                .map(PathBuf::from)
                .or_else(platypus_mcp::profile::portable_dir)
            {
                Some(dir) => dir,
                None => app
                    .path()
                    .app_data_dir()
                    .map_err(|e| CommandError::new("other", e.to_string()))?,
            };
            let opened = AgentCallStore::open(&dir.join(AGENT_CALLS_FILE))?;
            opened.prune(AGENT_CALLS_LIMIT)?;
            *store = Some(opened);
        }
        let store = store
            .as_ref()
            .unwrap_or_else(|| unreachable!("opened above"));
        f(store).map_err(Into::into)
    })
    .await
    .map_err(|e| CommandError::new("other", e.to_string()))?
}

/// The calls matching `query`, newest first, a page at a time.
#[tauri::command]
pub async fn agent_calls(
    app: AppHandle,
    mut query: AgentCallQuery,
) -> CommandResult<Vec<AgentCall>> {
    query.limit = query.limit.clamp(1, CALL_PAGE);
    with_agent_calls(app, move |s| s.search(&query)).await
}

/// The latest runs of the server, newest first.
#[tauri::command]
pub async fn agent_sessions(app: AppHandle, limit: usize) -> CommandResult<Vec<AgentSession>> {
    with_agent_calls(app, move |s| s.sessions(limit.clamp(1, CALL_PAGE))).await
}

/// Every call matching `query` (its limit ignored), as CSV.
#[tauri::command]
pub async fn agent_calls_export(
    app: AppHandle,
    mut query: AgentCallQuery,
) -> CommandResult<String> {
    query.limit = AGENT_CALLS_LIMIT;
    query.before_id = None;
    with_agent_calls(app, move |s| s.search(&query).map(|c| agent_calls_csv(&c))).await
}

/// The SQL agents proposed with `propose_sql` that no window opened yet; each is handed out
/// once.
#[tauri::command]
pub async fn take_agent_proposals(app: AppHandle) -> CommandResult<Vec<AgentProposal>> {
    with_agent_calls(app, AgentCallStore::take_proposals).await
}

/// A proposal of rows to change, checked and ready for the data viewer.
#[derive(Debug, Serialize)]
pub struct ProposedViewer {
    /// The viewer's statement, written here from the table's catalog (see
    /// [`platypus_sql::proposed::viewer`]); no text of the proposal is part of it as SQL.
    sql: String,
    /// The table, qualified and quoted, as the viewer names it.
    table: String,
    edits: Edits,
}

/// Parses and checks the rows an agent proposed (`edits`, JSON from the call log, which any
/// program can write) against the real table of the session, and writes the statement that
/// shows the rows to change. Nothing runs and nothing is saved here.
#[tauri::command]
pub async fn proposed_viewer(
    state: State<'_, AppState>,
    session_id: u64,
    edits: String,
) -> CommandResult<ProposedViewer> {
    let invalid = |m: String| CommandError::new("invalid_value", m);
    if edits.len() > proposed::MAX_BYTES {
        return Err(invalid("the proposal is too big".to_owned()));
    }
    let edits: Edits = serde_json::from_str(&edits).map_err(|e| {
        invalid(format!(
            "the proposal is not a list of rows to change ({e})"
        ))
    })?;
    if edits.rows() > proposed::MAX_ROWS {
        return Err(invalid(format!(
            "the proposal has {} rows, more than the {} allowed",
            edits.rows(),
            proposed::MAX_ROWS
        )));
    }
    let driver = state.session(session_id)?.config.driver;
    if matches!(
        driver,
        DriverKind::Mongo | DriverKind::Redis | DriverKind::Elasticsearch | DriverKind::Cassandra
    ) {
        return Err(invalid(format!(
            "rows of {} are not edited by key from the data viewer",
            driver.name()
        )));
    }
    let dialect = state.dialect(session_id)?;
    let conn = state.live(session_id).await?;
    let details = conn.table_details(&edits.schema, &edits.table).await?;
    let viewer = proposed::viewer(dialect, &edits, &details).map_err(invalid)?;
    Ok(ProposedViewer {
        sql: viewer.sql,
        table: viewer.table,
        edits,
    })
}

/// Empties the call log.
#[tauri::command]
pub async fn clear_agent_calls(app: AppHandle) -> CommandResult<()> {
    with_agent_calls(app, AgentCallStore::clear).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_for_claude_code_and_codex() {
        let path = r"C:\Program Files\PlatypusDB\platypus-db.exe";
        assert_eq!(
            claude_command(&Launch::app(path), None),
            r#"claude mcp add -s user platypus -- "C:\Program Files\PlatypusDB\platypus-db.exe" --mcp"#
        );
        assert_eq!(
            claude_command(&Launch::app("/opt/p/platypus-db"), Some("/tmp/profile")),
            r#"claude mcp add -s user platypus -e "PLATYPUS_DATA_DIR=/tmp/profile" -- "/opt/p/platypus-db" --mcp"#
        );
        assert_eq!(
            claude_command(&Launch::flatpak("dev.platypusdb.desktop"), None),
            r#"claude mcp add -s user platypus -- "flatpak" run --command=platypus-db dev.platypusdb.desktop --mcp"#
        );
        let setup = setup(&Launch::app(path), Some(r"D:\profile"));
        let parsed: toml::Table = toml::from_str(&setup.codex_config).unwrap();
        let server = &parsed["mcp_servers"]["platypus"];
        assert_eq!(server["command"].as_str(), Some(path));
        assert_eq!(
            server["args"].as_array().unwrap()[0].as_str(),
            Some("--mcp")
        );
        assert_eq!(
            server["env"]["PLATYPUS_DATA_DIR"].as_str(),
            Some(r"D:\profile")
        );
        assert!(
            setup.codex_config.starts_with("[mcp_servers.platypus]"),
            "{}",
            setup.codex_config
        );
    }

    #[test]
    fn finds_registrations_of_the_old_server() {
        let claude = serde_json::json!({
            "mcpServers": {
                "platypus": { "command": r"C:\Users\a\AppData\Local\PlatypusDB\platypus-mcp.exe" },
                "other": { "command": "npx" },
            },
            "projects": {
                r"D:\work": { "mcpServers": {
                    "db": { "command": "/opt/platypus/platypus-mcp" },
                    "new": { "command": "platypus-db", "args": ["--mcp"] },
                }},
            },
        });
        let stale = claude_stale(&claude);
        assert_eq!(stale.len(), 2, "{stale:?}");
        assert_eq!(stale[0].name, "platypus");
        assert_eq!(stale[0].project, None);
        assert_eq!(stale[1].project.as_deref(), Some(r"D:\work"));
        let codex: toml::Table = toml::from_str(
            "[mcp_servers.platypus]\ncommand = 'C:/PlatypusDB/platypus-mcp.exe'\n\
             [mcp_servers.ok]\ncommand = 'C:/PlatypusDB/platypus-db.exe'\nargs = ['--mcp']\n",
        )
        .unwrap();
        let stale = codex_stale(&codex);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].agent, "Codex");
        assert_eq!(claude_stale(&serde_json::json!({})), []);
    }
}
