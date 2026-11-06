# Plugins

PlatypusDB runs plugins written in any language that compiles to WebAssembly (Rust for
`wasm32-unknown-unknown`, AssemblyScript, TinyGo, C, Zig…). A plugin can add **actions**
(commands in the editor, the result grid and the navigator), a **driver** for a database that
speaks HTTP, **event hooks**, a **settings page**, **tabs and side panels** with pages of their own,
**extensions of the result grid** and a **theme**, which is the one kind with no code at all. Plugins
run in a sandbox: an interpreter
(`wasmi`) with no access to files, programs, sockets or the clock, only to the JSON messages the
app gives them and the host functions their permissions open.

The host side is the crate `platypus-plugins`; the examples are in
[`examples/plugins`](../examples/plugins) with their built modules.

## Package

A package is a folder, or a `.zip` with the files at its root or in one folder:

```
plugin.toml    the manifest
plugin.wasm    the module
ui/            what the pages of its tabs and panels load, if it has any
```

```toml
id = "sql-tools"                # lowercase letters, digits, - _ . ; stable; a folder name, see below
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
installed version was not approved for, and it stays turned on or off as the old one was.
`<data>/plugins/plugins.json` records which plugins are on, the approved permissions and the
description. A plugin turned off offers no action and no driver and runs no hook. Removing it deletes
its folder and its secrets in the keychain; a plugin with other settings asks whether to keep their
values for a later install, and otherwise they go too. If something of it cannot be cleared, the rest
is, it leaves the list all the same, and the error says what is left. Packages can also come from a
[catalog](#catalog).

The `id` and the `version` are folder names, and mean the same on every system: besides the characters
above, a name that ends in a dot, a device name of Windows in any case and with any extension (`CON`,
`PRN`, `AUX`, `NUL`, `COM0`-`COM9`, `LPT0`-`LPT9`, so `nul.txt` too), and `plugins.json` or
`settings.toml` are refused. The manifest in a plugin's folder must say the id and version
`plugins.json` has for it, or it does not run with what was approved. A plugin whose files are gone
or are not its own stays listed, marked as such, runs nothing, and can be removed.

If `plugins.json` cannot be read, no plugin runs and nothing overwrites the file: installing and
removing are refused. Statements still run, as a broken file must not stop your work, so a hook that
guards them is not guarding them: the first statement says so, Settings › Plugins says so with the
reason, and the log has it. Put the file right, or delete it to start with no plugins, and hooks count
again from the next statement.

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

[theme]                         # colours by the names of a theme file, and fonts of the package
name = "Sea"
dark = { bg = "#0b1d26", accent = "#3fc1c9" }
fonts = { mono = "fonts/Sea-Mono.woff2" }

[[settings]]                    # the plugin's settings page, below
key = "endpoint"
label = "Endpoint"
type = "text"
```

A kind and what it declares go together, and each needs its permission: declaring `hooks` without
`permissions.hooks`, or asking for a permission that nothing declared uses, is refused. A package
that is only a `theme` has no module: nothing of it runs, and a `plugin.wasm` beside its manifest
is neither read nor copied. Everything else needs one.

Tabs and panels are below in [Tabs and side panels](#tabs-and-side-panels), grid extensions in
[Grid extensions](#grid-extensions).

### Themes

A package with `kinds = ["theme"]` and no module shows up as a card in Settings › Appearance › Theme.
`examples/plugins/theme-sea` is one, with both variants:

```toml
id = "theme-sea"
name = "Sea theme"
version = "1.0.0"
api = 2
kinds = ["theme"]

[theme]
name = "Sea"                    # as the picker shows it

[theme.dark]                    # and/or [theme.light]
bg = "#0b1d26"
accent = "#3fc1c9"
```

With both variants the theme follows the system's light or dark mode. A colour it leaves out comes
from the Platypus theme of the same lightness, as in a theme file.

**Colours.** A name must be one of the app's tokens (`PALETTE_KEYS` in
`apps/desktop/src/theme.ts`, the same names as a theme file), and a value must be `#rgb`, `#rgba`,
`#rrggbb`, `#rrggbbaa`, `rgb()`, `rgba()`, `hsl()` or `hsla()` with numbers only, or `transparent`.
Anything else refuses the package by name at install: `url(...)`, `expression(...)`, `var(...)`,
`color-mix(...)`, a named colour, a `;` or a `}`. The page checks each value again and sets it with
`style.setProperty`; nothing of a theme is written as CSS text.

**Fonts.** `fonts` maps `ui` or `mono` to a path inside the package, of a `.woff2`, `.woff`, `.ttf`
or `.otf` file of at most 4 MB, which must really be a font by its first bytes. A family name or an
address is refused: a font comes only from the package. The app gives it a family name of its own.

**Legibility.** The review and the plugin's row warn when text would be hard to read, with the pairs
and their contrast (`fg / bg`, `fg-muted / bg`, `accent-fg / accent`…); it is a warning, not a
refusal.

A theme costs nothing unless it is chosen: its colours are in the list of plugins the page reads
anyway, and its fonts are read when it is the theme in use. When its plugin is turned off or removed,
or its files are gone, the app uses the default theme, and comes back to it if the plugin returns.

## Catalog

Settings › Plugin catalog lists the plugins of an index and installs, updates or reinstalls them. The
address of the index is a setting (`plugins.catalog_url`). It starts empty and there is no built-in
one: until you set it the app contacts nothing. The index is read when the page opens or when you
ask, never in the background. Nothing installs without your click, and an install goes through the
same review as a manual one, with the permissions in plain words and what an update asks anew.

### Building an index

An index is JSON, on any HTTPS server:

```json
{
  "format": 1,
  "plugins": [
    {
      "id": "theme-sea",
      "name": "Sea theme",
      "version": "1.0.0",
      "author": "PlatypusDB",
      "description": "A theme with no code.",
      "url": "theme-sea-1.0.0.zip",
      "sha256": "<the SHA-256 of the .zip, in hexadecimal>",
      "signature": "<the text of its .minisig file>"
    }
  ]
}
```

`id`, `name`, `version`, `url` and `sha256` are required, one entry per plugin. `url` may be relative
to the index. An entry that cannot be read is listed apart with its reason, and the others stay
usable. Fields it does not know are ignored. [`examples/plugins/catalog`](../examples/plugins/catalog)
has a complete one.

### Signing a package

A package is signed with [minisign](https://jedisct1.github.io/minisign/), by a detached signature
over the bytes of the `.zip`. The signature goes in the entry, so the package itself does not change:

```bash
minisign -G -p publisher.pub -s publisher.key          # once: keep the secret key
zip -r theme-sea-1.0.0.zip plugin.toml                 # the package, as for a manual install
minisign -S -s publisher.key -m theme-sea-1.0.0.zip    # writes theme-sea-1.0.0.zip.minisig
sha256sum theme-sea-1.0.0.zip                          # the entry's sha256
```

Publish the `.pub` file where your users can check it is yours. They paste it under **Keys you
trust**, with a name: the app has no keys of its own, and trusts none until they do. Signatures of the
legacy format (`minisign -S -l`) are refused.

### What is checked

Each check refuses with its own reason, and leaves nothing behind.

| Check | Refused when |
|---|---|
| Address | The index or the package is not HTTPS. `http://` works only on this computer, for tests. An index reaches nothing nearer than itself: one on the network cannot point to this computer (`localhost` and what is under it, with or without a final dot, `127.0.0.0/8`, `0.0.0.0/8`, `::1`, `::`) or to a private network (`10/8`, `172.16/12`, `192.168/16`, link-local such as `169.254.169.254`, `100.64/10`, `fc00::/7`, `fe80::/10`, multicast), in any way of writing them, IPv6 forms that carry an IPv4 address included. A name counts by what it resolves to at each request, so a name or DNS rebinding that leads there is refused too; an index you set by its own address on your network may use that network. A redirect goes through the same check, and an address with a user name or password is refused. |
| Size | The index is over 2 MB or the package over 64 MB. |
| Hash | The SHA-256 of the download is not the entry's. |
| Signature | It has no signature, or it is not by a trusted key, or it does not match the package. |
| Paths | A name in the `.zip` is absolute or climbs out of it with `..`. A manual install refuses this too. |
| Identity | The `id` or `version` of the manifest is not the entry's. |
| Downgrade | The version is older than the installed one. |

Two of them are yours to lift, each from its own question. A package with no valid signature shows
why, and installs only through **Install as unsigned** in a separate warning; the review then says it
is not signed. An older version installs only after you confirm it over the newer one. The hash, the
paths and the identity are always checked. Then the module is loaded for the review, and installing
takes the hash of what you reviewed, as a manual install does.

An index on your own network has to be set by its address (`https://10.0.0.5/index.json`): by a name,
it counts as on the network, whatever that name resolves to today. Behind a system proxy, it is the
proxy that resolves names, so this check only sees the address as it is written.

**A plugin's signer is pinned.** Any key you trust can sign any id, so an id alone does not say that
a package comes from the same publisher. When the signer of an installed plugin changes, from one key
to another, or to or from none (which is what a manual install and an unsigned one have), the review
says so plainly, and installing clears the plugin's secrets and what its pages keep. Its other settings
stay. Between two packages with no signature there is nothing to compare, so nothing is cleared.

The download waits in `<data>/plugins/.downloads` until you answer, and goes whatever the answer.

## ABI, versions 1 and 2

Version 2 adds two entry points (`platypus_hook`, `platypus_grid`) and a host function (`setting`) to
version 1. The rest is the same, and a module of version 1 cannot import `setting`: it would not load.

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
| `platypus_grid` | `(ptr: i32, len: i32) -> i64` | one batch (kind `grid`, ABI 2) |

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
  the rest as text, `null`), a sensitive column's as `••••••`, at most 1000 rows (`truncated` says
  there were more);
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

`statement.text` is a copy with **every value masked**: each string and each number reads `'••••••'`.
A name cannot tell where a value goes (`INSERT INTO users VALUES ('a@b.c', 's3cret')` names no column),
and a number can be a card or a salary as a string can be an email, so a guard reads what a statement
does, its keywords, tables and columns, and none of what it carries: it can refuse `DELETE` without
`WHERE` or anything naming a table, and cannot go by a value or a `LIMIT`. Parameters (`$1`) stay, as
they hold nothing, and so do comments, which are the text the user wrote about the statement. The text
is at most 64 KB (`truncated` says when it is cut), and `null` on MongoDB, Redis, Elasticsearch, the
drivers of plugins, ODBC and any other ADBC driver, whose literals cannot be told. A hook never gets a password, a value of a result or
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

Hooks run for the SQL editor, notebooks, scripts, the dialogs that run a statement, saves of edited
rows, `EXPLAIN ANALYZE`, a database's own client (as one statement, the whole script, ending as the
client's exit says) and scheduled scripts. A scheduled script asks before each of its statements: one
that is blocked fails the run with the hook's message, what ran before it stands, and the rest does not
run.

**Hooks do not cover everything, and must not be taken for a wall.** They do not run for:

- agents through MCP, which have their own policy (see [AGENTS.md](AGENTS.md));
- `.sql` files run from the file dialog and the table designer;
- exports, imports, transfers and copies, by hand or scheduled, and profiles;
- scheduled backups and schema snapshots.

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

### Tabs and side panels

`[[tabs]]` adds a kind of tab and `[[panels]]` a side panel, each with its page, an HTML document of
the package. The command palette has a command for each with its title, and a connection's menu in
the navigator opens a tab on that connection (a tab belongs to a connection). A panel opens beside
the tabs, one at a time, and goes when it is closed. A tab is kept for the next start, as other tabs
are; while its plugin is off or gone it says so, and comes back with it.

What is copied at install: the documents the manifest names, and under `ui/` the files of the kinds
a page can load (`html`, `js`, `css`, `json`, `svg`, `png`, `jpg`, `gif`, `webp`, `woff2`), at most
256 files, 8 MB each and 32 MB in all. The hash of the review covers them. A name that climbs out of
the package or a link is refused. Anything else is left behind. A document the manifest does not
name cannot be opened, and neither `plugin.toml` nor `plugin.wasm` can be read from a page.

**A page is untrusted, and it is isolated by these, all of them:**

- It runs in `<iframe sandbox="allow-scripts">`, never `allow-same-origin`, so on an opaque origin:
  no access to the app's DOM, `window.__TAURI__`, `__TAURI_INTERNALS__` or the app's storage, and it
  has no storage of its own. No popups, forms, dialogs, downloads or top navigation.
- It is served from a scheme of its own (`platypus-plugin`), whose answers carry its policy:
  `sandbox allow-scripts` again, whoever opens it, and `default-src 'none'`, with scripts and styles
  only inline or from the plugin's own folder, images and fonts also from `data:`. No `fetch`,
  WebSocket, frames, workers, `eval`, `<base>` or forms, and WebRTC blocked. The app's own policy
  admits this scheme as a frame and nothing else (`frame-src`), which also stops a page navigating
  anywhere outside it.
- Messages are the only way out. One counts only if it comes from that frame's window and from the
  opaque origin, and nothing of it is read before that.
- Tauri puts its scripts in the main frame only and its calls need a key given to that frame, so a
  frame has no way to call a command, even if it finds the transport.

`window.platypus` is there before the page's first script, with `request(name, params)`, a promise,
and `on(listener)`, told `listener("init", {plugin, language, theme, connection})` (a late listener
too), and `listener("theme", theme)` when the theme changes. `theme` is the app's colours and fonts
as CSS variables, `connection` the name, driver and database of the tab's connection, or the active
tab's for a panel; never its host, user or password.

| Request | Params | Needs | Answer |
|---|---|---|---|
| `setting` | `{key}` | its manifest declares it, and not as a `secret` | the value |
| `notify` | `{text}` | | shows a message, one line of plain text with the plugin's name |
| `clipboard` | `{text}` | `permissions.clipboard` | puts the text on the clipboard |
| `open_query` | `{text}` | | opens a new SQL tab with this text, **not run** |
| `http` | as the `http` host function | the host in `permissions.http` | `{status, headers, body}` |
| `run_action` | `{action, context}` | an action of its own module | what the action answers (not applied) |
| `state_get` | `{}` | | what its pages kept, `null` before anything |
| `state_set` | `{value}` | at most 256 KB, for each plugin | kept until the plugin is removed |

There is nothing else. **A page cannot run SQL, or read saved connections, passwords or a query's
rows**, and no permission opens that: the text of `open_query` runs only if the user runs it, through
the Safety checks of any statement. The network is reached only by the `http` request, by the app,
with the same limits as a module. Each request is checked in the page and again in the backend, for
its name, its fields, its permission, its size (1 MB) and its rate (50 a second after a burst of 100,
and a message or a new tab once a second). A refusal rejects the promise and the page goes on.

The app **stops a page**, removes it and says why, with a button to load it again, when it leaves its
document, sends more than 500 messages over the rate, sends three messages that are not the app's,
takes more than 20 s to load, or leaves four pings in a row unanswered, 8 s.

**What this cannot protect from.** In WebView2 (Windows) a sandboxed frame on this scheme runs in a
process of its own, and the app keeps answering while it loops forever or eats memory, until the
pings remove it. In WebKit (macOS, Linux) a frame shares the page's thread and process, so **a page
that loops forever freezes the whole window, and one that exhausts memory can take it down**: there
the app has no code running to stop it, and that is not something a same-process frame can be
protected from. A message's size can be known only once it has arrived: one very large message (a
400 MB string costs about a second to copy) is paid for before it is refused. The page can also draw
anything inside its own rectangle, a fake dialog included, and has the keyboard while it has the focus.
So install a plugin with a page as you would any program you run: the sandbox limits what it reaches,
not what it can make you believe.

### Grid extensions

`[[grid.columns]]`, `[[grid.renderers]]` and `[[grid.actions]]` add to the result grid, for the columns
of the database types in `types`, ignoring case and sizes (`varchar(20)` is `varchar`); all of them
without it. `platypus_grid` takes batches, in a new instance each, and keeps nothing:

| Request | Answer |
|---|---|
| `{"op": "column", "id", "columns": [{"name", "type", "masked"}], "rows": [[…], …]}` | `{"values": […]}`, one per row |
| `{"op": "render", "id", "column": {…}, "values": ["…", …]}` | `{"cells": […]}`, one per value |
| `{"op": "action", "id", "column": {…}, "value": "…"}` | what an action answers |

- A **computed column** is one more column after the result's, whose rows hold the values of the columns
  it applies to (and with no such column in the result, it is not added). A value is text, a number, a
  boolean, `null`, or `{"error": "…"}`, which shows in that cell. It is not part of the result, so
  copies, exports, totals and sorting leave it out, as with the grid's own computed columns.
- A **renderer** draws the cells of a column, one renderer per column, the first to claim it. A cell is
  `null` (as usual) or `{"text", "color", "background", "hint", "strike"}`, all optional, colours as
  `#rgb`, `#rrggbb` or `#rrggbbaa`. That is all it can say: the grid draws on a canvas, so a text is
  drawn as text, and a field that is not on this list, or a colour that is not one, is an error shown in
  that cell. There is no HTML. An unsaved edit, and search highlights, are drawn over it.
- A **command** is in the cell's menu, under Plugins. It runs on the focused cell, and its answer does
  what an action's does; `copy` needs `permissions.clipboard`.

**A plugin never reads a sensitive cell.** A column the rules or your marks call sensitive (Settings ›
Safety) comes as `••••••` with `"masked": true`, whether or not masking is on for the screen, and the
grid goes on showing you the real value until it is; NULL stays `null`. A renderer has nothing to draw
there, so it is not asked about that column. A module has no network here, whatever its `http` says, and
what it writes to its log stays masked too.

A batch has 300 million units of fuel, a tenth of an action's, 64 MB of memory, 4 MB per message and 2 s
of wall time; at most 1000 rows or values, each cut at 16 KB (a renderer leaves longer ones alone). A
batch that crashes, runs out, or answers the wrong number of values costs its extension for that result:
a column shows the error in its cells and a renderer goes back to plain values, the user is told once,
and the grid goes on.

**The grid never calls a module while drawing.** A cell is one map lookup, by row for a column and by
value for a renderer. A miss draws the value as it is, or as loading, and notes it; a timer outside the
frame sends what was noted, at most 200 at a time and one batch at a time per extension, the most recent
first, and redraws when the answer comes. Without an extension the grid goes through none of this.

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
- `examples/plugins/panel-demo`: a tab and a side panel: pages in `ui/` that read a setting, keep their
  state, call the module, show a message and open a query. `tests/frames.rs` serves them, and also
  without their permission.
- `examples/plugins/grid-demo`: a computed column, a renderer and a command on a cell, with rows that
  break a batch on purpose. `tests/grid.rs` runs it, and also without its permission.
- `examples/plugins/theme-sea`: a theme and nothing else, with no module.
- `examples/plugins/catalog`: an index with one entry, `theme-sea` as a `.zip`, its `.minisig` and the
  public key that verifies it (`trusted.pub`; its secret key was thrown away, so sign your own with
  yours). `tests/catalog.rs` serves it from a local server and installs it with that key, and is
  refused with any other.

After changing an example, build it again and copy its module next to its `plugin.toml`: the
tests use the committed `plugin.wasm`.

Other languages export the same functions: `memory`, an allocator under the names
`platypus_alloc` and `platypus_dealloc` (for actions a bump allocator is enough, since each
runs in a new instance; a driver keeps its instance and needs a real one), and the entry points; and import `log`, `http` and `take` from the
module `platypus` (in AssemblyScript, `@external("platypus", "http")`).
