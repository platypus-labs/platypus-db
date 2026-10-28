//! The MCP clients the Agents page installs PlatypusDB in: where each keeps its servers,
//! whether PlatypusDB is there already, and the entry to write, with a backup of the file.

use std::path::{Path, PathBuf};

use platypus_store::toml;
use serde::Serialize;
use serde_json::{Value, json};

use crate::error::{CommandError, CommandResult};

/// The server's name in the clients' configuration.
pub const SERVER_NAME: &str = "platypus";

/// How a client's file keeps its servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFormat {
    /// `{"mcpServers": {"<name>": {command, args, env}}}`.
    Json,
    /// Codex's `[mcp_servers.<name>]` tables.
    Toml,
}

/// A client PlatypusDB knows how to install in.
#[derive(Debug, Clone, Copy)]
pub struct ClientKind {
    pub id: &'static str,
    pub name: &'static str,
    format: ConfigFormat,
    /// The file, from the home folder and the OS's configuration folder.
    path: fn(home: &Path, config: &Path) -> PathBuf,
}

pub const CLIENTS: &[ClientKind] = &[
    ClientKind {
        id: "claude_desktop",
        name: "Claude Desktop",
        format: ConfigFormat::Json,
        path: |home, config| {
            if cfg!(target_os = "macos") {
                home.join("Library/Application Support/Claude/claude_desktop_config.json")
            } else {
                config.join("Claude").join("claude_desktop_config.json")
            }
        },
    },
    ClientKind {
        id: "claude_code",
        name: "Claude Code",
        format: ConfigFormat::Json,
        path: |home, _| home.join(".claude.json"),
    },
    ClientKind {
        id: "codex",
        name: "Codex",
        format: ConfigFormat::Toml,
        path: |home, _| home.join(".codex").join("config.toml"),
    },
    ClientKind {
        id: "cursor",
        name: "Cursor",
        format: ConfigFormat::Json,
        path: |home, _| home.join(".cursor").join("mcp.json"),
    },
    ClientKind {
        id: "windsurf",
        name: "Windsurf",
        format: ConfigFormat::Json,
        path: |home, _| {
            home.join(".codeium")
                .join("windsurf")
                .join("mcp_config.json")
        },
    },
    ClientKind {
        id: "antigravity",
        name: "Antigravity",
        format: ConfigFormat::Json,
        path: |home, _| {
            home.join(".gemini")
                .join("antigravity")
                .join("mcp_config.json")
        },
    },
    ClientKind {
        id: "gemini_cli",
        name: "Gemini CLI",
        format: ConfigFormat::Json,
        path: |home, _| home.join(".gemini").join("settings.json"),
    },
];

/// The program a client starts, with its arguments and environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerEntry {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl ServerEntry {
    fn json(&self) -> Value {
        let mut server = json!({ "command": self.command, "args": self.args });
        if !self.env.is_empty() {
            let env: serde_json::Map<String, Value> = self
                .env
                .iter()
                .map(|(k, v)| (k.clone(), json!(v)))
                .collect();
            server["env"] = Value::Object(env);
        }
        server
    }

    fn toml(&self) -> toml_edit::Table {
        let mut server = toml_edit::Table::new();
        server["command"] = toml_edit::value(&self.command);
        let mut args = toml_edit::Array::new();
        for arg in &self.args {
            args.push(arg.as_str());
        }
        server["args"] = toml_edit::value(args);
        if !self.env.is_empty() {
            let mut env = toml_edit::InlineTable::new();
            for (k, v) in &self.env {
                env.insert(k, v.as_str().into());
            }
            server["env"] = toml_edit::value(env);
        }
        server
    }

    /// Whether `server`, as a client's file has it, starts this program the same way.
    fn matches(&self, server: &Value) -> bool {
        let same_args =
            server
                .get("args")
                .and_then(Value::as_array)
                .map_or(self.args.is_empty(), |args| {
                    args.iter()
                        .map(Value::as_str)
                        .eq(self.args.iter().map(|a| Some(a.as_str())))
                });
        let same_env = self.env.iter().all(|(k, v)| {
            server
                .get("env")
                .and_then(|e| e.get(k))
                .and_then(Value::as_str)
                == Some(v)
        });
        server.get("command").and_then(Value::as_str) == Some(self.command.as_str())
            && same_args
            && same_env
    }
}

/// Whether a client has PlatypusDB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallState {
    /// The file has no `platypus` server.
    Missing,
    /// It has one that starts this app as the Agents page would.
    Installed,
    /// It has one that starts something else: an older path, another profile.
    Different,
    /// The file is there but does not read as its format: it is left alone.
    Unreadable,
}

/// A client as the Agents page lists it.
#[derive(Debug, Clone, Serialize)]
pub struct AgentClient {
    pub id: &'static str,
    pub name: &'static str,
    pub path: String,
    pub format: ConfigFormat,
    /// The file exists (else Install creates it).
    pub exists: bool,
    pub state: InstallState,
    /// What to add by hand: the entry, in the file's format.
    pub snippet: String,
}

/// The entry `kind`'s file would hold, as text to paste.
fn snippet(format: ConfigFormat, entry: &ServerEntry) -> String {
    match format {
        ConfigFormat::Json => {
            let doc = json!({ "mcpServers": { SERVER_NAME: entry.json() } });
            serde_json::to_string_pretty(&doc).unwrap_or_default()
        }
        ConfigFormat::Toml => {
            let mut doc = toml_edit::DocumentMut::new();
            let mut servers = toml_edit::Table::new();
            servers.set_implicit(true);
            servers.insert(SERVER_NAME, toml_edit::Item::Table(entry.toml()));
            doc.insert("mcp_servers", toml_edit::Item::Table(servers));
            doc.to_string()
        }
    }
}

/// The `platypus` server of a file's `text`, as JSON; `Err` when the text does not parse.
fn installed_server(format: ConfigFormat, text: &str) -> Result<Option<Value>, String> {
    match format {
        ConfigFormat::Json => {
            let doc = parse_json(text)?;
            Ok(doc
                .get("mcpServers")
                .and_then(|s| s.get(SERVER_NAME))
                .cloned())
        }
        ConfigFormat::Toml => {
            let doc: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
            let server = doc
                .get("mcp_servers")
                .and_then(|s| s.get(SERVER_NAME))
                .cloned();
            Ok(server.and_then(|s| serde_json::to_value(s).ok()))
        }
    }
}

/// A JSON file's document; an empty file reads as `{}`.
fn parse_json(text: &str) -> Result<Value, String> {
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    let doc: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if doc.is_object() {
        Ok(doc)
    } else {
        Err("the file is not a JSON object".to_owned())
    }
}

fn state_of(format: ConfigFormat, text: Option<&str>, entry: &ServerEntry) -> InstallState {
    let Some(text) = text else {
        return InstallState::Missing;
    };
    match installed_server(format, text) {
        Err(_) => InstallState::Unreadable,
        Ok(None) => InstallState::Missing,
        Ok(Some(server)) if entry.matches(&server) => InstallState::Installed,
        Ok(Some(_)) => InstallState::Different,
    }
}

/// Every known client, with its file under `home` and `config`.
pub fn list(home: &Path, config: &Path, entry: &ServerEntry) -> Vec<AgentClient> {
    CLIENTS
        .iter()
        .map(|kind| {
            let path = (kind.path)(home, config);
            let text = std::fs::read_to_string(&path).ok();
            AgentClient {
                id: kind.id,
                name: kind.name,
                path: path.display().to_string(),
                format: kind.format,
                exists: path.exists(),
                state: state_of(kind.format, text.as_deref(), entry),
                snippet: snippet(kind.format, entry),
            }
        })
        .collect()
}

/// `text` with the `platypus` server set to `entry`, everything else as it was.
pub fn with_entry(format: ConfigFormat, text: &str, entry: &ServerEntry) -> Result<String, String> {
    match format {
        ConfigFormat::Json => {
            let mut doc = parse_json(text)?;
            let servers = doc
                .as_object_mut()
                .map(|o| o.entry("mcpServers").or_insert_with(|| json!({})));
            match servers.and_then(Value::as_object_mut) {
                Some(servers) => {
                    servers.insert(SERVER_NAME.to_owned(), entry.json());
                }
                None => return Err("its `mcpServers` is not an object".to_owned()),
            }
            let mut out = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
            out.push('\n');
            Ok(out)
        }
        ConfigFormat::Toml => {
            let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| format!("{e}"))?;
            let servers = doc
                .entry("mcp_servers")
                .or_insert_with(|| {
                    let mut t = toml_edit::Table::new();
                    t.set_implicit(true);
                    toml_edit::Item::Table(t)
                })
                .as_table_mut()
                .ok_or_else(|| "its `mcp_servers` is not a table".to_owned())?;
            servers.insert(SERVER_NAME, toml_edit::Item::Table(entry.toml()));
            Ok(doc.to_string())
        }
    }
}

/// What an install did.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Installed {
    pub path: String,
    /// The copy of the file as it was, when there was one.
    pub backup: Option<String>,
}

/// The backup's path: the file's name with `.platypus-backup-<stamp>` after it.
fn backup_path(path: &Path, stamp: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".platypus-backup-{stamp}"));
    path.with_file_name(name)
}

fn io_error(path: &Path, e: &std::io::Error) -> CommandError {
    CommandError::new("io", format!("cannot write {}: {e}", path.display()))
}

/// Writes `entry` in client `id`'s file, after a copy of the file as it was (named with
/// `stamp`). A file that does not read as its format is left alone.
pub fn install(
    id: &str,
    home: &Path,
    config: &Path,
    entry: &ServerEntry,
    stamp: &str,
) -> CommandResult<Installed> {
    let kind = CLIENTS
        .iter()
        .find(|k| k.id == id)
        .ok_or_else(|| CommandError::new("invalid", format!("unknown agent client `{id}`")))?;
    let path = (kind.path)(home, config);
    let old = match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(io_error(&path, &e)),
    };
    let new = with_entry(kind.format, old.as_deref().unwrap_or_default(), entry).map_err(|e| {
        CommandError::new(
            "invalid",
            format!(
                "{} cannot be read, so it was left as it is: {e}",
                path.display()
            ),
        )
    })?;
    let backup = match &old {
        Some(_) => {
            let backup = backup_path(&path, stamp);
            std::fs::copy(&path, &backup).map_err(|e| io_error(&backup, &e))?;
            Some(backup.display().to_string())
        }
        None => {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| io_error(dir, &e))?;
            }
            None
        }
    };
    // Written beside the file, then moved over it: a crash leaves the old file or the new.
    let temp = backup_path(&path, "tmp");
    std::fs::write(&temp, new).map_err(|e| io_error(&temp, &e))?;
    std::fs::rename(&temp, &path).map_err(|e| io_error(&path, &e))?;
    Ok(Installed {
        path: path.display().to_string(),
        backup,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> ServerEntry {
        ServerEntry {
            command: r"C:\Apps\platypus-db.exe".to_owned(),
            args: vec!["--mcp".to_owned()],
            env: vec![],
        }
    }

    #[test]
    fn json_files_keep_their_other_servers_and_keys() {
        let text = r#"{"theme":"dark","mcpServers":{"other":{"command":"npx"}}}"#;
        let new = with_entry(ConfigFormat::Json, text, &entry()).unwrap();
        let doc: Value = serde_json::from_str(&new).unwrap();
        assert_eq!(doc["theme"], "dark");
        assert_eq!(doc["mcpServers"]["other"]["command"], "npx");
        assert_eq!(doc["mcpServers"]["platypus"]["args"], json!(["--mcp"]));
        assert!(
            new.find("theme") < new.find("mcpServers"),
            "order kept: {new}"
        );
        assert_eq!(
            state_of(ConfigFormat::Json, Some(&new), &entry()),
            InstallState::Installed
        );
        assert_eq!(
            state_of(ConfigFormat::Json, Some(text), &entry()),
            InstallState::Missing
        );
        assert!(with_entry(ConfigFormat::Json, "", &entry()).is_ok());
        assert!(with_entry(ConfigFormat::Json, "{oops", &entry()).is_err());
        assert!(with_entry(ConfigFormat::Json, r#"{"mcpServers":[]}"#, &entry()).is_err());
        assert_eq!(
            state_of(ConfigFormat::Json, Some("[1]"), &entry()),
            InstallState::Unreadable
        );
    }

    #[test]
    fn toml_files_keep_their_comments() {
        let text = "# my settings\nmodel = \"o3\"\n\n[mcp_servers.other]\ncommand = \"npx\"\n";
        let new = with_entry(ConfigFormat::Toml, text, &entry()).unwrap();
        assert!(new.starts_with("# my settings\nmodel = \"o3\"\n"), "{new}");
        assert!(new.contains("[mcp_servers.platypus]"), "{new}");
        assert_eq!(
            state_of(ConfigFormat::Toml, Some(&new), &entry()),
            InstallState::Installed
        );
        let other = ServerEntry {
            command: "/old/platypus-mcp".to_owned(),
            ..entry()
        };
        assert_eq!(
            state_of(ConfigFormat::Toml, Some(&new), &other),
            InstallState::Different
        );
        let snippet = snippet(ConfigFormat::Toml, &entry());
        assert!(snippet.starts_with("[mcp_servers.platypus]"), "{snippet}");
    }

    #[test]
    fn env_must_match_too() {
        let with_env = ServerEntry {
            env: vec![("PLATYPUS_DATA_DIR".to_owned(), "D:/p".to_owned())],
            ..entry()
        };
        let new = with_entry(ConfigFormat::Json, "{}", &with_env).unwrap();
        assert_eq!(
            state_of(ConfigFormat::Json, Some(&new), &with_env),
            InstallState::Installed
        );
        let plain = with_entry(ConfigFormat::Json, "{}", &entry()).unwrap();
        assert_eq!(
            state_of(ConfigFormat::Json, Some(&plain), &with_env),
            InstallState::Different
        );
    }

    #[test]
    fn installs_with_a_backup() {
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join("config");
        let cursor = home.path().join(".cursor").join("mcp.json");
        // A new file: its folder is made, and there is nothing to back up.
        let done = install("cursor", home.path(), &config, &entry(), "1").unwrap();
        assert_eq!(done.backup, None);
        assert!(cursor.exists());
        let done = install("cursor", home.path(), &config, &entry(), "2").unwrap();
        let backup = PathBuf::from(done.backup.unwrap());
        assert!(
            backup.ends_with("mcp.json.platypus-backup-2"),
            "{}",
            backup.display()
        );
        // An unreadable file is left alone.
        std::fs::write(&cursor, "{broken").unwrap();
        assert!(install("cursor", home.path(), &config, &entry(), "3").is_err());
        assert_eq!(std::fs::read_to_string(&cursor).unwrap(), "{broken");
        assert!(install("nope", home.path(), &config, &entry(), "4").is_err());

        let listed = list(home.path(), &config, &entry());
        assert_eq!(listed.len(), CLIENTS.len());
        let cursor = listed.iter().find(|c| c.id == "cursor").unwrap();
        assert_eq!(cursor.state, InstallState::Unreadable);
        let codex = listed.iter().find(|c| c.id == "codex").unwrap();
        assert!(!codex.exists);
        assert_eq!(codex.state, InstallState::Missing);
    }
}
