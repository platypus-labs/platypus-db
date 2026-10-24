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

A driver that implements the method sets its flag in `fn capabilities`.

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

### SQL generation

Anything that builds SQL goes through `platypus_sql::Dialect` (`crates/platypus-sql/src/dialect.rs`)
rather than checking the driver. It provides:
- `quote_ident`, `qualified`, `quote_literal`
- `bool_literal`, `binary_literal`, `has_default_keyword`, `insert_defaults`
- `limit_clause`
- `explain_json_prefix`
- statements for the navigator (`generate.rs`): `select`, `insert`, `update`, `delete`,
  `upsert`, `drop`, `rename`, `truncate`. `drop` and `rename` return `None` for an object
  kind the dialect cannot drop or rename; `dialect_support` reports the kinds to the UI.

`platypus_sql::dml` builds the data editor's `UPDATE`, `INSERT` and `DELETE` statements from
text values and the columns' Arrow types, and `dml::apply` runs them all or none.
`platypus_sql::analyze` tells what a statement does (query, DML, DDL) and which table an
unfiltered `UPDATE`, `DELETE` or `TRUNCATE` changes in every row; the checks before running
(`apps/desktop/src/guards/`) use it. `platypus_sql::source_table` tells which table a `SELECT` reads, when its rows are that
table's, so the editor can change the results of such queries too.

Add dialect rules there, with tests. `apps/desktop/src/sqlNames.ts` mirrors `quote_ident`
for the UI.

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
| `editor` | `{ view: EditorView, driver, readOnly }` | editor menu, and shortcuts while the editor has focus (before CodeMirror's own keymaps) |

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
- `openDialog`, which shows a dialog from an action. Its `render(close)` returns the dialog.

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
(`grid/features/`), the value, grouping and references panels and the record view
(`panels/`, `grid/features/references.tsx`). It combines their decorations, so each feature
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
which makes the app open the protocol port itself.

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
  is down.

Set `PLATYPUS_E2E_SHOTS` to a folder to get a screenshot of each failure. CI runs the
scenarios on Windows. Scenarios that need the Docker databases skip themselves there.
