//! Setting external agents up: the commands that register the app, started with `--mcp`, as
//! an MCP server with Claude Code and Codex.

use std::path::PathBuf;

use platypus_store::toml;
use serde::Serialize;

use crate::MCP_FLAG;
use crate::error::{CommandError, CommandResult};

/// The environment variable that moves the profile, passed on to the server when the app
/// uses one.
const DATA_DIR_ENV: &str = "PLATYPUS_DATA_DIR";

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
}
