//! Plugin actions: commands the app lists in a "Plugins" submenu of the editor, the result
//! grid or the navigator, and runs with what that place holds.
//!
//! The guest's `platypus_action` receives `{"action": id, "scope": scope, "context": {...}}`
//! and answers an [`ActionOutcome`]: text to put in the editor, a new SQL tab, the clipboard
//! (with the `clipboard` permission) or a message.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::PluginError;
use crate::runtime::{ACTION, Grants, Limits, Sandbox};

/// The rows of a grid selection an action receives at most; the context says when there
/// were more (`"truncated": true`).
pub const MAX_GRID_ROWS: usize = 1000;

/// Where an action is offered, and so what its context holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionScope {
    /// The SQL editor: its text, the selection and the connection's driver.
    #[serde(rename = "editor")]
    Editor,
    /// A result grid: the selected rows and their columns' names and types.
    #[serde(rename = "grid")]
    Grid,
    /// An object of the navigator: the connection, without secrets, and the object.
    #[serde(rename = "navigator.object")]
    NavigatorObject,
}

/// An action, as the plugin describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionInfo {
    /// Unique within the plugin.
    pub id: String,
    /// The menu's text, in the plugin's own language.
    pub title: String,
    pub scope: ActionScope,
    /// A default shortcut, e.g. `Mod+Alt+U`; the user may change it.
    #[serde(default)]
    pub shortcut: Option<String>,
}

/// What the app does with an action's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionOutcome {
    /// Replaces the editor's selection, or its whole text without one.
    Replace { text: String },
    /// Opens a new SQL tab on the same connection with this text.
    NewTab { text: String },
    /// Puts the text on the clipboard: only with the `clipboard` permission.
    Copy { text: String },
    /// Shows a message.
    Message { text: String },
    /// Nothing to do.
    None,
}

/// Checks the actions of a description: ids unique and plain, titles not empty.
pub(crate) fn check(actions: &[ActionInfo]) -> Result<(), PluginError> {
    let mut seen = std::collections::HashSet::new();
    for action in actions {
        if action.id.is_empty()
            || action.id.len() > 64
            || !action
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            return Err(PluginError::Invalid(format!(
                "the action id {:?} must be letters, digits, `-`, `_` or `.`",
                action.id
            )));
        }
        if !seen.insert(action.id.as_str()) {
            return Err(PluginError::Invalid(format!(
                "two actions have the id {:?}",
                action.id
            )));
        }
        if action.title.trim().is_empty() {
            return Err(PluginError::Invalid(format!(
                "the action {:?} has no title",
                action.id
            )));
        }
    }
    Ok(())
}

/// The context with a grid's rows cut to [`MAX_GRID_ROWS`].
fn capped(scope: ActionScope, mut context: Value) -> Value {
    if scope == ActionScope::Grid
        && let Some(Value::Array(rows)) = context.get_mut("rows")
        && rows.len() > MAX_GRID_ROWS
    {
        rows.truncate(MAX_GRID_ROWS);
        context["truncated"] = Value::Bool(true);
    }
    context
}

/// Runs `action` of a plugin in a new sandbox; blocking (see [`crate::runtime`]).
pub(crate) fn run(
    module: &wasmi::Module,
    grants: &Grants,
    limits: Limits,
    clipboard: bool,
    action: &ActionInfo,
    context: Value,
) -> Result<ActionOutcome, PluginError> {
    let mut sandbox = Sandbox::new(module, grants, limits)?;
    let request = json!({
        "action": action.id,
        "scope": action.scope,
        "context": capped(action.scope, context),
    });
    let outcome: ActionOutcome = sandbox.call(ACTION, &request)?;
    if matches!(outcome, ActionOutcome::Copy { .. }) && !clipboard {
        return Err(PluginError::Denied(format!(
            "the plugin {:?} has no permission for the clipboard",
            grants.plugin
        )));
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(id: &str, title: &str) -> ActionInfo {
        ActionInfo {
            id: id.to_owned(),
            title: title.to_owned(),
            scope: ActionScope::Editor,
            shortcut: None,
        }
    }

    #[test]
    fn checks_ids_and_titles() {
        assert!(check(&[action("a", "A"), action("b.c", "B")]).is_ok());
        assert!(check(&[action("a", "A"), action("a", "B")]).is_err());
        assert!(check(&[action("a b", "A")]).is_err());
        assert!(check(&[action("", "A")]).is_err());
        assert!(check(&[action("a", " ")]).is_err());
    }

    #[test]
    fn reads_scopes_and_outcomes() {
        let info: ActionInfo = serde_json::from_value(
            json!({"id": "x", "title": "X", "scope": "navigator.object", "shortcut": "Mod+K"}),
        )
        .unwrap();
        assert_eq!(info.scope, ActionScope::NavigatorObject);
        let outcome: ActionOutcome =
            serde_json::from_value(json!({"kind": "new_tab", "text": "SELECT 1"})).unwrap();
        assert_eq!(
            outcome,
            ActionOutcome::NewTab {
                text: "SELECT 1".to_owned()
            }
        );
        assert!(serde_json::from_value::<ActionOutcome>(json!({"kind": "shell"})).is_err());
    }

    #[test]
    fn caps_grid_rows() {
        let rows: Vec<Value> = (0..MAX_GRID_ROWS + 5).map(|i| json!([i])).collect();
        let context = capped(ActionScope::Grid, json!({ "rows": rows }));
        assert_eq!(context["rows"].as_array().unwrap().len(), MAX_GRID_ROWS);
        assert_eq!(context["truncated"], true);
        let small = capped(ActionScope::Grid, json!({ "rows": [[1]] }));
        assert!(small.get("truncated").is_none());
    }
}
