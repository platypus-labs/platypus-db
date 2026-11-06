//! An example of PlatypusDB grid extensions. `platypus_grid` takes batches, never a cell at a time:
//!
//! - `column`: the column `length` is how many characters a row's text columns hold together; a
//!   row whose first value is `boom` gets an error of its own, `crash` makes the module trap and
//!   `endless` makes it loop until its fuel ends, so that a test can see a column fail alone;
//! - `render`: the renderer `status` colours `ok` and `failed`, and leaves other values as they
//!   are. All it can say is text and colours: there is no field for markup;
//! - `action`: the command `shout` shows the cell's value in capitals.
//!
//! A masked cell comes as dots, which is all this module can know of it.
//!
//! Build: `cargo build --release --target wasm32-unknown-unknown`, then package `plugin.toml` with
//! `target/wasm32-unknown-unknown/release/grid_demo.wasm` renamed to `plugin.wasm`.

use std::hint::black_box;

use platypus_plugin_sdk::entry;
use serde_json::{Value, json};

platypus_plugin_sdk::export_allocator!();

#[unsafe(no_mangle)]
pub extern "C" fn platypus_describe(ptr: i32, len: i32) -> i64 {
    entry(ptr, len, |_| Ok(json!({})))
}

#[unsafe(no_mangle)]
pub extern "C" fn platypus_grid(ptr: i32, len: i32) -> i64 {
    entry(ptr, len, |request| match request["op"].as_str() {
        Some("column") => Ok(json!({ "values": column(&request) })),
        Some("render") => Ok(json!({ "cells": render(&request) })),
        Some("action") => Ok(action(&request)),
        other => Err(format!("no such operation: {other:?}")),
    })
}

fn column(request: &Value) -> Vec<Value> {
    let rows = request["rows"].as_array().map_or(&[][..], Vec::as_slice);
    rows.iter().map(row_length).collect()
}

fn row_length(row: &Value) -> Value {
    let values = row.as_array().map_or(&[][..], Vec::as_slice);
    match values.first().and_then(Value::as_str) {
        Some("boom") => return json!({ "error": "this row cannot be counted" }),
        Some("crash") => panic!("a crash for the test"),
        Some("endless") => endless(),
        _ => {}
    }
    let total: usize = values
        .iter()
        .filter_map(Value::as_str)
        .map(|text| text.chars().count())
        .sum();
    json!(total)
}

fn render(request: &Value) -> Vec<Value> {
    let values = request["values"].as_array().map_or(&[][..], Vec::as_slice);
    values
        .iter()
        .map(|value| match value.as_str() {
            Some("ok") => json!({ "color": "#1a7f37", "hint": "fine" }),
            Some("failed") => json!({ "text": "FAILED", "background": "#ffd7d5" }),
            Some("markup") => json!({ "html": "<img src=x onerror=alert(1)>" }),
            _ => Value::Null,
        })
        .collect()
}

fn action(request: &Value) -> Value {
    let text = request["value"].as_str().unwrap_or("NULL").to_uppercase();
    json!({ "kind": "message", "text": text })
}

fn endless() -> ! {
    let mut i = 0_u64;
    loop {
        i = black_box(i.wrapping_add(1));
    }
}
