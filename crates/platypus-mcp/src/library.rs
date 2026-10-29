//! The user's saved scripts and query history, read-only. Both hold SQL the user wrote and
//! ran, so they follow the agent access of connections: what is linked to, or was run on, a
//! connection agents may not use is not shown, and a script is never written from here.

use std::collections::HashMap;
use std::path::PathBuf;

use platypus_store::{
    AgentAccess, HistoryOrigin, HistoryQuery, HistoryStatus, Project, StoreError, list_scripts,
    read_script, read_script_links, resolve_script, rfc3339, script_relative,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::Agent;
use crate::error::{AgentError, Result};
use crate::redact::Redactor;

/// Bytes of a script `read_script` returns at most.
pub const SCRIPT_BYTES: usize = 256 * 1024;

/// Entries `search_history` returns by default, and at most.
pub const HISTORY_PAGE: usize = 50;
pub const MAX_HISTORY_PAGE: usize = 500;

/// The arguments of `list_scripts`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListScriptsArgs {
    /// The project's name, as the app shows it; left out for All connections.
    pub project: Option<String>,
}

/// The arguments of `read_script`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReadScriptArgs {
    /// The script's path, as `list_scripts` shows it, e.g. `reports/monthly.sql`.
    pub path: String,
    /// The project's name, as the app shows it; left out for All connections.
    pub project: Option<String>,
}

/// Who ran the statements `search_history` returns.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum OriginArg {
    /// Run by the user in the PlatypusDB app.
    App,
    /// Run by an external agent.
    Agent,
}

/// The arguments of `search_history`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HistoryArgs {
    /// Only this connection's statements: its name, as `list_connections` shows it.
    pub connection: Option<String>,
    /// Text the statement contains, ignoring case.
    pub text: Option<String>,
    /// `app` for statements the user ran, `agent` for those agents ran; both when left out.
    pub origin: Option<OriginArg>,
    /// Entries returned at most: 50 by default, up to 500.
    pub limit: Option<u32>,
}

/// A script or folder as agents see it.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ScriptInfo {
    pub path: String,
    pub folder: bool,
    /// The name of the connection the script opens on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
}

/// One script's text.
#[derive(Debug, Serialize)]
pub struct ScriptContent {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    pub text: String,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// An executed statement as agents see it.
#[derive(Debug, Serialize)]
pub struct HistoryItem {
    pub connection: String,
    pub sql: String,
    /// RFC 3339, UTC.
    pub started_at: String,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<u64>,
    pub status: HistoryStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// `app`, or the name of the agent that ran it.
    pub origin: String,
}

/// The project named `name` (All connections when `None`) among `projects`, matched as
/// connections are: exactly, else ignoring case.
fn find_project<'a>(projects: &'a [Project], name: &str) -> Result<&'a Project> {
    let exact: Vec<&Project> = projects.iter().filter(|p| p.name == name).collect();
    let found = if exact.is_empty() {
        let loose = |p: &&Project| p.name.trim().eq_ignore_ascii_case(name.trim());
        projects.iter().filter(loose).collect()
    } else {
        exact
    };
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(AgentError::NotFound(format!(
            "no project named `{name}`: the projects are {}; leave project out for All connections",
            projects
                .iter()
                .map(|p| format!("`{}`", p.name))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
        _ => Err(AgentError::Invalid(format!(
            "several projects are named `{name}`: rename one in the app"
        ))),
    }
}

/// The names of the connections agents may use, by id.
struct Visible(HashMap<Uuid, String>);

impl Visible {
    /// Whether a script linked to `id` (or to nothing) may be shown.
    fn allows(&self, id: Option<Uuid>) -> bool {
        id.is_none_or(|id| self.0.contains_key(&id))
    }

    fn name(&self, id: Option<Uuid>) -> Option<String> {
        id.and_then(|id| self.0.get(&id).cloned())
    }
}

impl Agent {
    fn visible(&self) -> Result<Visible> {
        let store = self.profile().connections()?;
        Ok(Visible(
            store
                .list()
                .iter()
                .filter(|c| c.effective_agent_access() != AgentAccess::Off)
                .map(|c| (c.id, c.name.clone()))
                .collect(),
        ))
    }

    /// The folder of the scripts of `project`, as the app keeps them; it need not exist.
    fn scripts_root(&self, project: Option<&str>) -> Result<PathBuf> {
        let folder = match project {
            Some(name) => {
                let store = self.profile().connections()?;
                find_project(store.projects(), name)?.id.to_string()
            }
            None => "all".to_owned(),
        };
        Ok(self.profile().data_dir().join("scripts").join(folder))
    }

    /// The saved scripts of a project, without those linked to a connection agents may not
    /// use (access off, or deleted) and the folders left empty.
    pub fn list_scripts(&self, args: &ListScriptsArgs) -> Result<Vec<ScriptInfo>> {
        let root = self.scripts_root(args.project.as_deref())?;
        let visible = self.visible()?;
        let files: Vec<_> = list_scripts(&root)
            .into_iter()
            .filter(|e| e.folder || visible.allows(e.connection))
            .collect();
        let has_files = |folder: &str| {
            let prefix = format!("{folder}/");
            files
                .iter()
                .any(|e| !e.folder && e.path.starts_with(&prefix))
        };
        let shown = files
            .iter()
            .filter(|e| !e.folder || has_files(&e.path))
            .map(|e| ScriptInfo {
                path: e.path.clone(),
                folder: e.folder,
                connection: visible.name(e.connection),
            })
            .collect();
        Ok(shown)
    }

    /// The text of one saved script, up to [`SCRIPT_BYTES`].
    pub fn read_script(&self, args: &ReadScriptArgs) -> Result<ScriptContent> {
        let root = self.scripts_root(args.project.as_deref())?;
        let missing = || {
            AgentError::NotFound(format!(
                "no script `{}`: list_scripts shows them",
                args.path
            ))
        };
        let path = resolve_script(&root, &args.path).ok_or_else(|| {
            AgentError::Invalid(format!(
                "`{}` is not a script path inside the project's folder",
                args.path
            ))
        })?;
        // The link by the path as the app keeps it, ignoring case: file systems that do
        // would otherwise serve a linked script under another spelling.
        let rel = script_relative(&root, &path);
        let visible = self.visible()?;
        let links = read_script_links(&root);
        let linked = links
            .iter()
            .filter(|(p, _)| p.eq_ignore_ascii_case(&rel))
            .map(|(_, id)| *id);
        // A script linked to a connection that is off reads as missing, as in the list.
        if !linked.clone().all(|id| visible.allows(Some(id))) {
            return Err(missing());
        }
        let connection = visible.name(linked.clone().next());
        let read = read_script(&root, &rel, SCRIPT_BYTES).map_err(|e| match e {
            StoreError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
                missing()
            }
            StoreError::Io { source, .. } if source.kind() == std::io::ErrorKind::InvalidInput => {
                AgentError::Invalid(format!("`{}` is not a script of the project", args.path))
            }
            // The message names the profile's path, which agents have no use for.
            _ => AgentError::Store(format!("cannot read `{}`", args.path)),
        })?;
        // Group Q's sensitive columns, once they exist, would be masked in `read.text` here.
        let note = read
            .truncated
            .then(|| format!("cut at {SCRIPT_BYTES} bytes: the script goes on"));
        Ok(ScriptContent {
            path: rel,
            connection,
            text: read.text,
            truncated: read.truncated,
            note,
        })
    }

    /// The query history of the connections agents may use, newest first.
    pub fn search_history(&self, args: &HistoryArgs) -> Result<Vec<HistoryItem>> {
        let store = self.profile().connections()?;
        let ids = match &args.connection {
            Some(name) => vec![self.find(name)?.1.id],
            None => self.visible()?.0.into_keys().collect(),
        };
        let limit = args
            .limit
            .map_or(HISTORY_PAGE, |n| (n as usize).clamp(1, MAX_HISTORY_PAGE));
        let query = HistoryQuery {
            connection_ids: Some(ids),
            text: args.text.clone(),
            before_id: None,
            origin: args.origin.map(|o| match o {
                OriginArg::App => HistoryOrigin::App,
                OriginArg::Agent => HistoryOrigin::Agent,
            }),
            limit,
        };
        let entries = self.profile().history()?.search(&query)?;
        Ok(entries
            .into_iter()
            .filter_map(|e| {
                let saved = store.get(e.connection_id)?;
                // Only what needs no secret lookup: a URL's password, and a password command
                // that an error may echo.
                let mut redactor = Redactor::default();
                if let Some(command) = &store.effective(saved).password_command {
                    redactor.add(command);
                }
                // Group Q's sensitive columns, once they exist, would be masked in `e.sql`
                // and in the error here.
                Some(HistoryItem {
                    connection: saved.name.clone(),
                    sql: e.sql,
                    started_at: rfc3339(e.started_at),
                    duration_ms: e.duration_ms,
                    rows: e.rows,
                    status: e.status,
                    error: e.error.map(|m| redactor.apply(&m)),
                    origin: e.origin.unwrap_or_else(|| "app".to_owned()),
                })
            })
            .collect())
    }
}
