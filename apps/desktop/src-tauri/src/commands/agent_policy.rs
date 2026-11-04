//! The agent policy the user approved (see `platypus_mcp::policy`): sealed when the app first
//! sees it, kept up to date with what the user changes in the app, and checked for edits made
//! outside it that give agents more, which the page asks about.

use platypus_mcp::policy::{AgentPolicy, CONFIRM_WRITES, Loosening, Seal, Sealed, UNFILTERED};
use platypus_store::{AgentAccess, AgentValues, PasswordUpdate, toml};
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

    /// After the user set `key` in the app's settings to `value`.
    pub(crate) fn approve_agent_setting(&self, key: &str, value: Option<&toml::Value>) {
        if key == CONFIRM_WRITES {
            let on = value.and_then(toml::Value::as_bool).unwrap_or(true);
            self.approve_agent_change(|p| p.confirm_writes = on);
        } else if key == UNFILTERED {
            let off = value.and_then(toml::Value::as_str) == Some("off");
            self.approve_agent_change(|p| p.unfiltered_allowed = off);
        }
    }
}

/// What the files give agents beyond what the user approved: edits made outside the app,
/// which agents do not get until the user accepts them. The first call seals the files as
/// they are; a stricter edit is sealed at once, as it needs no question.
#[tauri::command]
pub fn agent_policy_changes(state: State<'_, AppState>) -> CommandResult<Vec<Loosening>> {
    let Some(seal) = state.agent_seal() else {
        return Ok(Vec::new());
    };
    let current = state.current_agent_policy()?;
    let connections = state.with_store(|s| Ok(s.list().to_vec()))?;
    let sealed = seal.read();
    let write = match &sealed {
        Sealed::Nothing => Some(current.clone()),
        Sealed::Approved(approved) => {
            let tightened = current.within(approved);
            (tightened != *approved).then_some(tightened)
        }
        Sealed::Broken => None,
    };
    if let Some(policy) = write {
        // Without a keychain there is nothing to compare with: agents get the files.
        if let Err(e) = seal.write(&policy) {
            eprintln!("agent policy: {e}");
        }
    }
    Ok(sealed.loosenings(&current, &connections))
}

/// The user's answer to [`agent_policy_changes`]: `accept` seals the files as they are;
/// otherwise they are put back to what was approved (the defaults, for a broken seal) and
/// sealed so.
#[tauri::command]
pub fn answer_agent_policy(state: State<'_, AppState>, accept: bool) -> CommandResult<()> {
    let seal = state
        .agent_seal()
        .ok_or_else(|| CommandError::new("store", "the profile's folder is not known"))?;
    let current = state.current_agent_policy()?;
    let policy = if accept {
        current
    } else {
        let honored = seal.read().honored(&current);
        state.with_store(|s| {
            let back: Vec<_> = s
                .list()
                .iter()
                .filter(|c| {
                    c.agent_access != honored.access(c.id) || c.agent_values != honored.values(c.id)
                })
                .map(|c| {
                    let mut c = c.clone();
                    c.agent_access = honored.access(c.id);
                    c.agent_values = honored.values(c.id);
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
            Ok(())
        })?;
        honored
    };
    seal.write(&policy)
        .map_err(|e| CommandError::new("store", e))
}
