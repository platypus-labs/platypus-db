//! Extensions of the result grid: computed columns, ways to draw a cell and commands on a cell.
//!
//! The module's `platypus_grid` takes batches, never one cell at a time: the page caches what it
//! answers, by value or by row, and asks only for what it has not seen, away from drawing. Each
//! call is a new sandbox, with less fuel and time than an action, and none of them can reach the
//! network: a value read to draw a cell has nowhere to go, whatever the plugin's `http` says.
//!
//! The page gives a value as the grid shows it, so a masked cell comes as dots. What comes back
//! is text, or for a renderer a [`GridCell`] with no room for markup: the grid is a canvas, and a
//! cell's text is drawn as it is. One bad value costs its own cell; a call that traps, or runs out
//! of fuel or time, is an error of that batch, which the page shows in the column's cells.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wasmi::Module;

use crate::PluginError;
use crate::action::ActionOutcome;
use crate::extensions::GridExtension;
use crate::manifest::{Capability, MODULE_FILE};
use crate::package::{Installed, Plugins};
use crate::runtime::{self, GRID, Limits, Sandbox};

/// The most rows or values in one batch.
pub const MAX_BATCH: usize = 1000;
/// The longest value sent in a batch, in bytes; the page cuts what is longer.
pub const MAX_VALUE: usize = 16 << 10;
/// The longest text an extension may give a cell, in characters.
const MAX_TEXT: usize = 1000;
/// The longest hint after a value, or error, in characters.
const MAX_SHORT: usize = 200;

/// What one call of a grid extension may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridLimits {
    pub sandbox: Limits,
    /// How long the page waits for it; a sandbox still running then stops by its fuel.
    pub time: Duration,
}

impl Default for GridLimits {
    fn default() -> Self {
        Self {
            sandbox: Limits {
                // As a hook's: a fraction of a second of the interpreter.
                fuel: 300_000_000,
                message: 4 << 20,
                deadline: Duration::from_secs(2),
                http_timeout: Duration::from_millis(1),
                log_lines: 20,
                ..Limits::default()
            },
            time: Duration::from_secs(2),
        }
    }
}

/// A column of the result, as an extension sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridColumn {
    pub name: String,
    #[serde(rename = "type")]
    pub db_type: String,
    /// Its values come as the grid shows them: dots.
    #[serde(default)]
    pub masked: bool,
}

/// A value of a computed column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum GridValue {
    /// `null` shows as NULL.
    Value { text: Option<String> },
    /// Shown in its cell.
    Error { error: String },
}

/// How to draw a cell: all a renderer can say. Nothing here reads as markup: the grid draws text
/// on a canvas and takes colours as `#rgb`, `#rrggbb` or `#rrggbbaa`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridCell {
    /// Drawn in place of the value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    /// Muted text after the value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strike: bool,
}

/// A renderer's answer for one value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum GridRendered {
    /// Drawn this way; `None` draws the value as usual.
    Cell {
        cell: Option<GridCell>,
    },
    Error {
        error: String,
    },
}

/// The module of a plugin, compiled at its first batch and kept while its file stays the same:
/// a batch costs a new instance, not a compilation.
static MODULES: Mutex<Option<HashMap<PathBuf, (SystemTime, Module)>>> = Mutex::new(None);

fn module_of(path: &Path) -> Result<Module, PluginError> {
    let changed = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map_err(PluginError::io(path))?;
    let mut modules = MODULES.lock().unwrap_or_else(PoisonError::into_inner);
    let modules = modules.get_or_insert_with(HashMap::new);
    if let Some((at, module)) = modules.get(path)
        && *at == changed
    {
        return Ok(module.clone());
    }
    let wasm = std::fs::read(path).map_err(PluginError::io(path))?;
    let module = runtime::compile(&wasm)?;
    modules.insert(path.to_owned(), (changed, module.clone()));
    Ok(module)
}

/// `text` without control characters, at most `max` characters.
fn clean(text: &str, max: usize) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max)
        .collect()
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa`.
fn is_colour(text: &str) -> bool {
    text.strip_prefix('#').is_some_and(|hex| {
        matches!(hex.len(), 3 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
    })
}

/// A cell of a computed column from what the module answered for it.
fn read_value(value: Value) -> GridValue {
    let text = match value {
        Value::Null => None,
        Value::String(s) => Some(s),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Object(ref map) if map.contains_key("error") => {
            let error = map.get("error").map_or_else(String::new, |e| match e {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            });
            return GridValue::Error {
                error: clean(&error, MAX_SHORT),
            };
        }
        _ => {
            return GridValue::Error {
                error: "the plugin answered something that is not a value".to_owned(),
            };
        }
    };
    GridValue::Value {
        text: text.map(|t| clean(&t, MAX_TEXT)),
    }
}

/// A renderer's cell from what the module answered for it.
fn read_cell(value: Value) -> GridRendered {
    let map = match &value {
        Value::Null => return GridRendered::Cell { cell: None },
        Value::Object(map) => map,
        _ => {
            return GridRendered::Error {
                error: "the plugin answered something that is not a cell".to_owned(),
            };
        }
    };
    if let Some(error) = map.get("error") {
        let error = error
            .as_str()
            .map_or_else(|| error.to_string(), str::to_owned);
        return GridRendered::Error {
            error: clean(&error, MAX_SHORT),
        };
    }
    let cell: GridCell = match serde_json::from_value(value) {
        Ok(cell) => cell,
        Err(e) => {
            return GridRendered::Error {
                error: clean(&format!("not a cell: {e}"), MAX_SHORT),
            };
        }
    };
    for colour in [&cell.color, &cell.background].into_iter().flatten() {
        if !is_colour(colour) {
            return GridRendered::Error {
                error: clean(&format!("{colour:?} is not a colour"), MAX_SHORT),
            };
        }
    }
    GridRendered::Cell {
        cell: Some(GridCell {
            text: cell.text.map(|t| clean(&t, MAX_TEXT)),
            hint: cell.hint.map(|h| clean(&h, MAX_SHORT)),
            ..cell
        }),
    }
}

/// The answers of a batch: one per item of the request, or the batch is broken.
fn exactly(what: &str, values: Vec<Value>, count: usize) -> Result<Vec<Value>, PluginError> {
    if values.len() == count {
        Ok(values)
    } else {
        Err(PluginError::Invalid(format!(
            "asked for {count} {what}, the plugin answered {}",
            values.len()
        )))
    }
}

/// `value` within [`MAX_VALUE`] bytes, on a character's boundary: the page's limit counts its own
/// units, and a page is not what a module's memory should rely on.
fn capped(mut value: String) -> String {
    if value.len() > MAX_VALUE {
        let mut end = MAX_VALUE;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
    }
    value
}

fn check_batch(what: &str, count: usize) -> Result<(), PluginError> {
    if count > MAX_BATCH {
        return Err(PluginError::Denied(format!(
            "more than {MAX_BATCH} {what} in a batch"
        )));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColumnAnswer {
    values: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RenderAnswer {
    cells: Vec<Value>,
}

impl Plugins {
    /// The values of the computed column `extension` of `id` for `rows`, each row holding the
    /// values of `columns`, as the grid shows them.
    pub async fn grid_column(
        &self,
        id: &str,
        extension: &str,
        columns: Vec<GridColumn>,
        rows: Vec<Vec<Option<String>>>,
    ) -> Result<Vec<GridValue>, PluginError> {
        check_batch("rows", rows.len())?;
        let count = rows.len();
        let rows: Vec<Vec<Option<String>>> = rows
            .into_iter()
            .map(|row| row.into_iter().map(|v| v.map(capped)).collect())
            .collect();
        let installed = self.require(id, Capability::Grid)?;
        find(&installed.manifest.grid.columns, id, extension)?;
        let answer: ColumnAnswer = self
            .grid_call(
                &installed,
                json!({ "op": "column", "id": extension, "columns": columns, "rows": rows }),
            )
            .await?;
        Ok(exactly("rows", answer.values, count)?
            .into_iter()
            .map(read_value)
            .collect())
    }

    /// How the renderer `extension` of `id` draws `values` of `column`, as the grid shows them.
    pub async fn grid_render(
        &self,
        id: &str,
        extension: &str,
        column: GridColumn,
        values: Vec<String>,
    ) -> Result<Vec<GridRendered>, PluginError> {
        check_batch("values", values.len())?;
        let count = values.len();
        let values: Vec<String> = values.into_iter().map(capped).collect();
        let installed = self.require(id, Capability::Grid)?;
        find(&installed.manifest.grid.renderers, id, extension)?;
        let answer: RenderAnswer = self
            .grid_call(
                &installed,
                json!({ "op": "render", "id": extension, "column": column, "values": values }),
            )
            .await?;
        Ok(exactly("cells", answer.cells, count)?
            .into_iter()
            .map(read_cell)
            .collect())
    }

    /// Runs the command `extension` of `id` on a cell, as the grid shows it; the page applies
    /// the outcome, and text for the clipboard needs that permission.
    pub async fn grid_action(
        &self,
        id: &str,
        extension: &str,
        column: GridColumn,
        value: Option<String>,
    ) -> Result<ActionOutcome, PluginError> {
        let installed = self.require(id, Capability::Grid)?;
        find(&installed.manifest.grid.actions, id, extension)?;
        let value = value.map(capped);
        let outcome: ActionOutcome = self
            .grid_call(
                &installed,
                json!({ "op": "action", "id": extension, "column": column, "value": value }),
            )
            .await?;
        if matches!(outcome, ActionOutcome::Copy { .. }) && !installed.approved.clipboard {
            return Err(PluginError::Denied(format!(
                "the plugin {id:?} has no permission for the clipboard"
            )));
        }
        Ok(outcome)
    }

    /// One call in a new sandbox on a blocking thread, for at most its time. No HTTP.
    async fn grid_call<T: serde::de::DeserializeOwned + Send + 'static>(
        &self,
        installed: &Installed,
        request: Value,
    ) -> Result<T, PluginError> {
        let limits = self.grid_limits();
        let mut grants = self.grants(installed);
        grants.http.clear();
        let path = self.module_path(&installed.manifest).join(MODULE_FILE);
        let call = tokio::task::spawn_blocking(move || {
            let module = module_of(&path)?;
            let mut sandbox = Sandbox::new(&module, &grants, limits.sandbox)?;
            sandbox.call(GRID, &request)
        });
        match tokio::time::timeout(limits.time, call).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(e)) => Err(PluginError::Trap(e.to_string())),
            Err(_) => Err(PluginError::Timeout),
        }
    }
}

fn find<'a>(
    items: &'a [GridExtension],
    id: &str,
    extension: &str,
) -> Result<&'a GridExtension, PluginError> {
    items.iter().find(|e| e.id == extension).ok_or_else(|| {
        PluginError::Denied(format!(
            "the plugin {id:?} declares no grid extension {extension:?} of this kind"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_cut_to_the_limit_on_a_characters_boundary() {
        assert_eq!(capped("short".to_owned()), "short");
        let long = capped("x".repeat(MAX_VALUE + 10));
        assert_eq!(long.len(), MAX_VALUE);
        // Three bytes each: the limit falls inside a character.
        let wide = capped("€".repeat(MAX_VALUE));
        assert!(wide.len() <= MAX_VALUE && wide.len() > MAX_VALUE - 3);
        assert!(wide.chars().all(|c| c == '€'));
    }

    #[test]
    fn a_value_is_text_or_its_own_error() {
        let text = |t: &str| GridValue::Value {
            text: Some(t.to_owned()),
        };
        assert_eq!(read_value(json!("a")), text("a"));
        assert_eq!(read_value(json!(3)), text("3"));
        assert_eq!(read_value(json!(true)), text("true"));
        assert_eq!(read_value(json!(null)), GridValue::Value { text: None });
        assert_eq!(
            read_value(json!({"error": "bad"})),
            GridValue::Error {
                error: "bad".to_owned()
            }
        );
        for broken in [json!([1]), json!({"html": "<b>x</b>"})] {
            assert!(matches!(read_value(broken), GridValue::Error { .. }));
        }
        let long = read_value(json!("x\n".repeat(MAX_TEXT)));
        let GridValue::Value { text: Some(long) } = long else {
            panic!("a value")
        };
        assert_eq!(long.chars().count(), MAX_TEXT);
        assert!(!long.contains('\n'));
    }

    #[test]
    fn a_cell_says_what_to_draw_and_nothing_else() {
        assert_eq!(read_cell(json!(null)), GridRendered::Cell { cell: None });
        assert_eq!(
            read_cell(
                json!({"text": "ok", "color": "#0f0", "background": "#00ff0040", "strike": true})
            ),
            GridRendered::Cell {
                cell: Some(GridCell {
                    text: Some("ok".to_owned()),
                    color: Some("#0f0".to_owned()),
                    background: Some("#00ff0040".to_owned()),
                    hint: None,
                    strike: true,
                })
            }
        );
        for broken in [
            json!({"html": "<img src=x onerror=alert(1)>"}),
            json!({"color": "red; background: url(x)"}),
            json!({"background": "url(http://evil)"}),
            json!({"color": "#12"}),
            json!("<b>x</b>"),
            json!({"error": "no"}),
        ] {
            assert!(
                matches!(read_cell(broken.clone()), GridRendered::Error { .. }),
                "{broken}"
            );
        }
    }

    #[test]
    fn a_batch_answers_for_all_it_was_given() {
        assert!(exactly("rows", vec![json!(1)], 1).is_ok());
        assert!(exactly("rows", vec![json!(1)], 2).is_err());
        assert!(check_batch("rows", MAX_BATCH).is_ok());
        assert!(check_batch("rows", MAX_BATCH + 1).is_err());
    }

    #[test]
    fn the_limits_are_less_than_an_action_s_and_have_no_network_time() {
        let limits = GridLimits::default();
        assert!(limits.sandbox.fuel < Limits::default().fuel);
        assert!(limits.time <= Duration::from_secs(2));
    }
}
