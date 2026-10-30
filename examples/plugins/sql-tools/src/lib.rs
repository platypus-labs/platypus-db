//! An example of a PlatypusDB action plugin, one action per place:
//!
//! - editor: "Uppercase SQL keywords" rewrites the selection, or the whole text, with its
//!   keywords in capitals, leaving strings, quoted names and comments alone;
//! - grid: "Copy rows as Markdown table" puts the selected rows on the clipboard;
//! - navigator.object: "Count rows" opens a new SQL tab counting the table's rows.
//!
//! Build: `cargo build --release --target wasm32-unknown-unknown`, then package
//! `plugin.toml` with `target/wasm32-unknown-unknown/release/sql_tools.wasm` renamed to
//! `plugin.wasm`.

use platypus_plugin_sdk::entry;
use serde_json::{Value, json};

platypus_plugin_sdk::export_allocator!();

#[unsafe(no_mangle)]
pub extern "C" fn platypus_describe(ptr: i32, len: i32) -> i64 {
    entry(ptr, len, |_| {
        Ok(json!({
            "actions": [
                { "id": "uppercase", "title": "Uppercase SQL keywords", "scope": "editor", "shortcut": "Mod+Alt+U" },
                { "id": "markdown", "title": "Copy rows as Markdown table", "scope": "grid" },
                { "id": "count", "title": "Count rows", "scope": "navigator.object" },
            ]
        }))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn platypus_action(ptr: i32, len: i32) -> i64 {
    entry(ptr, len, |request| {
        let context = &request["context"];
        match request["action"].as_str() {
            Some("uppercase") => uppercase(context),
            Some("markdown") => markdown(context),
            Some("count") => count(context),
            other => Err(format!("unknown action {other:?}")),
        }
    })
}

/// The editor's selection, or its text, with the keywords in capitals.
fn uppercase(context: &Value) -> Result<Value, String> {
    let selected = context["selection"]["text"]
        .as_str()
        .filter(|s| !s.is_empty());
    let text = selected
        .or_else(|| context["text"].as_str())
        .ok_or("no text in the editor")?;
    Ok(json!({ "kind": "replace", "text": uppercase_keywords(text) }))
}

const KEYWORDS: &[&str] = &[
    "add",
    "all",
    "alter",
    "and",
    "as",
    "asc",
    "between",
    "by",
    "case",
    "column",
    "create",
    "cross",
    "default",
    "delete",
    "desc",
    "distinct",
    "drop",
    "else",
    "end",
    "exists",
    "from",
    "full",
    "group",
    "having",
    "in",
    "index",
    "inner",
    "insert",
    "into",
    "is",
    "join",
    "key",
    "left",
    "like",
    "limit",
    "not",
    "null",
    "offset",
    "on",
    "or",
    "order",
    "outer",
    "primary",
    "references",
    "right",
    "select",
    "set",
    "table",
    "then",
    "union",
    "unique",
    "update",
    "values",
    "view",
    "when",
    "where",
    "with",
];

/// `sql` with the words of [`KEYWORDS`] in capitals, outside strings, quoted names and
/// comments.
pub fn uppercase_keywords(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        if matches!(c, '\'' | '"' | '`') {
            // To the closing quote; a doubled one is part of the text.
            i += 1;
            while i < chars.len() {
                if chars[i] == c {
                    if chars.get(i + 1) == Some(&c) {
                        i += 1;
                    } else {
                        break;
                    }
                }
                i += 1;
            }
            i = (i + 1).min(chars.len());
        } else if c == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i = (i + 2).min(chars.len());
        } else if c.is_alphabetic() || c == '_' {
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if KEYWORDS.contains(&word.to_lowercase().as_str()) {
                out.push_str(&word.to_uppercase());
            } else {
                out.push_str(&word);
            }
            continue;
        } else {
            i += 1;
        }
        out.extend(&chars[start..i]);
    }
    out
}

/// The selected rows as a Markdown table, on the clipboard.
fn markdown(context: &Value) -> Result<Value, String> {
    let columns = context["columns"].as_array().ok_or("no columns")?;
    let rows = context["rows"].as_array().ok_or("no rows")?;
    if rows.is_empty() {
        return Ok(json!({ "kind": "message", "text": "Select some rows first." }));
    }
    let cell = |v: &Value| match v {
        Value::Null => "NULL".to_owned(),
        Value::String(s) => s.replace('|', "\\|").replace('\n', " "),
        other => other.to_string(),
    };
    let names: Vec<String> = columns.iter().map(|c| cell(&c["name"])).collect();
    let mut text = format!(
        "| {} |\n|{}|\n",
        names.join(" | "),
        vec![" --- "; names.len()].join("|")
    );
    for row in rows {
        let values: Vec<String> = match row {
            Value::Array(values) => values.iter().map(cell).collect(),
            Value::Object(map) => names
                .iter()
                .map(|n| map.get(n).map(cell).unwrap_or_default())
                .collect(),
            other => vec![cell(other)],
        };
        text.push_str(&format!("| {} |\n", values.join(" | ")));
    }
    if context["truncated"] == true {
        platypus_plugin_sdk::log(1, "only the first rows of the selection were copied");
    }
    Ok(json!({ "kind": "copy", "text": text }))
}

/// A new tab that counts the rows of the table.
fn count(context: &Value) -> Result<Value, String> {
    let target = context["target"].as_str().ok_or("no object")?;
    let kind = context["object"]["kind"].as_str().unwrap_or("table");
    if !matches!(
        kind,
        "table" | "view" | "materialized_view" | "foreign_table"
    ) {
        return Ok(
            json!({ "kind": "message", "text": format!("Only tables and views have rows, not a {kind}.") }),
        );
    }
    Ok(json!({ "kind": "new_tab", "text": format!("SELECT COUNT(*) FROM {target};") }))
}
