//! An example of PlatypusDB event hooks:
//!
//! - `before_query` blocks a statement that names `forbidden_table`, with a message;
//! - `before_save` blocks edits to the table `locked_items`;
//! - `after_query` and `on_connect` write a line in the app's log;
//! - anything else is allowed. A hook sees a masked copy of the statement and answers only allow
//!   or block: it cannot change what runs.
//!
//! So that a test can see what a hook gets and what becomes of a broken one, a statement naming
//! `show_hook_view` is blocked with its text as the message, one naming `hook_crash` makes the
//! module trap and one naming `hook_endless` makes it loop until its fuel ends. The app lets
//! these two through and tells the user once.
//!
//! Build: `cargo build --release --target wasm32-unknown-unknown`, then package `plugin.toml`
//! with `target/wasm32-unknown-unknown/release/query_guard.wasm` renamed to `plugin.wasm`.

use platypus_plugin_sdk::{entry, log};
use serde_json::{Value, json};

platypus_plugin_sdk::export_allocator!();

const INFORMATION: i32 = 2;

#[unsafe(no_mangle)]
pub extern "C" fn platypus_describe(ptr: i32, len: i32) -> i64 {
    entry(ptr, len, |_| Ok(json!({})))
}

#[unsafe(no_mangle)]
pub extern "C" fn platypus_hook(ptr: i32, len: i32) -> i64 {
    entry(ptr, len, |event| {
        Ok(match event["event"].as_str() {
            Some("before_query") => before_query(&event),
            Some("before_save") => before_save(&event),
            Some("after_query") => after_query(&event),
            Some("on_connect") => on_connect(&event),
            _ => allow(),
        })
    })
}

fn allow() -> Value {
    json!({ "action": "allow" })
}

fn block(message: &str) -> Value {
    json!({ "action": "block", "message": message })
}

fn before_query(event: &Value) -> Value {
    // A driver that does not speak SQL gives no text.
    let Some(text) = event["statement"]["text"].as_str() else {
        return allow();
    };
    let lower = text.to_lowercase();
    if lower.contains("hook_crash") {
        panic!("a crash for the test");
    }
    if lower.contains("hook_endless") {
        endless();
    }
    if lower.contains("show_hook_view") {
        return block(text);
    }
    if lower.contains("forbidden_table") {
        return block("forbidden_table is off limits.");
    }
    allow()
}

/// Spends the call's fuel: a side effect the compiler cannot remove.
fn endless() {
    let mut i = 0_u32;
    loop {
        i = std::hint::black_box(i.wrapping_add(1));
    }
}

fn before_save(event: &Value) -> Value {
    if event["table"] == "locked_items" {
        return block("locked_items cannot be edited.");
    }
    log(
        INFORMATION,
        &format!(
            "before_save {}: {} updates, {} inserts, {} deletes",
            event["table"], event["updates"], event["inserts"], event["deletes"]
        ),
    );
    allow()
}

fn after_query(event: &Value) -> Value {
    log(
        INFORMATION,
        &format!(
            "after_query {} {}: {}",
            event["status"], event["affected"], event["statement"]["text"]
        ),
    );
    allow()
}

fn on_connect(event: &Value) -> Value {
    log(
        INFORMATION,
        &format!(
            "on_connect {} ({})",
            event["connection"]["name"], event["connection"]["driver"]
        ),
    );
    allow()
}
