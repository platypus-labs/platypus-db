//! The host side of the guest ABI (version [`API_VERSION`](crate::API_VERSION); the full
//! description is in `docs/PLUGINS.md`).
//!
//! Messages are UTF-8 JSON in the guest's linear memory. The host asks the guest for a
//! buffer with `platypus_alloc(len) -> ptr`, writes the request there and calls an entry
//! point `(ptr, len) -> i64`; the answer is at the pointer in the high 32 bits, its length
//! in the low ones. The host frees both buffers with `platypus_dealloc(ptr, len)` once the
//! call is over. Answers are `{"ok": value}` or `{"error": "message"}`.
//!
//! The guest imports from the `platypus` module only `log(level, ptr, len)`,
//! `http(ptr, len) -> len` and `take(ptr)`, which copies the answer of the last `http` into
//! a buffer of that length; nothing else is linked, so a module that imports anything else
//! (WASI, files, sockets) does not load.
//!
//! Each call gets [`Limits::fuel`] (about one unit per instruction: an endless loop stops
//! there), the memory never grows past [`Limits::memory`], and the host functions refuse
//! to work past [`Limits::deadline`]. A trap or any of these ends the call with an error and
//! leaves the sandbox [broken](Sandbox::is_broken): the next call needs a new one.
//!
//! Calls block: run them on a blocking thread (`tokio::task::spawn_blocking`), never on an
//! async worker, as the `http` function waits for its answer on the runtime.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasmi::{
    Caller, Config, Engine, Extern, Instance, Linker, Memory, Module, Store, StoreLimits,
    StoreLimitsBuilder, TrapCode, TypedFunc,
};

use crate::action::ActionInfo;
use crate::driver::DriverInfo;
use crate::http::Http;
use crate::{PluginError, log};

/// The module the host functions are imported from.
pub const HOST_MODULE: &str = "platypus";
/// `(len: i32) -> i32`: a buffer of `len` bytes in the guest's memory, 0 when it failed.
pub const ALLOC: &str = "platypus_alloc";
/// `(ptr: i32, len: i32)`: frees a buffer from `platypus_alloc` or an answer.
pub const DEALLOC: &str = "platypus_dealloc";
/// `(ptr, len) -> i64`: what the plugin provides, a [`Description`].
pub const DESCRIBE: &str = "platypus_describe";
/// `(ptr, len) -> i64`: runs one of its actions.
pub const ACTION: &str = "platypus_action";
/// `(ptr, len) -> i64`: one operation of its driver.
pub const DRIVER: &str = "platypus_driver";

/// What one call may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Fuel per call, about one unit per WebAssembly instruction.
    pub fuel: u64,
    /// Bytes the guest's memory may grow to.
    pub memory: usize,
    /// Bytes of a message in either direction, an HTTP answer included.
    pub message: usize,
    /// Wall time of a call, checked by the host functions: the time spent waiting on HTTP.
    pub deadline: Duration,
    /// Wall time of one HTTP request.
    pub http_timeout: Duration,
    /// Lines a call may write to the log; the rest are dropped.
    pub log_lines: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            // A few seconds of the interpreter on a laptop.
            fuel: 3_000_000_000,
            memory: 64 << 20,
            message: 16 << 20,
            deadline: Duration::from_secs(60),
            http_timeout: Duration::from_secs(30),
            log_lines: 100,
        }
    }
}

/// What a module provides, as its `platypus_describe` answers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Description {
    #[serde(default)]
    pub actions: Vec<ActionInfo>,
    #[serde(default)]
    pub driver: Option<DriverInfo>,
}

/// What a sandbox may reach outside itself.
#[derive(Debug, Clone, Default)]
pub struct Grants {
    /// The plugin's id, for the log.
    pub plugin: String,
    /// The hosts HTTP requests may go to (see [`crate::manifest::host_allowed`]).
    pub http: Vec<String>,
    /// An `http://` proxy for those requests; the system's otherwise.
    pub proxy: Option<String>,
}

/// One interpreter for every plugin, metering fuel.
fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = Config::default();
        config.consume_fuel(true);
        Engine::new(&config)
    })
}

/// Checks and compiles a module.
pub fn compile(wasm: &[u8]) -> Result<Module, PluginError> {
    Module::new(engine(), wasm)
        .map_err(|e| PluginError::Invalid(format!("the module is not valid WebAssembly: {e}")))
}

/// The guest's state seen by the host functions.
struct Host {
    limits: StoreLimits,
    plugin: String,
    /// The client, or why the plugin has no HTTP.
    http: Result<Http, String>,
    /// The answer of the last `http`, until `take` copies it.
    pending: Option<Vec<u8>>,
    logged: usize,
    max_log: usize,
    message: usize,
    deadline: Instant,
    /// A host function found the deadline passed.
    timed_out: bool,
}

/// An instantiated module and its store: its memory lasts from one call to the next.
pub struct Sandbox {
    store: Store<Host>,
    instance: Instance,
    memory: Memory,
    alloc: TypedFunc<i32, i32>,
    dealloc: TypedFunc<(i32, i32), ()>,
    limits: Limits,
    broken: bool,
}

impl std::fmt::Debug for Sandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sandbox")
            .field("plugin", &self.store.data().plugin)
            .field("broken", &self.broken)
            .finish_non_exhaustive()
    }
}

impl Sandbox {
    /// Instantiates `module` with `grants`; its start function, if any, runs on the fuel
    /// of a call.
    pub fn new(module: &Module, grants: &Grants, limits: Limits) -> Result<Self, PluginError> {
        let http = if grants.http.is_empty() {
            Err(format!(
                "the plugin {:?} has no permission for HTTP",
                grants.plugin
            ))
        } else {
            Http::new(grants, &limits).map_err(|e| e.to_string())
        };
        let host = Host {
            limits: StoreLimitsBuilder::new()
                .memory_size(limits.memory)
                .instances(1)
                .build(),
            plugin: grants.plugin.clone(),
            http,
            pending: None,
            logged: 0,
            max_log: limits.log_lines,
            message: limits.message,
            deadline: Instant::now() + limits.deadline,
            timed_out: false,
        };
        let mut store = Store::new(engine(), host);
        store.limiter(|host| &mut host.limits);
        store.set_fuel(limits.fuel).map_err(invalid)?;
        let linker = linker()?;
        let instance = linker
            .instantiate_and_start(&mut store, module)
            .map_err(|e| match e.as_trap_code() {
                Some(TrapCode::OutOfFuel) => PluginError::OutOfFuel,
                Some(_) => PluginError::Trap(e.to_string()),
                None => PluginError::Invalid(format!("the module cannot be loaded: {e}")),
            })?;
        let memory = instance
            .get_memory(&store, "memory")
            .ok_or_else(|| PluginError::Invalid("the module exports no `memory`".to_owned()))?;
        let alloc = instance
            .get_typed_func::<i32, i32>(&store, ALLOC)
            .map_err(|_| missing(ALLOC, "(i32) -> i32"))?;
        let dealloc = instance
            .get_typed_func::<(i32, i32), ()>(&store, DEALLOC)
            .map_err(|_| missing(DEALLOC, "(i32, i32)"))?;
        Ok(Self {
            store,
            instance,
            memory,
            alloc,
            dealloc,
            limits,
            broken: false,
        })
    }

    /// A call trapped or ran out of fuel or time: the guest's memory may be half updated,
    /// and the sandbox takes no more calls.
    #[must_use]
    pub fn is_broken(&self) -> bool {
        self.broken
    }

    /// The module exports `entry`.
    #[must_use]
    pub fn exports(&self, entry: &str) -> bool {
        self.instance.get_func(&self.store, entry).is_some()
    }

    /// Calls `entry` with `input` as JSON and reads its answer's `ok` value.
    pub fn call<I: Serialize + ?Sized, O: DeserializeOwned>(
        &mut self,
        entry: &str,
        input: &I,
    ) -> Result<O, PluginError> {
        let request = serde_json::to_vec(input)
            .map_err(|e| PluginError::Invalid(format!("cannot write the request: {e}")))?;
        let answer = self.call_raw(entry, &request)?;
        let answer: Value = serde_json::from_slice(&answer).map_err(|e| {
            PluginError::Invalid(format!(
                "`{entry}` answered something that is not JSON: {e}"
            ))
        })?;
        match answer {
            Value::Object(mut map) => {
                if let Some(error) = map.remove("error") {
                    return Err(PluginError::Guest(match error {
                        Value::String(s) => s,
                        other => other.to_string(),
                    }));
                }
                let ok = map.remove("ok").ok_or_else(|| {
                    PluginError::Invalid(format!("`{entry}` answered neither `ok` nor `error`"))
                })?;
                serde_json::from_value(ok).map_err(|e| {
                    PluginError::Invalid(format!("`{entry}` answered an unexpected value: {e}"))
                })
            }
            _ => Err(PluginError::Invalid(format!(
                "`{entry}` answered something that is not a JSON object"
            ))),
        }
    }

    /// Calls `entry` with the bytes of `input` and returns the bytes of its answer.
    pub fn call_raw(&mut self, entry: &str, input: &[u8]) -> Result<Vec<u8>, PluginError> {
        if self.broken {
            return Err(PluginError::Trap(
                "it failed earlier and must be started again".to_owned(),
            ));
        }
        if input.len() > self.limits.message {
            return Err(too_large(self.limits.message));
        }
        let func = self
            .instance
            .get_typed_func::<(i32, i32), i64>(&self.store, entry)
            .map_err(|_| missing(entry, "(i32, i32) -> i64"))?;
        self.store.set_fuel(self.limits.fuel).map_err(invalid)?;
        let host = self.store.data_mut();
        host.deadline = Instant::now() + self.limits.deadline;
        host.timed_out = false;
        host.logged = 0;
        host.pending = None;
        let result = self.run(func, input);
        if result.is_err() {
            self.broken = true;
        }
        result
    }

    fn run(
        &mut self,
        func: TypedFunc<(i32, i32), i64>,
        input: &[u8],
    ) -> Result<Vec<u8>, PluginError> {
        let len = i32::try_from(input.len()).map_err(|_| too_large(self.limits.message))?;
        let ptr = if input.is_empty() {
            0
        } else {
            let ptr = self
                .alloc
                .call(&mut self.store, len)
                .map_err(|e| self.failure(&e))?;
            if ptr == 0 {
                return Err(PluginError::Trap("`platypus_alloc` failed".to_owned()));
            }
            let at = self.checked(ptr, len)?;
            self.memory
                .write(&mut self.store, at, input)
                .map_err(|e| PluginError::Trap(e.to_string()))?;
            ptr
        };
        let packed = func
            .call(&mut self.store, (ptr, len))
            .map_err(|e| self.failure(&e))?;
        if ptr != 0 {
            self.dealloc
                .call(&mut self.store, (ptr, len))
                .map_err(|e| self.failure(&e))?;
        }
        let (out_ptr, out_len) = unpack(packed);
        if usize::try_from(out_len).unwrap_or(usize::MAX) > self.limits.message {
            return Err(too_large(self.limits.message));
        }
        let at = self.checked(out_ptr, out_len)?;
        let mut out = vec![0; usize::try_from(out_len).unwrap_or_default()];
        self.memory
            .read(&self.store, at, &mut out)
            .map_err(|e| PluginError::Trap(e.to_string()))?;
        if out_len > 0 {
            self.dealloc
                .call(&mut self.store, (out_ptr, out_len))
                .map_err(|e| self.failure(&e))?;
        }
        Ok(out)
    }

    /// The offset of `len` bytes at `ptr`, when they lie in the guest's memory.
    fn checked(&self, ptr: i32, len: i32) -> Result<usize, PluginError> {
        let (Ok(at), Ok(n)) = (usize::try_from(ptr), usize::try_from(len)) else {
            return Err(PluginError::Trap("a negative pointer or length".to_owned()));
        };
        if at
            .checked_add(n)
            .is_none_or(|end| end > self.memory.data_size(&self.store))
        {
            return Err(PluginError::Trap(
                "a buffer outside the plugin's memory".to_owned(),
            ));
        }
        Ok(at)
    }

    fn failure(&self, e: &wasmi::Error) -> PluginError {
        if self.store.data().timed_out {
            PluginError::Timeout
        } else if e.as_trap_code() == Some(TrapCode::OutOfFuel) {
            PluginError::OutOfFuel
        } else {
            PluginError::Trap(e.to_string())
        }
    }
}

/// `(ptr, len)` packed as the entry points return them.
fn unpack(packed: i64) -> (i32, i32) {
    let bits = packed.cast_unsigned();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the halves are 32-bit by design"
    )]
    let (ptr, len) = ((bits >> 32) as u32, bits as u32);
    (ptr.cast_signed(), len.cast_signed())
}

fn missing(name: &str, signature: &str) -> PluginError {
    PluginError::Invalid(format!("the module does not export `{name}` {signature}"))
}

fn too_large(limit: usize) -> PluginError {
    PluginError::Denied(format!("a message larger than {} MB", limit / (1 << 20)))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a `map_err` callback"
)]
fn invalid(e: wasmi::Error) -> PluginError {
    PluginError::Invalid(e.to_string())
}

/// The host functions every module may import.
fn linker() -> Result<Linker<Host>, PluginError> {
    let mut linker = Linker::new(engine());
    let link = |e: wasmi::errors::LinkerError| PluginError::Invalid(e.to_string());
    linker
        .func_wrap(HOST_MODULE, "log", host_log)
        .map_err(link)?
        .func_wrap(HOST_MODULE, "http", host_http)
        .map_err(link)?
        .func_wrap(HOST_MODULE, "take", host_take)
        .map_err(link)?;
    Ok(linker)
}

fn memory_of(caller: &Caller<'_, Host>) -> Result<Memory, wasmi::Error> {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| wasmi::Error::new("the module exports no `memory`"))
}

/// Reads `len` bytes at `ptr` of the guest's memory, at most `max`.
fn read_guest(
    caller: &Caller<'_, Host>,
    ptr: i32,
    len: i32,
    max: usize,
) -> Result<Vec<u8>, wasmi::Error> {
    let (Ok(at), Ok(n)) = (usize::try_from(ptr), usize::try_from(len)) else {
        return Err(wasmi::Error::new("a negative pointer or length"));
    };
    if n > max {
        return Err(wasmi::Error::new(format!(
            "a message larger than {} MB",
            max / (1 << 20)
        )));
    }
    let mut buf = vec![0; n];
    memory_of(caller)?
        .read(caller, at, &mut buf)
        .map_err(|_| wasmi::Error::new("a buffer outside the plugin's memory"))?;
    Ok(buf)
}

/// `log(level, ptr, len)`: 0 error, 1 warning, anything else information.
fn host_log(
    mut caller: Caller<'_, Host>,
    level: i32,
    ptr: i32,
    len: i32,
) -> Result<(), wasmi::Error> {
    const LINE: usize = 4096;
    let host = caller.data();
    if host.logged >= host.max_log {
        return Ok(());
    }
    let n = len.min(i32::try_from(LINE).unwrap_or(i32::MAX));
    let text = read_guest(&caller, ptr, n, LINE)?;
    let host = caller.data_mut();
    host.logged += 1;
    let level = match level {
        0 => "ERROR",
        1 => "WARN",
        _ => "INFO",
    };
    log(
        level,
        &format!("plugin {}: {}", host.plugin, String::from_utf8_lossy(&text)),
    );
    Ok(())
}

/// `http(ptr, len) -> len`: runs the request and keeps its answer for `take`.
fn host_http(mut caller: Caller<'_, Host>, ptr: i32, len: i32) -> Result<i32, wasmi::Error> {
    let host = caller.data();
    if Instant::now() >= host.deadline {
        caller.data_mut().timed_out = true;
        return Err(wasmi::Error::new("deadline"));
    }
    let request = read_guest(&caller, ptr, len, host.message)?;
    let host = caller.data();
    let answer = match &host.http {
        Ok(http) => http.request(&request, host.deadline, host.message),
        Err(why) => serde_json::json!({ "error": format!("not allowed: {why}") }),
    };
    let bytes = serde_json::to_vec(&answer).map_err(|e| wasmi::Error::new(e.to_string()))?;
    let n = i32::try_from(bytes.len()).map_err(|_| wasmi::Error::new("answer too large"))?;
    caller.data_mut().pending = Some(bytes);
    Ok(n)
}

/// `take(ptr)`: copies the answer of the last `http` to `ptr`, a buffer of its length.
fn host_take(mut caller: Caller<'_, Host>, ptr: i32) -> Result<(), wasmi::Error> {
    let bytes = caller
        .data_mut()
        .pending
        .take()
        .ok_or_else(|| wasmi::Error::new("`take` without an answer to take"))?;
    let at = usize::try_from(ptr).map_err(|_| wasmi::Error::new("a negative pointer"))?;
    memory_of(&caller)?
        .write(&mut caller, at, &bytes)
        .map_err(|_| wasmi::Error::new("a buffer outside the plugin's memory"))
}

#[cfg(test)]
mod tests;
