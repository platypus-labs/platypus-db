# Plugins

PlatypusDB runs plugins written in any language that compiles to WebAssembly (Rust for
`wasm32-unknown-unknown`, AssemblyScript, TinyGo, C, Zig…). A plugin can add **actions**
(commands in the editor, the result grid and the navigator), a **driver** for a database that
speaks HTTP, **event hooks** and a **settings page**. Plugins run in a sandbox: an interpreter
(`wasmi`) with no access to files, programs, sockets or the clock, only to the JSON messages the
app gives them and the host functions their permissions open.

The host side is the crate `platypus-plugins`; the examples are in
[`examples/plugins`](../examples/plugins) with their built modules.

## Package

A package is a folder, or a `.zip` with the files at its root or in one folder:

```
plugin.toml    the manifest
plugin.wasm    the module
```

```toml
id = "sql-tools"                # lowercase letters, digits, - _ . ; stable
name = "SQL tools"
version = "1.0.0"
author = "Jane Doe"             # optional
description = "What it does."   # optional
api = 1                         # the ABI version below
kinds = ["actions"]             # "actions" and/or "driver"; ABI 2 adds more, below

[permissions]                   # optional; nothing is allowed by default
http = ["api.example.com", "*.example.org"]   # hosts for the http function
clipboard = true                # an action may put text on the clipboard
```

Settings › Plugins installs a package from a folder or a `.zip`. Before installing, the app
loads the module, asks it what it provides (`platypus_describe`) and shows that with the
permissions, in plain words; installing approves them. A module whose description does not match
`kinds`, or that does not load, is refused, and so is a manifest with a field, a permission or a
kind the app does not know, by name. Packages are kept in `<data>/plugins/<id>/<version>/`; a new
version replaces the old one after the same review, which lists apart what it asks that the
installed version was not approved for. `<data>/plugins/plugins.json` records which plugins are
on, the approved permissions and the description. A plugin turned off offers no action and no
driver and runs no hook; removing it deletes its folder and its settings.

### What ABI 2 declares

`api = 2` adds to the manifest; a module of ABI 1 loads and behaves as it did, and a manifest of
ABI 1 that uses any of this is refused with what needs `api = 2`.

```toml
api = 2
kinds = ["actions", "hooks", "tabs", "panels", "grid", "theme"]
hooks = ["before_query", "after_query", "on_connect", "before_save"]

[permissions]
hooks = true                    # needed by `hooks`
tabs = true                     # needed by `tabs` and `panels`
grid = true                     # needed by `grid`

[[tabs]]                        # a tab kind: an HTML document of the package
kind = "notes"
title = "Notes"
entry = "ui/notes.html"

[[panels]]                      # a side panel, the same way
id = "outline"
title = "Outline"
entry = "ui/outline.html"

[[grid.columns]]                # also [[grid.renderers]] and [[grid.actions]]
id = "length"
title = "Length"
types = ["text"]                # optional; all types without it

[theme]                         # colours by the names of a theme file, and fonts
name = "Sea"
dark = { bg = "#0b1d26", accent = "#3fc1c9" }
fonts = { mono = "Fira Code" }

[[settings]]                    # the plugin's settings page, below
key = "endpoint"
label = "Endpoint"
type = "text"
```

A kind and what it declares go together, and each needs its permission: declaring `hooks` without
`permissions.hooks`, or asking for a permission that nothing declared uses, is refused. A package
that is only a `theme` has no module: nothing of it runs, and a `plugin.wasm` beside its manifest
is neither read nor copied. Everything else needs one.

Tabs, panels, grid extensions and themes are declared and checked here, and shown at install; what
the app does with them comes with those features.

## ABI, versions 1 and 2

Version 2 adds an entry point (`platypus_hook`) and a host function (`setting`) to version 1. The
rest is the same, and a module of version 1 cannot import `setting`: it would not load.

Messages are UTF-8 JSON in the module's linear memory. The module exports:

| Export | Signature | |
|---|---|---|
| `memory` | memory | its linear memory |
| `platypus_alloc` | `(len: i32) -> i32` | a buffer of `len` bytes; 0 on failure |
| `platypus_dealloc` | `(ptr: i32, len: i32)` | frees a buffer from `platypus_alloc` or an answer |
| `platypus_describe` | `(ptr: i32, len: i32) -> i64` | what it provides |
| `platypus_action` | `(ptr: i32, len: i32) -> i64` | runs an action (kind `actions`) |
| `platypus_driver` | `(ptr: i32, len: i32) -> i64` | one driver operation (kind `driver`) |
| `platypus_hook` | `(ptr: i32, len: i32) -> i64` | one event (kind `hooks`, ABI 2) |

A call: the host allocates a buffer with `platypus_alloc`, writes the request, calls the entry
point with its pointer and length (0, 0 for an empty request), and reads the answer at the
pointer in the high 32 bits of the result, of the length in the low 32 bits. Then the host
frees both the request and the answer with `platypus_dealloc`: the module must not keep
either. Every answer is an object:

```json
{"ok": <value>}            or            {"error": "a message for the user"}
```

The module may import only these functions, from the module `platypus`:

| Import | Signature | |
|---|---|---|
| `log` | `(level: i32, ptr: i32, len: i32)` | a line in the app's log: 0 error, 1 warning, 2 information |
| `http` | `(ptr: i32, len: i32) -> i32` | sends a request; returns the length of the answer |
| `take` | `(ptr: i32)` | copies the answer of the last `http` or `setting` into a buffer of that length |
| `setting` | `(ptr: i32, len: i32) -> i32` | reads one of its own settings; returns the length of the answer (ABI 2) |

A module that imports anything else (WASI, for example) does not load: build Rust plugins for
`wasm32-unknown-unknown`, not `wasm32-wasip1`.

`http` takes `{"method": "GET", "url": "https://…", "headers": {…}, "body": "text"}` (only
`url` is required) and answers `{"ok": {"status": 200, "headers": {…}, "body": "text"}}` or
`{"error": "…"}`. It goes only to the hosts of the approved permissions (and, for a driver,
the host of its connection); any other host, a scheme other than `http`/`https`, or a plugin
without the permission gets an error, and the call goes on. Redirects are followed only to
allowed hosts. Requests go through the app's network stack: verified TLS, the system's proxy
(`HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY`, and the system settings on Windows and macOS), or
the connection's `http://` proxy for a driver. Bodies are text (invalid UTF-8 is replaced).

### Limits

Each call has:

- **fuel**: about 3 billion units, roughly one per WebAssembly instruction (a few seconds of
  the interpreter). An endless loop stops there with "the plugin used up its time for this
  call";
- **memory**: the module's memory may not grow past 64 MB; a module that asks for more at
  start does not load, and a growth past it fails (`memory.grow` returns -1);
- **messages**: 16 MB each way, an HTTP answer included;
- **time**: 60 s for the call, checked by the host functions; one HTTP request at most 30 s;
- **log**: 100 lines, 4 KB each.

A trap (`unreachable`, an access outside memory, a failed allocation), running out of fuel or
time, or an answer that points outside memory ends the call with an error and never takes the
app down. After that the module's state may be inconsistent: an action starts from a fresh
instance anyway, and a driver connection is closed.

### Describe

`platypus_describe` gets an empty request and answers:

```json
{"ok": {
  "actions": [
    {"id": "uppercase", "title": "Uppercase SQL keywords", "scope": "editor", "shortcut": "Mod+Alt+U"},
    {"id": "markdown", "title": "Copy rows as Markdown table", "scope": "grid"},
    {"id": "count", "title": "Count rows", "scope": "navigator.object"}
  ],
  "driver": {"name": "Toy HTTP database", "metadata": true}
}}
```

Titles are shown as written (plugins are not translated). Shortcuts use the app's notation
(`Mod` is Ctrl, or Cmd on macOS) and can be changed in Settings › Keyboard shortcuts.

### Actions

Actions appear under a **Plugins** submenu of the context menu of their scope, and run with
their shortcut where that scope has the focus. Each call to `platypus_action` starts a new instance of the
module, so actions keep nothing between calls. The request is
`{"action": id, "scope": scope, "context": {…}}`, with the context of the scope:

- `editor`: `{"text": "…", "selection": {"from", "to", "text"} | null, "driver": "postgres"}`;
- `grid`: `{"columns": [{"name", "type"}], "rows": [[…], …], "truncated": false}`: the
  selected rows and columns, values as "Copy as JSON" writes them (numbers and booleans bare,
  the rest as text, `null`), at most 1000 rows (`truncated` says there were more);
- `navigator.object`: `{"connection": {"name", "driver", "host", "port", "database", "user"},
  "database", "schema", "object": {"name", "kind"}, "target": "\"main\".\"items\""}`: never a
  password; `target` is the quoted, qualified name.

The answer is one of:

```json
{"kind": "replace", "text": "…"}   the editor's selection, or its whole text without one
{"kind": "new_tab", "text": "…"}   a new SQL tab on the same connection
{"kind": "copy", "text": "…"}      the clipboard: only with the clipboard permission
{"kind": "message", "text": "…"}   a message
{"kind": "none"}
```

### Drivers

A connection whose driver is a plugin (`driver = "plugin"`, the plugin's id in the `plugin`
option) has its own instance of the module for as long as it is open, so the module can keep
its session in memory. Calls on one connection run one at a time. `platypus_driver` receives:

| Request | Answer |
|---|---|
| `{"op": "connect", "config": {"host", "port", "database", "user", "ssl_mode", "options"}, "secrets": {"password"}}` | `{"product", "version"}` (both optional) |
| `{"op": "execute", "sql": "…"}` | `{"columns": [{"name", "type"}], "rows": [[…]]}`, or `{"affected": n}` |
| `{"op": "schemas"}` | `[{"name", "current"}]` |
| `{"op": "objects", "schema": "…"}` | `[{"name", "kind", "comment"}]`, `kind` as the app names them (`table`, `view`, …; `table` by default) |
| `{"op": "table", "schema": "…", "table": "…"}` | `{"columns": [{"name", "type", "nullable", "primary_key", "comment"}]}` |
| `{"op": "close"}` | anything; sent when the connection is dropped |

The password is only in the `connect` request, in memory; the `options` sent leave out the
app's own (`plugin`, `proxy`, the connect and disconnect commands). The navigator calls
`schemas`, `objects` and `table` only when the description says `"metadata": true`; without
it the navigator stays empty and the editor still runs statements.

The host turns rows into Arrow batches. A column takes its declared `type` when every value
fits it (`bool`/`boolean`, `int`/`integer`/`bigint`, `float`/`double`/`real`/`number`/
`numeric`/`decimal`, `text`/`string`/`varchar`), otherwise the narrowest type all its values
fit (boolean, 64-bit integer, double), otherwise text, where objects and arrays are written as
JSON. The declared type is shown as the column's type.

A driver reaches its database **only through `http`**: the host of the connection, as the user
typed it, is allowed without a permission; other hosts (a login service) must be listed in
`permissions.http`. Raw TCP, UDP and local files are not possible, so databases with an HTTP
API only. Plugin drivers have no transactions (autocommit only), no cancel (the call's fuel
and deadline stop it), no SSH tunnel (the tunnel's local address is not passed to the
plugin) and no terminal mode. They use the standard SQL dialect in the editor.

### Hooks

With `kinds = ["hooks"]`, `hooks = [...]` and `permissions.hooks = true`, `platypus_hook` runs on
those events and answers `{"action": "allow"}` or `{"action": "block", "message": "why"}`. Each
call starts a new instance of the module and keeps nothing.

| Event | When | Besides `connection` it gets |
|---|---|---|
| `before_query` | a statement passed the app's checks and is about to run; it may block | `statement` |
| `after_query` | that statement ended; the answer does not matter, and nothing waits for it | `statement`, `status` (`ok`, `error`, `cancelled`), `elapsed_ms`, `affected` |
| `on_connect` | a connection opened; nothing waits for it | nothing |
| `before_save` | edited rows are about to be saved; it may block | `schema`, `table`, `updates`, `inserts`, `deletes` |

```json
{
  "event": "before_query",
  "connection": {"name": "Shop", "driver": "postgres", "database": "shop", "production": false},
  "statement": {"text": "SELECT * FROM users WHERE email = '••••••'", "truncated": false}
}
```

`statement.text` is a copy, with the passwords the query log hides and the literals of sensitive
columns masked, by the rules and marks that mask them for agents (Settings › Safety › Masking).
It is at most 64 KB (`truncated` says when it is cut), and `null` on MongoDB, Redis and
Elasticsearch, whose literals cannot be told. A hook never gets a password, a value of a result or
the text of an error, which may quote one, and a save gets only how many rows change, as its
statements quote their values.

The statement that runs is the app's: the answer has no field for another one, and an answer with
any other field is a broken hook. The hook comes after the Safety checks (the question before a
`DELETE` without `WHERE` or a change on a production connection, a read-only connection's refusal),
so it cannot lift them, and a statement they refuse never reaches it: a hook can only add a refusal.
That refusal is the statement's error, and names the plugin.

A hook is bound: 300 million units of fuel, a tenth of an action's, the same 64 MB of memory, 1 MB
per message, 20 log lines, and 2 s of wall time, after which nothing waits for it. **A hook that
crashes, runs out of fuel or time, or answers something else does not stop the action**: the
statement runs, and the user is told once per plugin until it is changed or turned off and on. So
a hook is a help against mistakes, not a wall. The database's own permissions are the wall.

Hooks run for what the user runs in the app: the SQL editor, notebooks, scripts, the dialogs that
run a statement, and saves of edited rows; `EXPLAIN ANALYZE` and a database's own client go through
`before_query` only. Not for agents, which have their own policy (see [AGENTS.md](AGENTS.md)), and not for `.sql` files run from
the file dialog, the table designer, exports or scheduled jobs.

A user with no hook pays one atomic read per statement, with nothing to wait for and nothing
allocated: the table of hooks comes from the list of plugins the page reads at start anyway, and
no module is read or compiled until its first call.

### Settings

`[[settings]]` in the manifest draws a page beside the plugin in Settings › Plugins, in the
plugin's own language, loaded when it opens.

| Field | |
|---|---|
| `key` | lowercase letters, digits, `-` and `_`; unique |
| `label`, `description` | as shown; only `label` is required |
| `type` | `text`, `number`, `boolean`, `choice` or `secret` |
| `default` | until the user sets a value; not for a `secret` |
| `options` | for a `choice`: `[{ value = "x", label = "X" }]` |
| `min`, `max` | for a `number` |

Without a `default` the control starts empty (no text, the lowest number, off, the first choice)
and the module reads `null`. Values go in `<data>/plugins/settings.toml`. A `secret` goes in the
system's keychain, never in a file: without a keychain it cannot be saved, and the page says so.
The page never reads a secret back, only whether it is set. The review before installing lists
the fields, and those that go to the keychain.

The module reads them with `setting`, whose request is `{"key": "endpoint"}` and whose answer,
through `take`, is `{"ok": value}` (what the user saved, or the default, or `null`; a secret as
its text) or `{"error": "…"}`. Only the keys of its own manifest can be named, so another plugin's
values are out of reach; any other key gets an error and the call goes on. Removing a plugin
forgets its values, and an update forgets those of the fields it dropped.

## Writing a plugin in Rust

`examples/plugins/sdk` is a small guest library: `export_allocator!()` exports
`platypus_alloc` and `platypus_dealloc`, `entry(ptr, len, |request| …)` reads a request and
writes the answer, `http`, `setting` and `log` wrap the host functions.

```rust
use platypus_plugin_sdk::entry;
use serde_json::json;

platypus_plugin_sdk::export_allocator!();

#[unsafe(no_mangle)]
pub extern "C" fn platypus_describe(ptr: i32, len: i32) -> i64 {
    entry(ptr, len, |_| Ok(json!({ "actions": [
        { "id": "hello", "title": "Say hello", "scope": "editor" }
    ] })))
}

#[unsafe(no_mangle)]
pub extern "C" fn platypus_action(ptr: i32, len: i32) -> i64 {
    entry(ptr, len, |_| Ok(json!({ "kind": "message", "text": "Hello!" })))
}
```

```bash
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/<name>.wasm plugin.wasm
```

The examples build with `opt-level = "z"`, LTO and `panic = "abort"`: about 90 KB each.

- `examples/plugins/sql-tools`: an action per scope (uppercase keywords in the editor, copy rows
  as a Markdown table, count a table's rows in a new tab).
- `examples/plugins/toy-http-driver`: a driver for a database answering `GET /hello`,
  `POST /query`, `GET /tables` and `GET /tables/<name>` with JSON, the password sent as a bearer
  token. The tests of `platypus-plugins` (`tests/examples.rs`) run it against a local server.
- `examples/plugins/query-guard`: hooks that block statements naming a table and edits of another,
  and log what ran. `tests/hooks.rs` runs it, and also without its permission.
- `examples/plugins/settings-demo`: a settings page with a field of each type, and actions that
  read them, and try a key the manifest does not declare.

After changing an example, build it again and copy its module next to its `plugin.toml`: the
tests use the committed `plugin.wasm`.

Other languages export the same functions: `memory`, an allocator under the names
`platypus_alloc` and `platypus_dealloc` (for actions a bump allocator is enough, since each
runs in a new instance; a driver keeps its instance and needs a real one), and the entry points; and import `log`, `http` and `take` from the
module `platypus` (in AssemblyScript, `@external("platypus", "http")`).
