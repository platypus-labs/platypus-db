//! An example of a PlatypusDB tab and side panel. The pages are `ui/notes.html` and
//! `ui/outline.html`, each in an isolated frame; this module is what the pages may call with
//! `platypus.request("run_action", ...)`, and an action of the editor shows the same.
//!
//! Build: `cargo build --release --target wasm32-unknown-unknown`, then package `plugin.toml` and
//! `ui/` with `target/wasm32-unknown-unknown/release/panel_demo.wasm` renamed to `plugin.wasm`.

use platypus_plugin_sdk::entry;
use serde_json::json;

platypus_plugin_sdk::export_allocator!();

#[unsafe(no_mangle)]
pub extern "C" fn platypus_describe(ptr: i32, len: i32) -> i64 {
    entry(ptr, len, |_| {
        Ok(json!({ "actions": [
            { "id": "count-words", "title": "Count words", "scope": "editor" }
        ] }))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn platypus_action(ptr: i32, len: i32) -> i64 {
    entry(ptr, len, |request| {
        let text = request["context"]["text"].as_str().unwrap_or_default();
        let words = text.split_whitespace().count();
        Ok(json!({ "kind": "message", "text": format!("{words} words") }))
    })
}
