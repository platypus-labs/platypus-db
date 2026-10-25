# Extending PlatypusDB

The roadmap ([ROADMAP.md](ROADMAP.md)) splits work into groups that run in parallel.
Each group owns its own folders and plugs into the app through the extension points below,
instead of editing shared files. If an extension point is missing something, extend it in
a small separate change rather than adding feature logic to a shared file.

## Backend

### Tauri commands

`apps/desktop/src-tauri/src/commands/` has one module per area: `connections`, `query`,
`meta`, `data`, `grid`, `summary`, `transactions`, `export`, `sql`, `history`, `files` and
`settings`. Shared state
(`AppState`, sessions, `live`) is in `commands/mod.rs`.

To add commands:
1. Create `commands/<area>.rs`.
2. Declare it with `pub mod <area>;` in `commands/mod.rs`.
3. List its commands in `generate_handler!` in `lib.rs`.

Child modules can read `AppState`'s private fields.

### Optional driver features

A database feature that not every driver supports is an optional method on the
`Connection` trait (`crates/platypus-core/src/connection.rs`), plus a flag in
`Capabilities` (`capabilities.rs`). The method's default returns `Error::Unsupported`, and
the flag says whether the driver overrides it. `open_session` returns the capabilities, so
the UI can hide what a connection cannot do.

Existing optional methods are `explain`, `object_ddl` and `databases`. `object_properties`
and `catalog_foreign_keys` are optional too, but need no flag: their default is an empty
list. To add one:
1. Add the method, with a default that calls `unsupported("…")`.
2. Add a flag in `Capabilities`, defaulting to `false`, and the same field in
   `Capabilities` in `api.ts`.
3. Add a check to `capabilities_match_optional_methods` in
   `crates/platypus-conformance/tests/conformance.rs`.
4. Forward it in the two connection wrappers, which otherwise return the default:
   `Tunneled` in `crates/platypus-net/src/lib.rs` and `Logged` in
   `apps/desktop/src-tauri/src/querylog.rs`.

A driver that implements the method sets its flag in `fn capabilities`.

`explain` returns the server's plan document and the same plan as a tree of
`platypus_core::PlanNode` (operation, detail, cost, estimated and actual rows, time, loops,
other properties), which the explain tab shows for every database alike. A new driver runs
its plan request through `platypus_core::explain_document`, which with `analyze` wraps it
in a transaction (or a savepoint) rolled back afterwards, and parses the result in its own
`explain` module, then builds the result with `Explain::new`, which fills each step's own
cost and time for the heatmap. A driver that recognises a filtered full scan of a table sets
`PlanNode::full_scan`; `Explain::new` turns the large ones into index suggestions.

### Query log

Every session and export wraps its connection in `Logged`
(`apps/desktop/src-tauri/src/querylog.rs`), which records each request into the in-memory
`QueryLog` that the query manager reads with `query_log`. A new place that opens a
connection for the user wraps it the same way (`Logged::wrap`).

Metadata every driver must answer is a required method instead, checked by
`navigates_metadata` in the conformance tests: `schemas`, `objects`, `table_details`,
`referencing_keys` (the foreign keys of other tables that point at a table) and
`catalog_columns`.

### Display formats

The grid's text is rendered in Rust (`display.rs`) with the user's formats from
`formats.rs`: dates and timestamps, the time zone of zoned timestamps, digit grouping and
relative times. Their settings are declared in `apps/desktop/src/grid/formats.ts`. Editing,
copying, searching and exports read the plain values of `display::format_options` instead.

### Network paths

`platypus_net::connect` opens a driver's connection through the SSH tunnel of its
`ConnectionConfig` (`ssh`), and keeps the tunnel open as long as the connection. Drivers
only see `connect_addr`, the local end of the tunnel, while `host` still names the server
for TLS. A new kind of path (a proxy, a port-forward) is a new module of `platypus-net`
that sets `connect_addr` the same way.

TLS for a network driver comes from `platypus_tls::client_config`: a rustls `ClientConfig`
(`ring` provider) with libpq `sslmode` semantics, root and client certificates from the
config, or `None` with `disable`. The driver decides how a mode maps to its protocol:
PostgreSQL negotiates TLS in-band, ClickHouse's HTTP picks `https` from `require` up,
libSQL's tries it first from `prefer` up.

### Session setup

`ConnectionConfig.init` (`platypus_core::SessionInit`) holds the default schema, the time
zone and the bootstrap statements. Every driver calls `platypus_core::run_init` at the end
of `connect`, so a reconnect sets the session up again; `SessionInit::sql` gives the
statements per driver. A new driver does the same in its `connect`, and adds its case to
`SessionInit::sql`.

### Passwords

Where a connection's password comes from, in order: a shell command
(`platypus_store::command_password`), `.pgpass` (`pgpass_password`), then the store's
`SecretStore`: the OS keychain (`KeyringSecrets`), or with a master password the encrypted
`secrets.vault` (`VaultSecrets`, Argon2id key, AES-256-GCM). `ConnectionStore::move_secrets`
moves every saved secret from one store to the other; `commands/vault.rs` turns the master
password on and off. A locked vault makes `password` fail with `StoreError::Locked`, which
the UI answers with the unlock dialog.

`commands/health.rs` pings an idle session (`ping_session`): it measures the latency and
reopens a connection the server dropped. Any answer from the server, an error included,
counts as alive.

### SQL generation

Anything that builds SQL goes through `platypus_sql::Dialect` (`crates/platypus-sql/src/dialect.rs`)
rather than checking the driver. It provides:
- `quote_ident`, `qualified`, `quote_literal`
- `bool_literal`, `binary_literal`, `has_default_keyword`, `insert_defaults`
- `limit_clause`, and `limited` for queries without `ORDER BY` (`SELECT TOP (n)` in
  Transact-SQL, whose `OFFSET ... FETCH` needs one)
- statements for the navigator (`generate.rs`): `select`, `insert`, `update`, `delete`,
  `upsert`, `drop`, `rename`, `truncate`. `drop` and `rename` return `None` for an object
  kind the dialect cannot drop or rename; `dialect_support` reports the kinds to the UI.
- `column_type`: the column type that holds an Arrow type, for tables created by an import;
  `upsert_rows`, the upsert of a `VALUES` list (`ON CONFLICT`, `ON DUPLICATE KEY`, `MERGE`)

`platypus_sql::dml` builds the data editor's `UPDATE`, `INSERT` and `DELETE` statements from
text values and the columns' Arrow types, and `dml::apply` runs them all or none.
`platypus_transfer::import` copies a data file's rows into a table the same way, read through a
DuckDB connection on the file (`platypus_driver_duckdb::files`, which also opens data files as
databases); a new file format is a reader there.
`platypus_sql::analyze` tells what a statement does (query, DML, DDL) and which table an
unfiltered `UPDATE`, `DELETE` or `TRUNCATE` changes in every row; the checks before running
(`apps/desktop/src/guards/`) use it. `platypus_sql::source_table` tells which table a `SELECT` reads, when its rows are that
table's, so the editor can change the results of such queries too.
`platypus_sql::pg_literal` splits PostgreSQL array and composite literals into their
elements and joins them back, for the data editor's list of elements; `ColumnMeta.fields`
names a composite type's fields.

Add dialect rules there, with tests. `apps/desktop/src/sqlNames.ts` mirrors `quote_ident`
for the UI.

### Drivers without SQL (MongoDB)

A driver whose language is not SQL still implements `Connection`: `execute` takes the
statement in its own language and returns Arrow batches. `platypus-driver-mongo` is the
example:
- `shell` parses the mongosh subset into a `Command`, and `split`/`statement_at` cut a script
  into statements; `kind` and `unfiltered` feed the checks before running (the desktop
  `analyze_sql` command calls them instead of `platypus_sql` for `DriverKind::Mongo`).
- `docs` turns documents into batches: one column per top-level field of the first batch,
  nested values as JSON text, the BSON type in the `META_DB_TYPE` field metadata
  (`document`, `array`, `objectId`, ...), which the UI reads (`DocumentPanel`).
- `generate` writes the navigator's object statements in mongosh; the desktop
  `generate_sql` and `dialect_support` commands route there.
- `Dialect::from(DriverKind::Mongo)` maps to an inert dialect: commands that build SQL
  (`lint_sql`, `format_sql`, `query_source`) return early for MongoDB, and the UI hides SQL-only
  features with `isMongo` (`apps/desktop/src/api.ts`): the data editor, explain, the visual query
  builder, data search and the grid's SQL filters. `sqlNames.ts` writes the data viewer's query
  (`viewerSql`) and collection references in mongosh; `editor/mongo.ts` is the editor language.

Conformance scenarios that are not SQL-specific have a MongoDB version in
`crates/platypus-conformance/tests/mongo.rs`.

### Connections file

`platypus_store::ConnectionStore` keeps `connections.toml`, now `version = 5`: a list of
`[[projects]]` (`id`, `name`, `color` as `#rrggbb`, `initials`, `connections`, the ids of
its connections, with their `folders` and `placement`), then the `[[project_folders]]` of
the rail, then the `[[network_profiles]]` (an SSH tunnel and SSL settings that
connections reference by `network_profile`; `ConnectionStore::effective` puts them in place
of the connection's own), before the `[[connections]]`. A connection can be in several
projects, or in none. Older files are read and written back as version 5; a file with a
newer version is refused, so an older app never drops what it cannot read. Ids of
connections that no longer exist are dropped when the file is opened, and deleting a
connection removes it from every project. Version 4 added each connection's
`agent_access` (`off`, `read_only` by default, `read_write`); see [Agents](#agents-mcp).
Version 5 added folders (`connections/folders.rs`): a project's `folders` (`id`, `name`,
`parent`) and `placement` (connection id → folder id), and the rail's `project_folders`
(`id`, `name`, `color`, `projects`, `collapsed`); a project is in one rail folder at most.

The projects' commands are in `commands/projects.rs`: `list_projects`, `save_project`,
`delete_project`, `set_project_membership` and `project_initials`; for folders
`create_connection_folder`, `rename_connection_folder`, `move_connection_folder`,
`delete_connection_folder` and `place_connection`, and for the rail `list_project_folders`,
`group_projects`, `save_project_folder`, `set_project_folder`, `ungroup_projects` and
`reorder_projects`, which return a `Workspace` (connections, projects and rail folders).
`save_connection` takes the projects a connection joins. Imports (`platypus_store::import`)
read each file into `ImportedProject`s (a DBeaver `.dbp` gives one per project), and
`save_imported` saves the ticked part as an `ImportRequest` in one write
(`connections/imports.rs`).

### Agents (MCP)

`crates/platypus-mcp` lets external agents use the saved connections. Its library is the
`Agent` (`agent.rs`), with one method per tool; the binary `platypus-mcp` serves them over
stdio with `rmcp` (`server.rs`), and the `platypus` CLI calls the same methods for
`--connection` (`crates/platypus-cli/src/agent.rs`). The pieces:
- `Profile` finds the app's folders the way Tauri does (`dirs::config_dir()` and
  `dirs::data_dir()` with the bundle identifier), or `PLATYPUS_DATA_DIR`. It reopens
  `connections.toml` at every call, so a change in the app applies at once; with a master
  password the `VaultSecrets` stay locked, and a connection needing them fails with
  `AgentError::Locked`.
- `connect.rs` opens a saved connection like the app: `ConnectionStore::effective` (network
  profile), the password from `platypus_store::external_password` or the store, the SSH
  secrets, the bootstrap statements, and `platypus_net::connect`. A DuckDB session for
  agents then turns external access off (no `read_text`, `glob`, `COPY` to files or
  `INSTALL`; a data file may read only itself, through `allowed_paths`) and locks its
  configuration. Connections stay open between calls, one per saved connection, and are
  reopened when the fingerprint of their effective settings changes; DuckDB's close after
  each call, since its file lock would keep the app out.
- `access.rs` decides what runs. `SavedConnection::effective_agent_access` caps production
  and read-only connections at read-only. `access::check` takes one statement
  (`platypus_sql::split`) and runs `platypus_sql::analyze`: read-only access runs only
  `StatementKind::Query`, and refuses by name the functions a read-only transaction lets
  act outside it (PostgreSQL's `pg_terminate_backend`, `pg_reload_conf`, `pg_notify`,
  `dblink`, `lo_export`, advisory locks…; MySQL's `LOAD_FILE`, `INTO OUTFILE`); read-write
  refuses transaction control and, unless the `safety.unfiltered` setting is `off`, the
  `unfiltered` changes the app would confirm.
- `query.rs` runs the statement within `Limits` (rows, bytes, timeout; the timeout cancels
  through the connection's `CancelHandle`). On read-only access it runs between
  `Dialect::read_only_begin` and `Dialect::read_only_end`: a `READ ONLY` transaction where
  the database has one (`Dialect::refuses_writes_when_read_only`), else a transaction that
  is always rolled back. DuckDB and SQL Server have no read-only transaction: a write that
  got past the checks runs and is rolled back, holding its locks until then; SQL Server
  waits 5 seconds at most for a lock meanwhile (`LOCK_TIMEOUT`), and effects outside the
  transaction are not undone. A new driver adds its case to those `Dialect` methods; the
  conformance test `agents_read_in_read_only_transactions` checks it.
- `output.rs` renders a result as an aligned text table or the JSON documented in the README
  ([Agents](../README.md#agents)); the whole rendering stays within the byte limit, rows
  dropped from the end until it fits. Change that JSON only by adding fields.
- `redact.rs` masks the connection's password, SSH secrets, password command and the
  password of any URL in every error an agent reads. Tools never return the host, the user
  or SSH settings.

Every `run_query` is recorded in the query history with `origin`, the MCP client's name
(`clientInfo.name`, trimmed to 64 characters, `agent` when empty) or `platypus-cli`; `HistoryQuery::origin` filters the app's entries
(`NULL`) or the agents'. A new tool is a method on `Agent`, a `#[tool]` in `server.rs` and,
for the CLI, a subcommand. `commands/agents.rs` gives the Settings' Agents page
(`apps/desktop/src/agents/`) the server's path and the commands for Claude Code and Codex.
The installers ship `platypus-mcp` as a Tauri external binary: `tauri build --config
src-tauri/tauri.mcp.conf.json` runs `scripts/stage-mcp.mjs`, which builds it into
`src-tauri/binaries/`; plain builds and `cargo` commands do not need it.

### Settings storage

`platypus_store::SettingsStore` stores `settings.toml` under dotted keys. Rust code reads a
value with `get::<T>("section.key")`. The keys, types and defaults are declared in the UI,
as described under [Settings](#settings).

## Frontend

### Actions: menus and shortcuts

`apps/desktop/src/actions.ts` is the registry of user commands. An action has:
- an id;
- a scope: where it applies and what argument it receives;
- a title;
- optionally a shortcut, a menu group and order, and a `when` condition.

Context menus, shortcuts and the command palette read from the registry: global actions
appear in the palette by their title. The user can change any action's shortcut in the
Settings; it is stored under `shortcuts.<action id>`, and `shortcutsOf(action)` gives the
ones in effect.
Actions with the same `submenu` title are listed together under that entry, e.g. `Copy as`.

```ts
registerAction({
  id: "grid.copy_as_insert",
  scope: "grid",
  title: "Copy as INSERT",
  group: "copy",
  order: 20,
  when: ({ grid, source }) => grid.selection() !== null && source?.table != null,
  run: ({ grid, source }) => { /* … */ },
});
```

| Scope | Argument | Shown in |
|---|---|---|
| `global` | — | shortcuts anywhere, including in the editor |
| `navigator.connection` | `{ connection }` | navigator menu |
| `navigator.database` | `{ connection, database, name }` | navigator menu |
| `navigator.schema` | `{ connection, database, schema }` | navigator menu |
| `navigator.object` | `{ connection, database, schema, object, target }` | navigator menu |
| `navigator.column` | `{ connection, database, schema, object, column }` | navigator menu |

`database` is `null` for the connection's saved database. Otherwise it names another
database of the same server (PostgreSQL), and sessions and tabs opened from there use it.
| `grid` | `{ grid: GridApi, source }` | grid menu, and shortcuts while the grid has focus |
| `editor` | `{ view: EditorView, driver, readOnly, host? }` | editor menu, and shortcuts while the editor has focus (before CodeMirror's own keymaps). `host` is what the tab offers (`EditorHost` in `editor/SqlEditor.tsx`): open an object, run with the CLI, run into a new result tab |
| `project` | `Project` | menu of a project's square in the rail |
| `script` | `{ entry, project, root, connections, rename }` | menu of a saved script or folder in the Scripts panel |

To add a scope, augment `ActionScopes` from the feature's own module:
`declare module "../actions" { interface ActionScopes { … } }`.

Put a feature's actions in its own module and import that module once. For example,
`grid/actions.ts` holds the grid's built-in Copy and Select all.

### Tab kinds

`apps/desktop/src/tabs/registry.ts` defines tab kinds. A kind has:
- a component;
- `create`, which builds the tab from what the opener passes;
- `label`;
- `persist` and `restore`, for the workspace saved across restarts.

To add a kind:
1. Augment `TabKinds` with its model, which extends `TabBase`.
2. Call `registerTabKind` in `tabs/<kind>.ts`.
3. Import that file in `tabs/index.ts`.

Open a tab of the new kind with `workbench().openTab("<kind>", connectionId, init)`. The SQL
editor (`kind: "query"`) is the reference implementation, in `components/QueryTab.tsx` and
`tabs/query.ts`.

### Workbench

`workbench()` (`apps/desktop/src/workbench.ts`) is how actions and tabs reach the shell
without props. It provides:
- the connections, and `setConnections` for commands that return the changed list;
- `openTab`;
- `sideSession`, the navigator session of a connection and database;
- `capabilities`, what an open session's driver supports;
- `showError`;
- `openSettings`;
- `openDialog`, which shows a dialog from an action. Its `render(close)` returns the dialog;
- `projects`, `setProjects`, `activeProject` and `switchProject`, and `editProject` and
  `deleteProject`, which open the project dialog or ask before deleting.

### Projects

`apps/desktop/src/projects/` holds the rail (`ProjectRail.tsx`), the project dialog, the
dialog that adds saved connections to a project, and the actions (`actions.ts`): the
`Ctrl+1`… switches, next and previous project, the `project` scope of a square's menu, and
"Add to project" and "Remove from this project" in the navigator. `state.ts` filters the
connections of a project and remembers the active one.

Every tab has a `project` (`TabBase.project`, `null` for All connections). All tabs stay
mounted; the tab bar shows those of the active project, and each project remembers its
active tab. The workspace is saved as `platypus.workspace.v2` in `localStorage`, with the
active tab per project; a v1 workspace is read once and its tabs go to All connections. A
new tab joins the active project, so features that open tabs need nothing more.

### SQL editor

`apps/desktop/src/editor/` holds the editor (`SqlEditor.tsx`) and what it is built from:
completion sources (`joinCompletion.ts`, `functions.ts` for the built-in functions of each
dialect, `snippets.ts`), the linter (`lint.ts`), spell checking (`spellcheck.ts`), the
terminal mode (`terminal.tsx`) and the side panels. A completion source is added in
`language()` in `SqlEditor.tsx`.

Statements come split by the backend (`analyze_sql`). A line starting with `@` at a
statement start is a client command: it ends at the line and never reaches the database.
`parseCommand` in `editor/params.ts` reads it and the run loop in `QueryTab.tsx` runs it; a
new command is a case in both. Placeholders (`:name`, `?`, `${name}`) come with each
statement (`StatementSpan.parameters`) and `substitute` replaces them before the statement
runs.

Snippets are stored as JSON in the settings under `editor.snippets`. Saved scripts are
`.sql` files in `scripts/<project id>` (`scripts/all` for All connections) in the app's data
folder, with the connection links in `.links.json`; the commands are in
`commands/scripts.rs` and the panel in `apps/desktop/src/scripts/`.

### Navigator refresh

After changing objects, call `requestNavigatorRefresh({ connectionId, database, schema })`
from `navigatorEvents.ts`. The navigator reloads that part of the tree, and autocompletion
and open object tabs reload as well. Listen with `onNavigatorRefresh` to react to it.

### Settings

Declare a setting next to the code that reads it:

```ts
export const scriptRowLimit = defineSetting({
  key: "editor.script_row_limit",
  section: "SQL editor",
  label: "Rows fetched per statement when running a script",
  default: 1000,
  control: { type: "number", min: 1 },
});

getSetting(scriptRowLimit);  // in handlers
useSetting(scriptRowLimit);  // in components, re-renders on change
```

The Settings dialog (Ctrl+,) lists every declared setting, grouped by section in the order
they are declared. Settings are loaded before the first render; `onSettingsChanged` calls a
function after any change, for code outside React. Controls are `boolean`, `number`, `text`,
`select`, `color` (a `#rrggbb` value, empty for none) and `swatches` (choices shown as cards
of colors).

### Theme

`apps/desktop/src/theme.ts` owns every color: the presets in `PRESETS` and the Appearance
settings over them become CSS variables on `<html>` (`--bg`, `--fg`, `--accent`, the grid's
and the syntax colors, `--radius`, `--grid-row-h`, `--editor-font-size`). Stylesheets use
the variables, never fixed colors. A new preset is one more entry in `PRESETS`. Code that
caches colors, like the grid's canvas, reads them again on `THEME_EVENT`.

A preset gives the base colors (`BasePalette`) and may give the extra ones (`ExtraPalette`):
`--bg-rail`, `--bg-tabbar`, `--fg-strong`, `--border-strong`, `--active`, the object colors
`--obj-db|table|view|routine|trigger` and the connection types
`--type-dev|test|prod` with their `-bg`. What a preset leaves out comes from `derive()`,
which also follows the custom background, sidebar, text and accent colors. "System" stands
for `platypus_dark` or `platypus_light`. The last applied variables are cached in
`platypus.appearance.v2`; a change to the variables' set bumps that key.

### Stylesheets, fonts and icons

`src/styles/` holds one file per area, imported in order by `index.css`: `base` (reset,
fonts, buttons, inputs, `kbd`), `shell` (layout, tabs, status bar), `navigator`, `editor`
(toolbar, connection chip, history, object tabs), `grid` (results, data editor, panels) and
`dialogs` (modals, Settings, palette, menus). A group adds its rules to the file of its area.

Inter and JetBrains Mono (Latin and Latin Extended, OFL) are in `src/assets/fonts/`, exposed
as `--font-ui` and `--font-mono`; the grid's canvas reads `--font-mono` too and redraws when
the fonts finish loading.

Icons come from Lucide through `src/ui/Icon.tsx`: `<Icon name="table" />`, and
`objectIcon(kind)` for a database object. A new icon is one more name in its map; components
never import `lucide-react` themselves.

### Result grid

`DataGrid` (`apps/desktop/src/grid/DataGrid.tsx`) takes these extension props:

| Prop | Purpose |
|---|---|
| `decorate(row, col)` | Background, colour or replacement text for a cell, e.g. edited values or search hits. It runs for every visible cell on each frame, so keep it cheap. |
| `overlay` | Elements positioned over the grid, e.g. an inline editor placed with `GridApi.cellRect`. |
| `onCellDoubleClick` | Called when a cell is double-clicked. |
| `topRows` | Rows drawn above the result's, e.g. new rows; their cells come from `decorate`. |
| `onScroll` | Called when the grid scrolls, e.g. to move an overlay. |
| `onFocusChange` | Called when the focused cell changes, e.g. for the value panel. |
| `onSelectionChange` | Called when the selection changes, e.g. for its totals. |
| `layout`, `onLayoutChange` | The column layout (`grid/layout.ts`): order, hidden and pinned columns, widths. Without `onLayoutChange` the columns keep the result's order. |
| `ref` | Gives the `GridApi`. |

Cells and selections always use result columns, wherever the layout shows them: a
selection lists its columns in `cols`, left to right as shown, and `visibleColumns()` gives
the display order. `GridApi.dataRow` maps a grid row to the result's row (`null` for the top
rows). `grid/ResultGrid.tsx` hosts a result's features around the grid: the data editor
(`grid/edit/`), search, totals, column, copy, filter and foreign key actions
(`grid/features/`), the value, grouping, metadata, calc, map and references panels, the
record view and the row comparison (`panels/`, `grid/features/references.tsx`). It combines their decorations, so each feature
keeps its own.

A decoration can set `background`, `color`, replacement `text`, `strike` and a muted `hint`
after the value (the data editor's foreign key labels). Features that
need their own object in grid actions add a field to `GridSource` by augmenting
`GridSourceExtensions`, as the data editor does with `edit` (`grid/edit/TableEditor.tsx`).

The `GridApi` reads values and the selection, moves the selection, gives cell boxes,
redraws and gives the keyboard back to the grid. It is also what `grid` actions receive,
together with `source`: the connection, the SQL, the job and the data-viewer table.

## End-to-end tests

`apps/desktop/e2e/` drives the release build through the WebView2 DevTools protocol, on
Windows only. Each scenario runs against a fresh profile: `PLATYPUS_DATA_DIR` and a separate
WebView2 folder. The user's data is never touched. The runner sets `PLATYPUS_DEVTOOLS_PORT`,
which makes the app open the protocol port itself. With it set, `app_info().devtools` is
true and the app exposes test hooks on `window.__platypus`, such as `switchProject(id)`.

```bash
npm run tauri build -- --no-bundle
npm run e2e              # all scenarios
npm run e2e -- postgres  # file names containing "postgres"
```

A scenario in `e2e/scenarios/` exports:
- `default(page, ctx)`, which runs the steps;
- optionally `setup(ctx)`, which seeds `ctx.dir` (for example `connections.toml`) before
  the app starts;
- optionally `skip()`, which returns a reason not to run, for example when a dev database
  is down;
- optionally `env(ctx)`, extra environment variables for the app;
- optionally `ready`, the condition to wait for before the steps (by default the navigator
  showing a connection; a scenario with no saved connections waits for something else).

Set `PLATYPUS_E2E_SHOTS` to a folder to get a screenshot of each failure, plus the ones the
scenarios take: `visual-sqlite` saves the main screens in Platypus Dark and Light, to compare
a change to the look by eye. CI runs the
scenarios on Windows. Scenarios that need the Docker databases skip themselves there.
