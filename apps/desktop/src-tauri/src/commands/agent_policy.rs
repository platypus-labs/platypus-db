//! The agent policy the user approved (see `platypus_mcp::policy`), what masks for agents
//! included: sealed when the app first sees it, kept up to date with what the user changes in the
//! app, and checked for edits made outside it that give agents more, which the page asks about.

use platypus_mcp::masking_seal::{MASK_NAMES, MASK_TYPES, SealedMarks};
use platypus_mcp::policy::{AgentPolicy, CONFIRM_WRITES, Loosening, Seal, Sealed, UNFILTERED};
use platypus_store::{
    AgentAccess, AgentLimits, AgentValues, PasswordUpdate, SavedConnection, SettingsStore, toml,
};
use tauri::State;
use uuid::Uuid;

use super::AppState;
use crate::error::{CommandError, CommandResult};

impl AppState {
    /// The seal of this profile's agent policy: next to the connections and settings.
    fn agent_seal(&self) -> Option<Seal> {
        let dir = self.vault_path.as_ref().ok()?.parent()?;
        Some(Seal::of(dir))
    }

    /// The policy the files say now.
    fn current_agent_policy(&self) -> CommandResult<AgentPolicy> {
        let connections = self.with_store(|s| Ok(s.list().to_vec()))?;
        self.with_settings(|s| Ok(AgentPolicy::current(&connections, Some(s))))
    }

    /// Records in the seal a change the user made in the app; `change` edits the approved
    /// policy. Nothing happens without a seal, or with a broken one, which the page asks
    /// about first.
    fn approve_agent_change(&self, change: impl FnOnce(&mut AgentPolicy)) {
        let Some(seal) = self.agent_seal() else {
            return;
        };
        if let Sealed::Approved(mut approved) = seal.read() {
            change(&mut approved);
            if let Err(e) = seal.write(&approved) {
                eprintln!("agent policy: {e}");
            }
        }
    }

    /// After the user saved connection `id` in the app with `access`: a change of it is theirs.
    pub(crate) fn approve_agent_access(
        &self,
        id: Uuid,
        before: Option<AgentAccess>,
        access: AgentAccess,
    ) {
        // Saving a form that shows a value edited outside the app approves nothing.
        if before != Some(access) {
            self.approve_agent_change(|p| {
                p.connections.insert(id, access);
            });
        }
    }

    /// After the user saved connection `id` in the app with `values`: a change of it is theirs.
    /// Only the Security page changes it, on a connection already saved: a new one, which may
    /// be a copy of one edited outside the app, approves nothing.
    pub(crate) fn approve_agent_values(
        &self,
        id: Uuid,
        before: Option<AgentValues>,
        values: AgentValues,
    ) {
        if before.is_some_and(|before| before != values) {
            self.approve_agent_change(|p| {
                if values == AgentValues::Shown {
                    p.values_shown.insert(id);
                } else {
                    p.values_shown.remove(&id);
                }
            });
        }
    }

    /// After the user saved connection `id` in the app with `limits`: a change of them is
    /// theirs, raised or lowered. As with the values, only on a connection already saved.
    pub(crate) fn approve_agent_limits(
        &self,
        id: Uuid,
        before: Option<AgentLimits>,
        limits: AgentLimits,
    ) {
        if before.is_some_and(|before| before != limits) {
            self.approve_agent_change(|p| {
                if limits.is_empty() {
                    p.limits.remove(&id);
                } else {
                    p.limits.insert(id, limits);
                }
            });
        }
    }

    /// After the user set `key` in the app's settings from `before` to `value`.
    pub(crate) fn approve_agent_setting(
        &self,
        key: &str,
        before: Option<&str>,
        value: Option<&toml::Value>,
    ) {
        if key == CONFIRM_WRITES {
            let on = value.and_then(toml::Value::as_bool).unwrap_or(true);
            self.approve_agent_change(|p| p.confirm_writes = on);
        } else if key == UNFILTERED {
            let off = value.and_then(toml::Value::as_str) == Some("off");
            self.approve_agent_change(|p| p.unfiltered_allowed = off);
        } else if key == MASK_NAMES || key == MASK_TYPES {
            let after = value.and_then(toml::Value::as_str);
            self.approve_agent_change(|p| {
                if let Some(rules) = &mut p.masking {
                    rules.apply_edit(key, before, after);
                }
            });
        }
    }

    /// Records in the seal a change of the sensitive marks the user made in the app; `change`
    /// edits the approved marks. Nothing happens before the first seal of masking, or with a
    /// broken seal, which the page asks about first.
    pub(crate) fn approve_agent_marks(&self, change: impl FnOnce(&mut SealedMarks)) {
        let Some(seal) = self.agent_seal() else {
            return;
        };
        let Sealed::Approved(approved) = seal.read() else {
            return;
        };
        if approved.masking.is_none() {
            return;
        }
        let Some(before) = seal.marks(&approved) else {
            return;
        };
        let mut marks = (*before).clone();
        change(&mut marks);
        if marks != *before
            && let Err(e) = seal.write_with_marks(&approved, &marks)
        {
            eprintln!("agent policy: {e}");
        }
    }

    /// The marks of `connections` in the catalog.
    fn catalog_marks(&self, connections: &[SavedConnection]) -> CommandResult<SealedMarks> {
        let marks = connections
            .iter()
            .map(|c| Ok((c.id, self.with_catalog(|s| s.sensitive_columns(c.id))?)))
            .collect::<CommandResult<Vec<_>>>()?;
        Ok(SealedMarks::from_catalog(marks))
    }

    /// Reads `settings.toml` again: the question is about the file, which may have changed
    /// outside the app since it was read. A file that cannot be read leaves what the app holds.
    fn reload_settings(&self) {
        if let Err(e) = self.with_settings(SettingsStore::reload) {
            eprintln!("agent policy: {}", e.message());
        }
    }
}

/// What the files give agents beyond what the user approved: edits made outside the app,
/// which agents do not get until the user accepts them. The first call seals the files as
/// they are; a stricter edit is sealed at once, as it needs no question. Without `marks`, as at
/// the app's start, this reads the sealed policy and compares what the app already holds, and
/// neither the marks nor the files are read.
#[tauri::command]
pub fn agent_policy_changes(
    state: State<'_, AppState>,
    marks: bool,
) -> CommandResult<Vec<Loosening>> {
    let Some(seal) = state.agent_seal() else {
        return Ok(Vec::new());
    };
    if marks {
        state.reload_settings();
    }
    let current = state.current_agent_policy()?;
    let connections = state.with_store(|s| Ok(s.list().to_vec()))?;
    Ok(seal.review(&current, &connections, marks, &|| {
        state
            .catalog_marks(&connections)
            .map_err(|e| e.message().to_owned())
    }))
}

/// The user's answer to [`agent_policy_changes`]: `accept` seals the files as they are;
/// otherwise they are put back to what was approved (the defaults, for a broken seal) and
/// sealed so.
#[tauri::command]
pub fn answer_agent_policy(state: State<'_, AppState>, accept: bool) -> CommandResult<()> {
    let seal = state
        .agent_seal()
        .ok_or_else(|| CommandError::new("store", "the profile's folder is not known"))?;
    state.reload_settings();
    let current = state.current_agent_policy()?;
    let connections = state.with_store(|s| Ok(s.list().to_vec()))?;
    let file = state.catalog_marks(&connections)?;
    let (policy, marks) = if accept {
        (current, file)
    } else {
        let undo = seal.undo(&current, &file);
        let honored = undo.policy;
        state.with_store(|s| {
            let back: Vec<_> = s
                .list()
                .iter()
                .filter(|c| {
                    c.agent_access != honored.access(c.id)
                        || c.agent_values != honored.values(c.id)
                        || c.agent_limits != honored.limits(c.id)
                })
                .map(|c| {
                    let mut c = c.clone();
                    c.agent_access = honored.access(c.id);
                    c.agent_values = honored.values(c.id);
                    c.agent_limits = honored.limits(c.id);
                    c
                })
                .collect();
            for c in back {
                s.save(c, PasswordUpdate::Keep)?;
            }
            Ok(())
        })?;
        state.with_settings(|s| {
            if honored.confirm_writes != current.confirm_writes {
                s.set(CONFIRM_WRITES, Some(toml::Value::Boolean(true)))?;
            }
            if honored.unfiltered_allowed != current.unfiltered_allowed {
                // The default again: asked before running.
                s.set(UNFILTERED, None)?;
            }
            if let (Some(rules), Some(files)) = (&honored.masking, &current.masking) {
                for (key, rule, file) in [
                    (MASK_NAMES, &rules.names, &files.names),
                    (MASK_TYPES, &rules.types, &files.types),
                ] {
                    if rule != file {
                        s.set(key, Some(toml::Value::String(rule.clone())))?;
                    }
                }
            }
            Ok(())
        })?;
        state.with_catalog(|c| {
            for (id, key, mark) in &undo.restore {
                c.set_sensitive(*id, key, *mark)?;
            }
            Ok(())
        })?;
        (honored, undo.marks)
    };
    seal.write_with_marks(&policy, &marks)
        .map_err(|e| CommandError::new("store", e))
}
