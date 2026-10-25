//! Setting external agents up: where `platypus-mcp` is, and the commands that register it
//! with Claude Code and Codex.

use std::path::Path;

use platypus_store::toml;
use serde::Serialize;

use crate::error::{CommandError, CommandResult};

/// The environment variable that moves the profile, passed on to the server when the app
/// uses one.
const DATA_DIR_ENV: &str = "PLATYPUS_DATA_DIR";

#[derive(Debug, Serialize)]
pub struct AgentSetup {
    /// `platypus-mcp` next to the app's executable.
    mcp_path: String,
    /// Whether it is there: a build from source has it only after `cargo build -p platypus-mcp`.
    mcp_found: bool,
    /// `claude mcp add …`, for Claude Code.
    claude_command: String,
    /// The `[mcp_servers.platypus]` table of Codex's `config.toml`.
    codex_config: String,
}

/// `claude mcp add platypus -- "<path>"`, with the profile's folder when the app uses one.
fn claude_command(path: &str, data_dir: Option<&str>) -> String {
    let env = data_dir
        .map(|dir| format!("-e \"{DATA_DIR_ENV}={dir}\" "))
        .unwrap_or_default();
    format!("claude mcp add platypus {env}-- \"{path}\"")
}

/// The Codex table for the server, as TOML.
fn codex_config(path: &str, data_dir: Option<&str>) -> String {
    let mut server = toml::Table::new();
    server.insert("command".to_owned(), path.into());
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

fn setup(mcp: &Path, data_dir: Option<&str>) -> AgentSetup {
    let path = mcp.display().to_string();
    AgentSetup {
        mcp_found: mcp.is_file(),
        claude_command: claude_command(&path, data_dir),
        codex_config: codex_config(&path, data_dir),
        mcp_path: path,
    }
}

#[tauri::command]
pub fn agent_setup() -> CommandResult<AgentSetup> {
    let exe = std::env::current_exe().map_err(|e| CommandError::new("other", e.to_string()))?;
    let mcp = exe.with_file_name(format!("platypus-mcp{}", std::env::consts::EXE_SUFFIX));
    let data_dir = std::env::var(DATA_DIR_ENV).ok();
    Ok(setup(&mcp, data_dir.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_for_claude_code_and_codex() {
        let path = r"C:\Program Files\PlatypusDB\platypus-mcp.exe";
        assert_eq!(
            claude_command(path, None),
            r#"claude mcp add platypus -- "C:\Program Files\PlatypusDB\platypus-mcp.exe""#
        );
        assert_eq!(
            claude_command("/opt/p/platypus-mcp", Some("/tmp/profile")),
            r#"claude mcp add platypus -e "PLATYPUS_DATA_DIR=/tmp/profile" -- "/opt/p/platypus-mcp""#
        );
        let codex = codex_config(path, Some(r"D:\profile"));
        let parsed: toml::Table = toml::from_str(&codex).unwrap();
        assert_eq!(
            parsed["mcp_servers"]["platypus"]["command"].as_str(),
            Some(path)
        );
        assert_eq!(
            parsed["mcp_servers"]["platypus"]["env"]["PLATYPUS_DATA_DIR"].as_str(),
            Some(r"D:\profile")
        );
        assert!(codex.starts_with("[mcp_servers.platypus]"), "{codex}");
        assert!(!setup(Path::new("/no/such/platypus-mcp"), None).mcp_found);
    }
}
