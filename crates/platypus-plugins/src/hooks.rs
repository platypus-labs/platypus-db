//! Event hooks: before and after a statement, on connect, before saving edited rows. The module
//! of a plugin with the `hooks` permission gets a [`HookContext`] and, for a "before" event, may
//! answer that the action stops; it has no way to change what runs.
//!
//! The caller asks [`HookTable::wanted`] first. That is one atomic load: with no hook on an
//! event, the query path neither waits nor allocates, and nothing was read at the app's start
//! beyond the list of plugins the page reads anyway.
//!
//! Every hook is bound: a new sandbox per call with less fuel than an action, and a limit on its
//! wall time, past which the action no longer waits for it. A hook that crashes, runs out of
//! fuel or time, or answers nonsense does not stop anything: the action goes ahead and the
//! caller gets a [`HookFailure`] once per plugin, to tell the user. The same goes for a plugin that
//! should have run and cannot: its files are gone, or the list of plugins cannot be read.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};
use std::time::Duration;

use futures::future::join_all;
use serde::{Deserialize, Serialize};
use wasmi::Module;

use crate::extensions::HookEvent;
use crate::manifest::{Capability, MODULE_FILE};
use crate::package::{Installed, Plugins};
use crate::runtime::{self, Grants, HOOK, Limits, Sandbox};
use crate::{PluginError, log};

/// Set while the table has not been read: every event then looks wanted, so that the first
/// asks. A hook has no way to be missed by coming before the list.
const UNREAD: u8 = 1 << 7;
/// Every event: what a plugin that cannot run hears of, to say so at the first one.
const ALL_EVENTS: u8 = !UNREAD;
/// Who failed when the list of plugins cannot be read: no plugin, and no id is empty.
const NO_LIST: &str = "";
/// The longest message of a hook that blocks, in characters.
const MAX_MESSAGE: usize = 500;
/// The longest statement a hook gets, in bytes.
const MAX_STATEMENT: usize = 64 << 10;

/// What one hook may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookLimits {
    pub sandbox: Limits,
    /// How long the action waits for it. A sandbox still running then stops by its fuel.
    pub time: Duration,
}

impl Default for HookLimits {
    fn default() -> Self {
        Self {
            sandbox: Limits {
                // A tenth of an action's: a fraction of a second of the interpreter.
                fuel: 300_000_000,
                message: 1 << 20,
                deadline: Duration::from_secs(2),
                http_timeout: Duration::from_millis(1500),
                log_lines: 20,
                ..Limits::default()
            },
            time: Duration::from_secs(2),
        }
    }
}

/// The connection an event is about: neither its host nor its user, and never a password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookConnection {
    pub name: String,
    pub driver: String,
    pub database: String,
    pub production: bool,
}

/// A statement as a hook sees it: masked by the caller, at most [`MAX_STATEMENT`] bytes. `None`
/// on a driver whose text cannot be masked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookStatement {
    pub text: Option<String>,
    pub truncated: bool,
}

impl HookStatement {
    /// The masked `text`, cut to the limit.
    #[must_use]
    pub fn new(text: Option<String>) -> Self {
        let mut truncated = false;
        let text = text.map(|mut text| {
            if text.len() > MAX_STATEMENT {
                let mut end = MAX_STATEMENT;
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                text.truncate(end);
                truncated = true;
            }
            text
        });
        Self { text, truncated }
    }
}

/// How a statement ended; not its error, which may quote values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum QueryStatus {
    Ok,
    Error,
    Cancelled,
}

/// What a hook is told. Each event has its own fields and none of them holds a value of a
/// result or a password: there is nowhere to put one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum HookContext {
    BeforeQuery {
        connection: HookConnection,
        statement: HookStatement,
    },
    AfterQuery {
        connection: HookConnection,
        statement: HookStatement,
        status: QueryStatus,
        elapsed_ms: u64,
        /// The rows it changed, when it says.
        affected: Option<u64>,
    },
    OnConnect {
        connection: HookConnection,
    },
    /// Only what changes and how much: the statements of a save quote the rows' values.
    BeforeSave {
        connection: HookConnection,
        schema: Option<String>,
        table: String,
        updates: usize,
        inserts: usize,
        deletes: usize,
    },
}

impl HookContext {
    #[must_use]
    pub const fn event(&self) -> HookEvent {
        match self {
            Self::BeforeQuery { .. } => HookEvent::BeforeQuery,
            Self::AfterQuery { .. } => HookEvent::AfterQuery,
            Self::OnConnect { .. } => HookEvent::OnConnect,
            Self::BeforeSave { .. } => HookEvent::BeforeSave,
        }
    }
}

/// What a hook answers. Nothing else fits: a statement in the answer makes it a broken hook.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct HookAnswer {
    action: HookAction,
    /// Why it blocks.
    #[serde(default)]
    message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum HookAction {
    Allow,
    Block,
}

/// A hook stopped the action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookBlock {
    /// The plugin's name.
    pub plugin: String,
    pub message: String,
}

/// A hook failed and the action went ahead; each plugin's only once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookFailure {
    /// The plugin's name; empty when the list of plugins cannot be read, so that no hook ran.
    pub plugin: String,
    pub reason: String,
}

/// What the hooks of an event said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HookReport {
    /// The first plugin, by id, that blocked.
    pub blocked: Option<HookBlock>,
    pub failures: Vec<HookFailure>,
}

/// A plugin's hooks, ready to run.
#[derive(Debug)]
struct Entry {
    id: String,
    name: String,
    events: u8,
    grants: Grants,
    limits: HookLimits,
    module_path: PathBuf,
    /// Compiled at its first call and kept: a new instance per call costs far less.
    module: OnceLock<Result<Module, String>>,
}

impl Entry {
    fn call(&self, request: &[u8]) -> Result<HookAnswer, PluginError> {
        let module = self.module.get_or_init(|| {
            std::fs::read(&self.module_path)
                .map_err(|e| e.to_string())
                .and_then(|wasm| runtime::compile(&wasm).map_err(|e| e.to_string()))
        });
        let module = module
            .as_ref()
            .map_err(|why| PluginError::Invalid(why.clone()))?;
        let mut sandbox = Sandbox::new(module, &self.grants, self.limits.sandbox)?;
        let answer = sandbox.call_raw(HOOK, request)?;
        runtime::read_answer(HOOK, &answer)
    }
}

/// Which plugins hook which events.
#[derive(Debug)]
pub struct HookTable {
    /// The bits of the events that have a hook, and [`UNREAD`].
    active: AtomicU8,
    entries: RwLock<Vec<Arc<Entry>>>,
    /// The plugins whose failure the user has been told of.
    told: Mutex<BTreeSet<String>>,
}

/// The app's hooks.
pub static HOOKS: HookTable = HookTable::new();

impl Default for HookTable {
    fn default() -> Self {
        Self::new()
    }
}

impl HookTable {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            active: AtomicU8::new(u8::MAX),
            entries: RwLock::new(Vec::new()),
            told: Mutex::new(BTreeSet::new()),
        }
    }

    /// Whether `event` may have a hook: no lock, no allocation, nothing to wait for.
    #[inline]
    #[must_use]
    pub fn wanted(&self, event: HookEvent) -> bool {
        self.active.load(Ordering::Relaxed) & event.bit() != 0
    }

    /// Fills the table from the plugins of `list`: those turned on, with the `hooks` permission
    /// approved. A plugin without it is not called at all. One whose files are gone may have been
    /// a guard: its events are not known any more, so the first of any says it did not run.
    pub fn load(&self, plugins: &Plugins, list: &[Installed]) {
        let entries: Vec<Arc<Entry>> = list
            .iter()
            .filter(|p| {
                p.enabled
                    && p.approved.allows(Capability::Hooks)
                    && (p.broken || !p.manifest.hooks.is_empty())
            })
            .map(|p| {
                let manifest = &p.manifest;
                let (events, module) = if p.broken {
                    let why = "its files are missing or damaged: remove it and install it again";
                    (ALL_EVENTS, OnceLock::from(Err(why.to_owned())))
                } else {
                    let events = manifest.hooks.iter().fold(0, |bits, e| bits | e.bit());
                    (events, OnceLock::new())
                };
                Arc::new(Entry {
                    id: manifest.id.clone(),
                    name: manifest.name.clone(),
                    events,
                    grants: plugins.grants(p),
                    limits: plugins.hook_limits(),
                    module_path: plugins.module_path(manifest).join(MODULE_FILE),
                    module,
                })
            })
            .collect();
        let active = entries.iter().fold(0, |bits, e| bits | e.events);
        *self.entries.write().unwrap_or_else(PoisonError::into_inner) = entries;
        self.active.store(active, Ordering::Relaxed);
    }

    /// A plugin changed: a failure of the new one is told again.
    pub fn forgive(&self, id: &str) {
        self.told
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
    }

    /// Runs the hooks of the context's event, at the same time and within their limits, and
    /// says what they answered. Reads the list of plugins first if nothing did yet. A list that
    /// cannot be read must not stop the user's queries, and a guard that does not run must not go
    /// unnoticed: the action goes ahead, the user is told once, and the table stays unread, so
    /// that a file put right counts from the next statement.
    pub async fn run(&self, plugins: &Plugins, context: &HookContext) -> HookReport {
        if self.active.load(Ordering::Relaxed) & UNREAD != 0 {
            match plugins.list() {
                Ok(list) => {
                    self.load(plugins, &list);
                    self.forgive(NO_LIST);
                }
                Err(e) => return self.list_failed(&e),
            }
        }
        let event = context.event();
        let entries: Vec<Arc<Entry>> = self
            .entries
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|e| e.events & event.bit() != 0)
            .cloned()
            .collect();
        if entries.is_empty() {
            return HookReport::default();
        }
        let request: Arc<[u8]> = match serde_json::to_vec(context) {
            Ok(bytes) => bytes.into(),
            Err(e) => {
                log("WARN", &format!("hooks: {e}"));
                return HookReport::default();
            }
        };
        let answers = join_all(entries.iter().map(|entry| {
            let (entry, request) = (Arc::clone(entry), Arc::clone(&request));
            run_one(entry, request)
        }))
        .await;
        self.report(event, &entries, answers)
    }

    fn report(
        &self,
        event: HookEvent,
        entries: &[Arc<Entry>],
        answers: Vec<Result<HookAnswer, PluginError>>,
    ) -> HookReport {
        let mut report = HookReport::default();
        for (entry, answer) in entries.iter().zip(answers) {
            match answer {
                Ok(answer) => {
                    if answer.action == HookAction::Block && event.can_block() {
                        report.blocked.get_or_insert_with(|| HookBlock {
                            plugin: entry.name.clone(),
                            message: clean_message(&answer.message),
                        });
                    }
                }
                Err(e) => {
                    // It may be the guest's own words: as short as a message.
                    let reason = clean_message(&e.to_string());
                    log(
                        "WARN",
                        &format!("plugin {}: its hook failed: {reason}", entry.id),
                    );
                    if self.first_failure(&entry.id) {
                        report.failures.push(HookFailure {
                            plugin: entry.name.clone(),
                            reason,
                        });
                    }
                }
            }
        }
        report
    }

    fn list_failed(&self, e: &PluginError) -> HookReport {
        let reason = clean_message(&e.to_string());
        log("WARN", &format!("hooks: no hook runs: {reason}"));
        let mut report = HookReport::default();
        if self.first_failure(NO_LIST) {
            report.failures.push(HookFailure {
                plugin: NO_LIST.to_owned(),
                reason,
            });
        }
        report
    }

    fn first_failure(&self, id: &str) -> bool {
        self.told
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.to_owned())
    }
}

/// One hook, on a blocking thread, for at most its time: past that the thread is left to run
/// out of fuel and nothing waits for it.
async fn run_one(entry: Arc<Entry>, request: Arc<[u8]>) -> Result<HookAnswer, PluginError> {
    let time = entry.limits.time;
    let call = tokio::task::spawn_blocking(move || entry.call(&request));
    match tokio::time::timeout(time, call).await {
        Ok(Ok(answer)) => answer,
        Ok(Err(e)) => Err(PluginError::Trap(e.to_string())),
        Err(_) => Err(PluginError::Timeout),
    }
}

/// A plugin's message as the user reads it: one short paragraph, never empty.
fn clean_message(message: &str) -> String {
    let flat: String = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_MESSAGE)
        .collect();
    let flat = flat.trim();
    if flat.is_empty() {
        "no reason given".to_owned()
    } else {
        flat.to_owned()
    }
}

#[cfg(test)]
mod tests;
