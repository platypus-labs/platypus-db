//! What a manifest of ABI 2 declares beyond its kinds: the events it hooks, its tabs and panels,
//! its grid extensions, its settings and its theme. Declared here so that a package is reviewed
//! and refused in one place, whichever part of the app ends up using each of them.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};

/// The most tabs, panels, grid extensions or settings fields one plugin may declare.
const MAX_DECLARED: usize = 64;
/// The longest title, label or description, in characters.
const MAX_TEXT: usize = 500;

/// An event a plugin may hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    /// A statement is about to run; the hook may block it.
    BeforeQuery,
    /// A statement ended.
    AfterQuery,
    /// A connection was opened.
    OnConnect,
    /// Edited rows are about to be saved; the hook may block it.
    BeforeSave,
}

impl HookEvent {
    /// Its bit in the set of events that have a hook.
    #[must_use]
    pub const fn bit(self) -> u8 {
        match self {
            Self::BeforeQuery => 1,
            Self::AfterQuery => 1 << 1,
            Self::OnConnect => 1 << 2,
            Self::BeforeSave => 1 << 3,
        }
    }

    /// The hook's answer can stop the action.
    #[must_use]
    pub const fn can_block(self) -> bool {
        matches!(self, Self::BeforeQuery | Self::BeforeSave)
    }
}

/// A tab kind the plugin adds: an HTML document in an isolated frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TabDecl {
    /// Unique within the plugin.
    pub kind: String,
    pub title: String,
    /// The document, a path inside the package.
    pub entry: String,
}

/// A side panel the plugin adds: an HTML document in an isolated frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelDecl {
    /// Unique within the plugin.
    pub id: String,
    pub title: String,
    /// The document, a path inside the package.
    pub entry: String,
}

/// One extension of the result grid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridExtension {
    /// Unique within the plugin's grid extensions.
    pub id: String,
    pub title: String,
    /// The database types it applies to; all of them when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<String>,
}

/// What a plugin adds to the result grid.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GridDecl {
    /// Computed columns.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<GridExtension>,
    /// Ways to draw a cell.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub renderers: Vec<GridExtension>,
    /// Commands on a cell.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<GridExtension>,
}

impl GridDecl {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty() && self.renderers.is_empty() && self.actions.is_empty()
    }
}

/// A theme: colours by the names of a theme file, and fonts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeDecl {
    /// As the theme picker shows it.
    pub name: String,
    /// The colours of its light variant.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub light: BTreeMap<String, String>,
    /// The colours of its dark variant.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dark: BTreeMap<String, String>,
    /// Font families by their use (`ui`, `mono`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fonts: BTreeMap<String, String>,
}

/// The control of a settings field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingKind {
    Text,
    Number,
    Boolean,
    Choice,
    /// Kept in the system's keychain, never in a file; the page never reads it back.
    Secret,
}

/// One value a `choice` field offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingChoice {
    pub value: String,
    pub label: String,
}

/// One field of a plugin's settings page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingField {
    /// Unique within the plugin: lowercase letters, digits, `-` and `_`.
    pub key: String,
    /// In the plugin's own language.
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(rename = "type")]
    pub kind: SettingKind,
    /// The value until the user sets one; a secret has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    /// What a `choice` field offers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<SettingChoice>,
    /// The bounds of a `number` field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<Number>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<Number>,
}

impl SettingField {
    /// Why `value` cannot be this field's value; `None` when it can.
    #[must_use]
    pub fn refuses(&self, value: &Value) -> Option<String> {
        let (key, label) = (&self.key, self.kind_name());
        let fits = match self.kind {
            SettingKind::Text | SettingKind::Secret => value.is_string(),
            SettingKind::Boolean => value.is_boolean(),
            SettingKind::Number => value.is_number(),
            SettingKind::Choice => value
                .as_str()
                .is_some_and(|v| self.options.iter().any(|o| o.value == v)),
        };
        if !fits {
            return Some(format!("{value} is not a valid {label} for `{key}`"));
        }
        let number = value.as_f64();
        let below = |bound: &Number| number.zip(bound.as_f64()).is_some_and(|(n, b)| n < b);
        let above = |bound: &Number| number.zip(bound.as_f64()).is_some_and(|(n, b)| n > b);
        if self.min.as_ref().is_some_and(below) || self.max.as_ref().is_some_and(above) {
            return Some(format!("{value} is out of range for `{key}`"));
        }
        None
    }

    const fn kind_name(&self) -> &'static str {
        match self.kind {
            SettingKind::Text => "text",
            SettingKind::Number => "number",
            SettingKind::Boolean => "boolean",
            SettingKind::Choice => "choice",
            SettingKind::Secret => "secret",
        }
    }
}

/// Checks the settings fields of a manifest.
pub(crate) fn check_settings(fields: &[SettingField]) -> Result<(), String> {
    check_count("settings", fields.len())?;
    let mut seen = HashSet::new();
    for field in fields {
        let key = &field.key;
        if !is_setting_key(key) {
            return Err(format!(
                "the settings key {key:?} must be lowercase letters, digits, `-` or `_`"
            ));
        }
        if !seen.insert(key.as_str()) {
            return Err(format!("two settings have the key {key:?}"));
        }
        check_text(&format!("the label of the setting {key:?}"), &field.label)?;
        if field.description.chars().count() > MAX_TEXT {
            return Err(format!(
                "the description of the setting {key:?} is too long"
            ));
        }
        check_field_shape(field)?;
    }
    Ok(())
}

fn check_field_shape(field: &SettingField) -> Result<(), String> {
    let key = &field.key;
    if field.kind == SettingKind::Secret && field.default.is_some() {
        return Err(format!(
            "the secret setting {key:?} has a default: a secret is never written in a file"
        ));
    }
    if (field.kind == SettingKind::Choice) == field.options.is_empty() {
        return Err(format!(
            "`options` belongs to `choice` settings, and every one has some: {key:?}"
        ));
    }
    if field.kind != SettingKind::Number && (field.min.is_some() || field.max.is_some()) {
        return Err(format!("only a number has `min` and `max`: {key:?}"));
    }
    if let Some(why) = field.default.as_ref().and_then(|d| field.refuses(d)) {
        return Err(format!("the default of the setting {key:?}: {why}"));
    }
    Ok(())
}

/// Checks a list of tabs or panels, as `(id, title, entry)`.
pub(crate) fn check_documents<'a>(
    what: &str,
    items: impl ExactSizeIterator<Item = (&'a str, &'a str, &'a str)>,
) -> Result<(), String> {
    check_count(what, items.len())?;
    let mut seen = HashSet::new();
    for (id, title, entry) in items {
        check_id(what, id, &mut seen)?;
        check_text(&format!("the title of {id:?} in `{what}`"), title)?;
        if !is_package_document(entry) {
            return Err(format!(
                "{entry:?} in `{what}` is not an .html file inside the package"
            ));
        }
    }
    Ok(())
}

/// Checks the grid extensions of a manifest.
pub(crate) fn check_grid(grid: &GridDecl) -> Result<(), String> {
    for (what, items) in [
        ("grid.columns", &grid.columns),
        ("grid.renderers", &grid.renderers),
        ("grid.actions", &grid.actions),
    ] {
        check_count(what, items.len())?;
        let mut seen = HashSet::new();
        for item in items {
            check_id(what, &item.id, &mut seen)?;
            check_text(
                &format!("the title of {:?} in `{what}`", item.id),
                &item.title,
            )?;
        }
    }
    Ok(())
}

/// Checks the theme of a manifest.
pub(crate) fn check_theme(theme: &ThemeDecl) -> Result<(), String> {
    check_text("the name of the theme", &theme.name)?;
    if theme.light.is_empty() && theme.dark.is_empty() {
        return Err("the theme has neither `light` nor `dark` colours".to_owned());
    }
    Ok(())
}

fn check_count(what: &str, count: usize) -> Result<(), String> {
    if count > MAX_DECLARED {
        return Err(format!("`{what}` has more than {MAX_DECLARED} items"));
    }
    Ok(())
}

fn check_id<'a>(what: &str, id: &'a str, seen: &mut HashSet<&'a str>) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(format!(
            "the id {id:?} in `{what}` must be letters, digits, `-`, `_` or `.`"
        ));
    }
    if !seen.insert(id) {
        return Err(format!("two items of `{what}` have the id {id:?}"));
    }
    Ok(())
}

fn check_text(what: &str, text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err(format!("{what} is empty"));
    }
    if text.chars().count() > MAX_TEXT {
        return Err(format!("{what} is too long"));
    }
    Ok(())
}

/// A settings key: also one component of a dotted key in the store, so without dots.
fn is_setting_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'))
}

/// A relative path to an `.html` file that stays inside the package.
fn is_package_document(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".html") && is_package_path(path)
}

/// A relative path that stays inside the package: no `..`, no root, no drive, no backslash and
/// nothing hidden, on any system. A `%` is refused too, so nothing is decoded on the way in.
pub(crate) fn is_package_path(path: &str) -> bool {
    path.len() <= 200
        && path.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
}
