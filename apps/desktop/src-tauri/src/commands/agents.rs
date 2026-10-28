//! Setting external agents up: the commands that register the app, started with `--mcp`, as
//! an MCP server with Claude Code and Codex.

use std::collections::HashMap;
use std::path::PathBuf;

use platypus_mcp::AgentBlocker;
use platypus_store::toml;
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use super::{AppState, lock};
use crate::MCP_FLAG;
use crate::error::{CommandError, CommandResult};

/// The environment variable that moves the profile, passed on to the server when the app
/// uses one.
const DATA_DIR_ENV: &str = "PLATYPUS_DATA_DIR";

/// The MCP server earlier versions installed beside the app, before `--mcp`.
const OLD_SERVER: &str = "platypus-mcp";

#[derive(Debug, Serialize)]
pub struct AgentSetup {
    /// The app's executable, which agents start with `--mcp`.
    mcp_path: String,
    /// `claude mcp add …`, for Claude Code.
    claude_command: String,
    /// The `[mcp_servers.platypus]` table of Codex's `config.toml`.
    codex_config: String,
}

/// `claude mcp add platypus -- "<path>" --mcp`, with the profile's folder when the app uses
/// one.
fn claude_command(path: &str, data_dir: Option<&str>) -> String {
    let env = data_dir
        .map(|dir| format!("-e \"{DATA_DIR_ENV}={dir}\" "))
        .unwrap_or_default();
    format!("claude mcp add platypus {env}-- \"{path}\" {MCP_FLAG}")
}

/// The Codex table for the server, as TOML.
fn codex_config(path: &str, data_dir: Option<&str>) -> String {
    let mut server = toml::Table::new();
    server.insert("command".to_owned(), path.into());
    server.insert("args".to_owned(), vec![MCP_FLAG].into());
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

fn setup(path: &str, data_dir: Option<&str>) -> AgentSetup {
    AgentSetup {
        claude_command: claude_command(path, data_dir),
        codex_config: codex_config(path, data_dir),
        mcp_path: path.to_owned(),
    }
}

/// The executable agents start. An `AppImage` runs from a folder mounted anew at each start:
/// the `AppImage` file itself stays put.
fn app_path() -> CommandResult<PathBuf> {
    if let Some(image) = std::env::var_os("APPIMAGE") {
        return Ok(image.into());
    }
    std::env::current_exe().map_err(|e| CommandError::new("other", e.to_string()))
}

#[tauri::command]
pub fn agent_setup() -> CommandResult<AgentSetup> {
    let data_dir = std::env::var(DATA_DIR_ENV).ok();
    Ok(setup(
        &app_path()?.display().to_string(),
        data_dir.as_deref(),
    ))
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
                let command = server.get("command")?.as_str()?;
                is_old_server(command).then(|| StaleRegistration {
                    agent: "Claude Code",
                    name: name.clone(),
                    command: command.to_owned(),
                    project: project.map(str::to_owned),
                })
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
        .filter_map(|(name, server)| {
            let command = server.get("command")?.as_str()?;
            is_old_server(command).then(|| StaleRegistration {
                agent: "Codex",
                name: name.clone(),
                command: command.to_owned(),
                project: None,
            })
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_for_claude_code_and_codex() {
        let path = r"C:\Program Files\PlatypusDB\platypus-db.exe";
        assert_eq!(
            claude_command(path, None),
            r#"claude mcp add platypus -- "C:\Program Files\PlatypusDB\platypus-db.exe" --mcp"#
        );
        assert_eq!(
            claude_command("/opt/p/platypus-db", Some("/tmp/profile")),
            r#"claude mcp add platypus -e "PLATYPUS_DATA_DIR=/tmp/profile" -- "/opt/p/platypus-db" --mcp"#
        );
        let setup = setup(path, Some(r"D:\profile"));
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
        assert!(claude_stale(&serde_json::json!({})).is_empty());
    }
}
