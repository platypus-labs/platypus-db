//! The user's saved scripts and query history, read-only. Both hold SQL the user wrote and
//! ran, so they follow the agent access of connections: what is linked to, or was run on, a
//! connection agents may not use is not shown, and a script is never written from here.

use std::collections::HashMap;
use std::path::PathBuf;

use platypus_store::{
    HistoryOrigin, HistoryQuery, HistoryStatus, Project, StoreError, list_scripts, read_script,
    read_script_links, resolve_script, rfc3339, script_relative,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::{Agent, may_use};
use crate::error::{AgentError, Result};
use crate::redact::Redactor;
use platypus_sql::Dialect;
use platypus_sql::sensitive::{DEFAULT_NAMES, NameRules, mask_literals};

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

/// Which column names are sensitive in text: the rules of the settings (Safety, "Sensitive
/// columns, by name"), and the names of the columns the user marked sensitive. In text a name
/// is not tied to a table, so a mark "not sensitive" on one column does not lift the rules for
/// the others of that name: it is not used here. The type rules need types, which text lacks.
pub(crate) struct SensitiveNames {
    rules: NameRules,
    /// Lower case.
    marked: std::collections::HashSet<String>,
}

impl SensitiveNames {
    #[cfg(test)]
    pub(crate) fn new(rules: NameRules, marked: std::collections::HashSet<String>) -> Self {
        Self { rules, marked }
    }

    pub(crate) fn is(&self, name: &str) -> bool {
        self.marked.contains(&name.to_lowercase()) || self.rules.matches(name)
    }

    /// Whether the rules alone say `name` is sensitive.
    pub(crate) fn by_rule(&self, name: &str) -> bool {
        self.rules.matches(name)
    }

    /// `sql` with the values of sensitive columns masked.
    pub(crate) fn mask(&self, sql: &str, dialect: Dialect) -> String {
        mask_literals(sql, dialect, &|c| self.is(c))
    }

    /// A database error with the values it quotes of sensitive columns masked.
    pub(crate) fn mask_error(&self, text: &str) -> String {
        platypus_sql::sensitive::mask_error(text, &|c| self.is(c))
    }
}

/// What tells the sensitive columns, read once for a call: the rules and the user's marks.
pub(crate) struct Sensitivity {
    rules: NameRules,
    types: NameRules,
    catalog: Option<platypus_store::CatalogStore>,
}

impl Sensitivity {
    /// The rules on the types of columns.
    pub(crate) const fn types(&self) -> &NameRules {
        &self.types
    }

    /// The user's marks on the columns of `connection`. Marks that cannot be read would let
    /// values through: the call fails instead.
    pub(crate) fn marks(&self, connection: Uuid) -> Result<Vec<platypus_store::SensitiveColumn>> {
        let Some(catalog) = &self.catalog else {
            return Ok(Vec::new());
        };
        catalog
            .sensitive_columns(connection)
            .map_err(|e| AgentError::Store(format!("cannot read the sensitive columns: {e}")))
    }

    /// The sensitive names of `connections`.
    pub(crate) fn names(&self, connections: &[Uuid]) -> Result<SensitiveNames> {
        let mut marked = std::collections::HashSet::new();
        for id in connections {
            marked.extend(
                self.marks(*id)?
                    .into_iter()
                    .filter(|m| m.sensitive)
                    .map(|m| m.key.column.to_lowercase()),
            );
        }
        Ok(SensitiveNames {
            rules: self.rules.clone(),
            marked,
        })
    }
}

impl Agent {
    /// The rules of the settings and the catalog of the user's marks.
    pub(crate) fn sensitivity(&self) -> Result<Sensitivity> {
        let settings = self.profile().settings();
        let rule = |key: &str| settings.as_ref().and_then(|s| s.get::<String>(key));
        Ok(Sensitivity {
            rules: NameRules::new(
                &rule("safety.mask_names").unwrap_or_else(|| DEFAULT_NAMES.to_owned()),
            ),
            types: NameRules::new(&rule("safety.mask_types").unwrap_or_default()),
            catalog: self.profile().catalog()?,
        })
    }

    fn visible(&self) -> Result<Visible> {
        let (_, connections) = self.connections()?;
        Ok(Visible(
            connections
                .iter()
                .filter(|c| may_use(c))
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
        // The links of this file: by the path as the app keeps it (ignoring case), and by the
        // file itself, so another spelling, a symlink or a short name of a linked file does
        // not get past the check.
        let rel = script_relative(&root, &path);
        let visible = self.visible()?;
        let opened = std::fs::canonicalize(&path).ok();
        let linked: Vec<Uuid> = read_script_links(&root)
            .into_iter()
            .filter(|(key, _)| {
                key.eq_ignore_ascii_case(&rel)
                    || opened.as_ref().is_some_and(|o| {
                        std::fs::canonicalize(root.join(key)).ok().as_ref() == Some(o)
                    })
            })
            .map(|(_, id)| id)
            .collect();
        // A script linked to a connection that is off reads as missing, as in the list.
        if !linked.iter().all(|id| visible.allows(Some(*id))) {
            return Err(missing());
        }
        let connection = visible.name(linked.first().copied());
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
        // The values of sensitive columns: those of the script's connections, or of every
        // connection agents may use when it opens on none.
        let ids: Vec<Uuid> = if linked.is_empty() {
            visible.0.keys().copied().collect()
        } else {
            linked.clone()
        };
        let dialect = match linked.first() {
            Some(id) => self
                .profile()
                .connections()?
                .get(*id)
                .map_or(Dialect::Postgres, |c| Dialect::from(c.driver)),
            None => Dialect::Postgres,
        };
        let text = self.sensitivity()?.names(&ids)?.mask(&read.text, dialect);
        let note = read
            .truncated
            .then(|| format!("cut at {SCRIPT_BYTES} bytes: the script goes on"));
        Ok(ScriptContent {
            path: rel,
            connection,
            text,
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
        let sensitivity = self.sensitivity()?;
        // Per connection, not per entry: the name, the secrets to mask, the dialect and the
        // sensitive columns.
        let mut known: HashMap<Uuid, Option<(String, Redactor, Dialect, SensitiveNames)>> =
            HashMap::new();
        let mut items = Vec::new();
        for e in entries {
            if let std::collections::hash_map::Entry::Vacant(slot) = known.entry(e.connection_id) {
                let entry = match store.get(e.connection_id) {
                    Some(saved) => {
                        // Only what needs no secret lookup: a URL's password, and a password
                        // command that an error may echo.
                        let mut redactor = Redactor::default();
                        if let Some(command) = &store.effective(saved).password_command {
                            redactor.add(command);
                        }
                        let sensitive = sensitivity.names(&[e.connection_id])?;
                        Some((
                            saved.name.clone(),
                            redactor,
                            Dialect::from(saved.driver),
                            sensitive,
                        ))
                    }
                    None => None,
                };
                slot.insert(entry);
            }
            items.push(e);
        }
        Ok(items
            .into_iter()
            .filter_map(|e| {
                let (name, redactor, dialect, sensitive) = known.get(&e.connection_id)?.as_ref()?;
                Some(HistoryItem {
                    connection: name.clone(),
                    // A statement may carry a password too, e.g. `CREATE USER ... PASSWORD`,
                    // and the values of sensitive columns, which its error may quote.
                    sql: sensitive.mask(&redactor.apply(&e.sql), *dialect),
                    started_at: rfc3339(e.started_at),
                    duration_ms: e.duration_ms,
                    rows: e.rows,
                    status: e.status,
                    error: e.error.map(|m| sensitive.mask_error(&redactor.apply(&m))),
                    origin: e.origin.unwrap_or_else(|| "app".to_owned()),
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use platypus_store::{CatalogKey, CatalogStore, HistoryStore, NewHistoryEntry};

    use super::*;
    use crate::profile::Profile;

    const ID: &str = "00000000-0000-4000-8000-000000000001";

    fn agent(dir: &Path, settings: &str) -> Agent {
        std::fs::write(
            dir.join("connections.toml"),
            format!(
                "version = 4\n\n[[connections]]\nid = \"{ID}\"\nname = \"Db\"\ndriver = \"sqlite\"\n\
                 database = '{}'\n",
                dir.join("db.sqlite").display()
            ),
        )
        .unwrap();
        std::fs::write(dir.join("settings.toml"), settings).unwrap();
        let mut catalog = CatalogStore::open(&dir.join("catalog.db")).unwrap();
        let key = |column: &str| CatalogKey {
            database: String::new(),
            schema: "main".to_owned(),
            object: "users".to_owned(),
            column: column.to_owned(),
        };
        let id = Uuid::parse_str(ID).unwrap();
        catalog
            .set_sensitive(id, &key("private_note"), Some(true))
            .unwrap();
        catalog
            .set_sensitive(id, &key("phone_model"), Some(false))
            .unwrap();
        Agent::new(Profile::at(dir))
    }

    #[test]
    fn scripts_and_history_mask_the_values_of_sensitive_columns() {
        let dir = tempfile::tempdir().unwrap();
        let agent = agent(dir.path(), "");
        let scripts = dir.path().join("scripts").join("all");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(
            scripts.join("report.sql"),
            "SELECT * FROM users\nWHERE email = 'a@b.it' AND phone_model = 'Pixel'\n  AND private_note LIKE '%debt%' AND name = 'Ann';",
        )
        .unwrap();
        let read = agent
            .read_script(&ReadScriptArgs {
                path: "report.sql".to_owned(),
                project: None,
            })
            .unwrap();
        // The rules say email and phone_model (a mark "not sensitive" on one table's column
        // does not lift them in text, where the table is not known), the marks private_note.
        assert_eq!(
            read.text,
            "SELECT * FROM users\nWHERE email = '••••••' AND phone_model = '••••••'\n  AND private_note LIKE '••••••' AND name = 'Ann';"
        );

        let history = HistoryStore::open(&dir.path().join("history.db")).unwrap();
        history
            .record(&NewHistoryEntry {
                connection_id: Uuid::parse_str(ID).unwrap(),
                sql: "UPDATE users SET email = 'x@y.it', name = 'N' WHERE id = 1".to_owned(),
                started_at: SystemTime::now(),
                duration: Duration::from_millis(3),
                rows: Some(1),
                status: HistoryStatus::Ok,
                error: None,
                origin: None,
            })
            .unwrap();
        history
            .record(&NewHistoryEntry {
                connection_id: Uuid::parse_str(ID).unwrap(),
                sql: "INSERT INTO users (email) VALUES ('giulia@x.it')".to_owned(),
                started_at: SystemTime::now(),
                duration: Duration::from_millis(1),
                rows: None,
                status: HistoryStatus::Error,
                error: Some("Key (email)=(giulia@x.it) already exists".to_owned()),
                origin: None,
            })
            .unwrap();
        let found = agent
            .search_history(&HistoryArgs {
                connection: None,
                text: None,
                origin: None,
                limit: None,
            })
            .unwrap();
        let failed = found.iter().find(|h| h.sql.starts_with("INSERT")).unwrap();
        assert_eq!(failed.sql, "INSERT INTO users (email) VALUES ('••••••')");
        assert_eq!(
            failed.error.as_deref(),
            Some("Key (email)=(••••••) already exists")
        );
        let found: Vec<_> = found
            .into_iter()
            .filter(|h| h.sql.starts_with("UPDATE"))
            .collect();
        assert_eq!(
            found[0].sql,
            "UPDATE users SET email = '••••••', name = 'N' WHERE id = 1"
        );
    }

    #[test]
    fn the_rules_are_the_settings_ones() {
        let dir = tempfile::tempdir().unwrap();
        // Only `name` by the rules: email goes through, the marks still count.
        let agent = agent(dir.path(), "[safety]\nmask_names = \"name\"\n");
        let scripts = dir.path().join("scripts").join("all");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(
            scripts.join("q.sql"),
            "SELECT 1 FROM users WHERE email = 'a@b' AND name = 'Ann' AND private_note = 'x'",
        )
        .unwrap();
        let read = agent
            .read_script(&ReadScriptArgs {
                path: "q.sql".to_owned(),
                project: None,
            })
            .unwrap();
        assert_eq!(
            read.text,
            "SELECT 1 FROM users WHERE email = 'a@b' AND name = '••••••' AND private_note = '••••••'"
        );
    }
}
