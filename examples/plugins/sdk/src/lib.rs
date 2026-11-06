//! The guest side of the PlatypusDB plugin ABI, versions 1 and 2 (see `docs/PLUGINS.md`), for
//! plugins written in Rust and built for `wasm32-unknown-unknown`. A module imports only the
//! host functions it uses: [`setting`] needs `api = 2`, the rest works under either.
//!
//! A plugin calls [`export_allocator!`] once and writes its entry points with [`entry`]:
//!
//! ```ignore
//! platypus_plugin_sdk::export_allocator!();
//!
//! #[unsafe(no_mangle)]
//! pub extern "C" fn platypus_describe(ptr: i32, len: i32) -> i64 {
//!     platypus_plugin_sdk::entry(ptr, len, |_| Ok(serde_json::json!({ "actions": [] })))
//! }
//! ```

use serde_json::{Value, json};

#[link(wasm_import_module = "platypus")]
unsafe extern "C" {
    #[link_name = "log"]
    fn host_log(level: i32, ptr: i32, len: i32);
    #[link_name = "http"]
    fn host_http(ptr: i32, len: i32) -> i32;
    #[link_name = "take"]
    fn host_take(ptr: i32);
    #[link_name = "setting"]
    fn host_setting(ptr: i32, len: i32) -> i32;
}

/// Exports `platypus_alloc` and `platypus_dealloc`, through Rust's allocator.
#[macro_export]
macro_rules! export_allocator {
    () => {
        #[unsafe(no_mangle)]
        pub extern "C" fn platypus_alloc(len: i32) -> i32 {
            $crate::alloc(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn platypus_dealloc(ptr: i32, len: i32) {
            $crate::dealloc(ptr, len);
        }
    };
}

/// A buffer of `len` bytes, owned by the host until it frees it with [`dealloc`].
#[doc(hidden)]
pub fn alloc(len: i32) -> i32 {
    let mut buf = Vec::<u8>::with_capacity(usize::try_from(len).unwrap_or(0));
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr as i32
}

/// Frees a buffer from [`alloc`], or an answer.
#[doc(hidden)]
pub fn dealloc(ptr: i32, len: i32) {
    if ptr != 0 {
        // SAFETY: the host gives back a buffer `alloc` or `answer` made, with its length,
        // which is also its capacity.
        unsafe {
            drop(Vec::from_raw_parts(
                ptr as *mut u8,
                0,
                usize::try_from(len).unwrap_or(0),
            ));
        }
    }
}

/// Reads the request at `ptr`, runs `f` on it and returns its answer as the host expects:
/// `{"ok": value}`, or `{"error": message}`.
pub fn entry(ptr: i32, len: i32, f: impl FnOnce(Value) -> Result<Value, String>) -> i64 {
    let request = if len == 0 {
        Value::Null
    } else {
        // SAFETY: the host wrote `len` bytes at `ptr`, a buffer from `alloc`, and keeps it
        // until this call returns.
        let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
        serde_json::from_slice(bytes).unwrap_or(Value::Null)
    };
    let answer = match f(request) {
        Ok(value) => json!({ "ok": value }),
        Err(message) => json!({ "error": message }),
    };
    pack(serde_json::to_vec(&answer).unwrap_or_default())
}

/// Hands `bytes` to the host, which frees them with `platypus_dealloc(ptr, len)`.
fn pack(bytes: Vec<u8>) -> i64 {
    // The capacity is the length, so that `dealloc` frees what was allocated.
    let mut bytes = bytes.into_boxed_slice();
    let (ptr, len) = (bytes.as_mut_ptr() as u32, bytes.len() as u32);
    std::mem::forget(bytes);
    (i64::from(ptr) << 32) | i64::from(len)
}

/// Writes a line to the app's log: `level` 0 error, 1 warning, 2 information.
pub fn log(level: i32, message: &str) {
    // SAFETY: the host only reads the bytes during the call.
    unsafe { host_log(level, message.as_ptr() as i32, message.len() as i32) }
}

/// Sends an HTTP request (`{"method", "url", "headers", "body"}`) and returns the answer
/// (`{"status", "headers", "body"}`), or why it failed or was not allowed.
pub fn http(request: &Value) -> Result<Value, String> {
    let bytes = serde_json::to_vec(request).map_err(|e| e.to_string())?;
    // SAFETY: the host reads the request during the call.
    let n = unsafe { host_http(bytes.as_ptr() as i32, bytes.len() as i32) };
    take(n, "HTTP failed")
}

/// One of the plugin's own settings: what the user saved, or the default of the manifest, or
/// `null`; a secret as its text. A key the manifest does not declare is an error (ABI 2).
pub fn setting(key: &str) -> Result<Value, String> {
    let bytes = serde_json::to_vec(&json!({ "key": key })).map_err(|e| e.to_string())?;
    // SAFETY: the host reads the request during the call.
    let n = unsafe { host_setting(bytes.as_ptr() as i32, bytes.len() as i32) };
    take(n, "the setting cannot be read")
}

/// Copies the answer of the last host call, `n` bytes, and reads its `ok` or `error`.
fn take(n: i32, failed: &str) -> Result<Value, String> {
    let mut buf = vec![0u8; usize::try_from(n).unwrap_or(0)];
    // SAFETY: the host writes exactly `n` bytes into the buffer.
    unsafe { host_take(buf.as_mut_ptr() as i32) };
    let mut answer: Value = serde_json::from_slice(&buf).map_err(|e| e.to_string())?;
    if let Some(error) = answer.get("error") {
        return Err(error.as_str().unwrap_or(failed).to_owned());
    }
    Ok(answer["ok"].take())
}
