//! An example of a PlatypusDB settings page: the fields are in `plugin.toml`, the app draws them
//! beside the plugin in Settings › Plugins and keeps the values, and the module reads its own
//! with `setting`:
//!
//! - editor: "Show my settings" shows them in a message, of the secret only its length;
//! - editor: "Read a setting of someone else" asks for a key the manifest does not declare, and
//!   shows the refusal.
//!
//! Build: `cargo build --release --target wasm32-unknown-unknown`, then package `plugin.toml`
//! with `target/wasm32-unknown-unknown/release/settings_demo.wasm` renamed to `plugin.wasm`.

use platypus_plugin_sdk::{entry, setting};
use serde_json::{Value, json};

platypus_plugin_sdk::export_allocator!();

#[unsafe(no_mangle)]
pub extern "C" fn platypus_describe(ptr: i32, len: i32) -> i64 {
    entry(ptr, len, |_| {
        Ok(json!({
            "actions": [
                { "id": "show", "title": "Show my settings", "scope": "editor" },
                { "id": "other", "title": "Read a setting of someone else", "scope": "editor" },
            ]
        }))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn platypus_action(ptr: i32, len: i32) -> i64 {
    entry(ptr, len, |request| {
        let text = match request["action"].as_str() {
            Some("show") => show()?,
            Some("other") => match setting("password_of_another_plugin") {
                Ok(value) => format!("It read {value}."),
                Err(why) => format!("Refused: {why}"),
            },
            other => return Err(format!("unknown action {other:?}")),
        };
        Ok(json!({ "kind": "message", "text": text }))
    })
}

fn show() -> Result<String, String> {
    let token = match setting("token")? {
        Value::String(token) => format!("{} characters", token.len()),
        _ => "not set".to_owned(),
    };
    Ok(format!(
        "endpoint={} retries={} verbose={} mode={} token: {token}",
        setting("endpoint")?,
        setting("retries")?,
        setting("verbose")?,
        setting("mode")?,
    ))
}
