//! An example of a PlatypusDB driver plugin, for a database that answers over HTTP:
//!
//! - `GET /hello` → `{"product", "version"}`: checks the token, sent as
//!   `Authorization: Bearer <password>`;
//! - `POST /query` with `{"sql"}` → `{"columns": [{"name", "type"}], "rows": [[...]]}`, or
//!   `{"affected": n}`; an error is any status from 400 with `{"error": message}`;
//! - `GET /tables` → `["name", ...]`, and `GET /tables/<name>` →
//!   `{"columns": [{"name", "type", "nullable", "primary_key"}]}`, for the navigator.
//!
//! The connection's host and port make the base URL, `https` unless SSL is disabled, with
//! the database, if any, as its path. The session (base URL and token) stays in the
//! module's memory between calls.

use std::cell::RefCell;

use platypus_plugin_sdk::{entry, http};
use serde_json::{Value, json};

platypus_plugin_sdk::export_allocator!();

struct Session {
    base: String,
    token: Option<String>,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

#[unsafe(no_mangle)]
pub extern "C" fn platypus_describe(ptr: i32, len: i32) -> i64 {
    entry(ptr, len, |_| {
        Ok(json!({ "driver": { "name": "Toy HTTP database", "metadata": true } }))
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn platypus_driver(ptr: i32, len: i32) -> i64 {
    entry(ptr, len, |request| match request["op"].as_str() {
        Some("connect") => connect(&request),
        Some("execute") => call("POST", "/query", Some(json!({ "sql": request["sql"] }))),
        Some("schemas") => Ok(json!([{ "name": "main", "current": true }])),
        Some("objects") => {
            let tables = call("GET", "/tables", None)?;
            let names = tables.as_array().cloned().unwrap_or_default();
            Ok(names
                .into_iter()
                .map(|name| json!({ "name": name, "kind": "table" }))
                .collect())
        }
        Some("table") => {
            let name = request["table"].as_str().unwrap_or_default();
            call("GET", &format!("/tables/{}", encode(name)), None)
        }
        Some("close") => {
            SESSION.with(|s| s.borrow_mut().take());
            Ok(Value::Null)
        }
        other => Err(format!("unsupported operation {other:?}")),
    })
}

fn connect(request: &Value) -> Result<Value, String> {
    let config = &request["config"];
    let host = config["host"]
        .as_str()
        .filter(|h| !h.is_empty())
        .ok_or("no host given")?;
    let scheme = if config["ssl_mode"] == "disable" {
        "http"
    } else {
        "https"
    };
    let mut base = match config["port"].as_u64() {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    };
    if let Some(db) = config["database"].as_str().filter(|d| !d.is_empty()) {
        base.push('/');
        base.push_str(&encode(db));
    }
    let token = request["secrets"]["password"].as_str().map(str::to_owned);
    SESSION.with(|s| *s.borrow_mut() = Some(Session { base, token }));
    let hello = call("GET", "/hello", None);
    if hello.is_err() {
        SESSION.with(|s| s.borrow_mut().take());
    }
    hello
}

/// Calls the API of the open session and reads its JSON answer.
fn call(method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let (url, token) = SESSION.with(|s| {
        s.borrow()
            .as_ref()
            .map(|s| (format!("{}{path}", s.base), s.token.clone()))
            .ok_or_else(|| "not connected".to_owned())
    })?;
    let mut headers = json!({ "Accept": "application/json" });
    if let Some(token) = token.filter(|t| !t.is_empty()) {
        headers["Authorization"] = json!(format!("Bearer {token}"));
    }
    let mut request = json!({ "method": method, "url": url, "headers": headers });
    if let Some(body) = body {
        request["headers"]["Content-Type"] = json!("application/json");
        request["body"] = json!(body.to_string());
    }
    let response = http(&request)?;
    let status = response["status"].as_u64().unwrap_or(0);
    let text = response["body"].as_str().unwrap_or_default();
    let answer: Value = serde_json::from_str(text).map_err(|_| format!("HTTP {status}: {text}"))?;
    if status >= 400 {
        let message = answer["error"]
            .as_str()
            .map_or_else(|| format!("HTTP {status}: {text}"), str::to_owned);
        return Err(message);
    }
    Ok(answer)
}

/// `s` with what is not a letter, a digit or `-_.~` percent-encoded, for a path segment.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
