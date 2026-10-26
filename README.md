<h1 align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/brand/platypus-db-on-dark.png" />
    <img src="docs/brand/platypus-db-on-light.png" alt="PlatypusDB" width="640" />
  </picture>
</h1>

A fast, lightweight desktop SQL client: a Rust core with a Tauri 2 + React interface. The
installer is about 5 MB, the window is ready in under half a second, and the results grid
scrolls through millions of rows.

It works with **PostgreSQL**, **MySQL**, **MariaDB**, **SQL Server** (and Azure SQL),
**ClickHouse**, **SQLite**, **DuckDB**, **libSQL/Turso** and **MongoDB**, and with the servers
that speak the PostgreSQL protocol: **CockroachDB**, **YugabyteDB**, **TimescaleDB**,
**Supabase**, **Neon** and **AlloyDB**.

![The data viewer with the value panel](docs/screenshots/data-viewer.png)

- [Highlights](#highlights)
- [Install](#install)
- [Databases](#databases)
- [Features](#features)
- [Agents (MCP) and the CLI](#agents-mcp-and-the-cli)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Performance](#performance)
- [Contributing](#contributing)

## Highlights

- **Small and quick**: a 5 MB installer, about 130 MB of memory with a connection open, and a
  canvas grid that reads rows a page at a time.
- **Edit data safely**: change cells in place, preview the SQL, save all or nothing, and get an
  undo script for every save.
- **A real SQL editor**: completion of tables, columns and JOINs from foreign keys, a formatter,
  parameters, snippets, a linter and query plans with a heatmap.
- **Move data around**: import and export CSV, JSON, Parquet, Excel and more, copy tables and
  whole schemas between any two databases, generate fake rows.
- **Production-aware**: connection types, confirmations before risky statements, SSH tunnels,
  keychain or encrypted passwords.
- **Agent-ready**: Claude Code, Codex, Cursor and other MCP clients can use your saved
  connections, with the access you give each one.

## Install

Download the installer for your system from the
[releases page](https://github.com/PaoloCisly/platypus-db/releases): `.exe`/`.msi` for
Windows, `.dmg` for macOS, `.AppImage`/`.deb`/`.rpm` for Linux. The app updates itself.

## Databases

| Database | How it connects | Notes |
|---|---|---|
| PostgreSQL | native protocol | Several databases per connection, partitions, PostGIS, arrays and composite types |
| MySQL, MariaDB | native protocol | |
| SQL Server, Azure SQL | TDS | `TOP (n)`, `[names]`, several result sets |
| ClickHouse | HTTP interface (8123, or 8443 with TLS), Arrow results | Browsed, queried and designed; no data editing in the grid |
| SQLite | embedded | |
| DuckDB | embedded, Arrow results | Also opens CSV, JSON, Parquet and Excel files as databases |
| libSQL / Turso | Hrana over HTTP | Token in the password or in `authToken` |
| MongoDB | official driver | mongosh syntax; no transactions, explain or data editing |
| CockroachDB, YugabyteDB, TimescaleDB, Supabase, Neon, AlloyDB | PostgreSQL driver | Profiles with their default port, user, database and SSL |
| Snowflake, BigQuery, Databricks, Trino | ADBC driver, downloaded on first use | Queries, results, navigator and autocompletion; no data editing, explain or manual commit yet |
| Any database with an ADBC driver | ADBC driver library you choose | Its URI and options as the driver documents them |
| Oracle | native driver with Oracle Instant Client, downloaded on first use | PL/SQL blocks (end them with a `/` line in scripts), real cancel, navigator, DDL; Windows needs the Visual C++ Redistributable, Linux `libaio` |
| IBM Db2 | IBM's ODBC driver, installed on first use (Windows) | Registering the driver asks for administrator rights (UAC) |
| Any ODBC driver or DSN (Windows) | the system's ODBC driver manager | A DSN, or a driver and its connection string |

The ADBC drivers come from the [dbc driver registry](https://docs.columnar.tech/dbc/) (Columnar):
the first connection that needs one asks to download it, checks its SHA-256 and keeps it with
the app's data. Oracle Instant Client and IBM's ODBC driver come from Oracle's and IBM's sites
in the same way, with their license shown first. *Settings › Drivers* lists and removes them,
and shows the ODBC drivers and data sources of the system.

## Features

### Connections

![Choosing the database for a new connection](docs/screenshots/connections.png)

- A new connection starts from a grid of database logos, with a search on top (`Enter` takes
  the first match); the `+` of the navigator opens it, and so does a right-click on a connection
  or below the list. Paste a `postgres://`, `mysql://`, `sqlserver://`, `clickhouse://`,
  `libsql://` or `mongodb://` URL to fill the form.
- **Import** connections from DBeaver, DataGrip, pgAdmin and `pg_service.conf`, with their
  folders. A DBeaver project export (`.dbp`) brings its projects too, optionally gathered in a
  folder of the project rail.
- **Folders**: connections inside a project go in nested folders (drag them, or *Move to
  folder*); projects in the rail go in folders too, by dropping one project on another.
- **TLS** with libpq `sslmode` semantics, custom CAs, and client certificates for PostgreSQL and
  MySQL.
- **SSH tunnels** with a password, a key or the SSH agent, optionally through a jump host;
  server keys are checked against `~/.ssh/known_hosts`. **Network profiles** share one tunnel
  and SSL setup between connections.
- **Passwords** from the system keychain, asked every time, read from `.pgpass` or printed by a
  shell command (a password manager's CLI). A **master password** can keep the saved ones in an
  encrypted file instead (Argon2id and AES-256-GCM).
- **Connection types** Development, Test and Production, with colors and a read-only flag.
  Production shows a red PROD badge and opens in manual commit.
- After connecting, a default schema, a time zone and bootstrap statements run on every
  connection and reconnection.
- Idle sessions are checked every minute: the status bar shows the latency, and a connection
  the server dropped is reopened.

### Navigator and objects

- Databases (PostgreSQL), schemas, tables, views, routines, triggers, types, extensions and
  events, loaded lazily, with a filter. PostgreSQL partitions sit under their table.
- Estimated rows and sizes of tables and indexes.
- Bookmarks, per-connection filters that hide schemas or objects by name (`tmp_*`), and a
  Simple view with only tables and views.
- The tree is cached on disk: it shows at once on start, and offline. Nothing connects until
  you open it; a green dot marks the connections with an open session.
- Generate SELECT, INSERT, UPDATE, DELETE, UPSERT or DDL; rename, truncate and drop with a
  preview of the statement. `Ctrl`/`Shift`+click selects several objects, for example to get
  the DDL of many tables at once.
- **Object details**: properties, columns, indexes, constraints, foreign keys, incoming
  references and the DDL of any object.
- **Notes and tags** on tables, views and columns, kept on your computer and never written to
  the database. A mark and a tooltip in the navigator, `#pii` in the filter to find the tagged
  objects, and the note and tags in the data viewer's column headers and in completion. They
  follow the renames made in the app.
- **Database comments** of tables, views and columns edit in the object tab: the statement
  shows first. Not on SQLite, libSQL and MongoDB, which have none.
- **Schema documentation**: right-click a schema, *Export documentation…*, for a Markdown file
  or a single HTML file (it works offline) with the columns, keys, indexes, comments, notes
  and an ER diagram.
- **Search** object names, columns and the source of views, routines and triggers across a
  database; or the text of every row of a schema's tables, opening the matching rows.

### SQL editor

![JOIN completion from the foreign keys](docs/screenshots/autocomplete.png)

- Run the statement under the cursor (`Ctrl+Enter`) or the whole script (`Alt+X`); errors are
  pointed out in the text.
- **Completion** of keywords, tables, columns and built-in functions with their signature. The
  columns offered are those of the tables the statement under the cursor reads (even when FROM
  comes later), and `alias.` completes the columns of a table, a CTE or a subquery. Names that
  need quotes are quoted on insert. `JOIN ` offers the related tables with alias and condition
  from the foreign keys; `JOIN t ON ` offers the condition. Tab or Enter accepts; Enter after a
  word typed exactly as the entry starts a new line. While a snippet's fields are active, Tab
  moves to the next field.
- **Formatter** (`Ctrl+Shift+F`) with configurable keyword case and indentation; the same
  setting gives the case of completed keywords.
- Line commands, multiple cursors, folding, an outline of the statements, and `Ctrl+click` on a
  name to open its object.
- **Parameters** (`:id`, `?`) and **variables** (`${name}`) asked in a dialog and kept in a
  Variables panel; client commands `@set`, `@echo`, `@include file.sql` and
  `@export file.csv`.
- Editable snippets (`sel` then `Tab`), query history, `.sql` files.
- `Ctrl+\` runs into a new result tab; a terminal mode shows results as text; a script can run
  with the local `psql`, `mysql`, `sqlcmd` or `sqlite3`.
- A **linter** marks unknown columns, joins without a condition and `SELECT *` on production;
  comments are spell checked.
- **Saved scripts**: `.sql` files per project in the Scripts panel, with folders, drag to move,
  and the connection each one opens on.

### Results and the data viewer

- The grid is drawn on a canvas and reads rows a page at a time (1,000 ahead of what is on
  screen); queries can be cancelled.
- Open a table to browse it: sort by clicking a header, filter with a `WHERE`, count the rows.
- **Copy** with headers or as CSV, JSON, Markdown, HTML, SQL `INSERT` or an `IN` list.
- **Find** in the loaded rows (`Ctrl+F`).
- **Value panel** (`F7`): long text, JSON and XML formatted, hex and images from binary
  columns, save to file.
- Quick filters on the focused value; columns hidden, pinned, moved and resized, with the
  layout kept per table.
- Totals of the selection (count, sum, average, minimum, maximum), a record view of one row
  at a time, counts by value, statistics of the selection, and two rows compared column by
  column.
- Foreign keys open the row they point to, and the References panel lists the rows of other
  tables that point at the focused one.
- A **map panel** draws PostGIS, MySQL and WKT geometries and PostgreSQL's geometric types.
- Read-only results can refresh every few seconds. Dates, time zone, digit grouping and
  relative times follow the Settings.

### Data editor

![Pending changes in the data editor](docs/screenshots/data-editor.png)

- Change cells in place (double-click, `F2` or just type), set NULL or the default, add,
  duplicate and delete rows. Changes stay highlighted until you save.
- **Preview the SQL** and save it all or nothing; in manual-commit mode the changes join the
  open transaction. Every save leaves an undo script that puts the rows back.
- Tables need a primary key or a unique key on non-null columns, or a virtual key you pick;
  otherwise the grid says why it is read-only. Results of a query that reads one table edit
  the same way, with computed columns read-only.
- Paste cells from a spreadsheet or CSV over the grid or as new rows.
- Foreign keys show the name of the row they point to and edit with a list of those rows; pick
  the column that names a table's rows from a header's menu or the object tab.
- PostgreSQL arrays and composite values edit one element or field per line.

### Table editor

![Designing a table](docs/screenshots/table-editor.png)

Right-click a table, *Edit table…*, or a schema, *Create table…*: columns (type, NOT NULL,
default, primary key, comment), indexes, foreign keys with their `ON DELETE`/`ON UPDATE`
actions, and UNIQUE and CHECK constraints. Saving shows the `CREATE TABLE` or `ALTER TABLE`
script first and runs it in one transaction where the database can roll DDL back (MySQL
commits each statement). SQLite and DuckDB, which alter little in place, get the table
created again with its rows copied.

### Visual query builder

![Two tables joined on their foreign key](docs/screenshots/query-builder.png)

Drag tables onto a canvas: they join on their foreign keys. Tick the columns, add a `WHERE`,
then run the SELECT or open it in the editor.

### Query plans and performance

![Explain analyze with the heatmap and an index suggestion](docs/screenshots/explain.png)

- `Ctrl+Shift+E` opens the plan of the statement under the cursor as a tree with costs,
  estimated rows and each step's details, as indented text or as a table.
- **Explain analyze** runs the statement for actual rows and times, then rolls back what it
  changed (tables that ignore transactions, like MySQL's MyISAM, keep the changes).
  PostgreSQL, MySQL 8.0.16+ and MariaDB analyze; SQLite gives its query plan only.
- A **heatmap** tints each step by its share of the time it spends itself, and a badge marks
  row estimates 10 times off or more.
- A table read in full (10,000 rows or more) to keep the rows matching a condition gets a
  suggestion to index that condition's columns.

![The query manager with the trace of a statement](docs/screenshots/query-manager.png)

- **Query manager** (`Ctrl+Shift+M`): every statement, metadata request, commit and rollback
  the app sent, with connection, duration, rows and outcome; the last 2,000 are kept in
  memory. A statement's trace splits its time into network, server and fetch.
- Statements slower than a threshold (5 s by default) are marked in the status bar and listed
  apart.

### Import, export and transfer

- **Export** to CSV, JSON, NDJSON, Parquet, Excel, SQL `INSERT`, Markdown, HTML or XML,
  streamed straight to the file; several tables or a whole schema at once. Optional gzip or
  zstd compression, and upload to a signed URL or to an S3, GCS (HMAC keys), R2 or MinIO
  bucket; the secret key is never saved.
- **Import** CSV, TSV, JSON, NDJSON, Parquet and Excel into a table, with a preview and a
  mapping of columns, adding the rows, replacing them or updating those with the same key; or
  into a new table created with the types DuckDB infers. *Paste CSV as a new table…* imports
  the clipboard.
- **Copy a table to another database** (PostgreSQL to MySQL, SQL Server to SQLite and every
  other pair), streamed, into a new table with the types mapped or into an existing one.
- **Copy a schema** to another database: each table with its columns, keys and identity
  column, then its defaults, indexes, foreign keys and CHECK constraints; between databases of
  one kind the views, functions, procedures and triggers too.
- **Fake data** for a table: names, emails, numbers, dates, enum values, foreign keys that
  point at existing rows, distinct values for unique columns.
- **Data files as databases**: drop a CSV, JSON, Parquet or Excel file on the window to query
  it with SQL; each Excel sheet is a table.

### MongoDB

Statements in mongosh syntax: `db.orders.find({ status: 'paid' }).sort({ placed_at: -1 }).limit(10)`,
`aggregate`, `countDocuments`, `distinct`, `insertOne`/`insertMany`, `updateOne`/`updateMany`,
`replaceOne`, `deleteOne`/`deleteMany`, `createIndex`, `db.runCommand`, `show dbs`,
`show collections` and `use`, with the shell's values (`ObjectId(...)`, `ISODate(...)`,
`NumberLong(...)`, `/regex/i`) and Extended JSON. Results are a grid of the top-level fields
with the focused document as a tree beside it (`F7`). The navigator lists databases,
collections, fields sampled from the documents and indexes; the editor completes collections,
methods and fields. `mongodb+srv://` and connection string options go in the Database field
(`shop?authSource=admin&replicaSet=rs0`).

### Transactions and safety

- Auto-commit or manual commit with Commit/Rollback; an open transaction shows how long it has
  been open and warns after a while (10 minutes by default).
- An `UPDATE` or `DELETE` without `WHERE`, or a `TRUNCATE`, asks first and says about how many
  rows it changes.
- On production connections every change is listed and confirmed before it runs.

### Workspace

| | |
|---|---|
| ![Command palette](docs/screenshots/palette.png) | ![Platypus Light](docs/screenshots/light-theme.png) |

- **Projects**: a rail of colored squares on the left, one per project (a client, an
  environment, a side job). A project shows only its connections, tabs and recent queries; a
  connection can be in several projects. `Ctrl+1` is All connections, `Ctrl+2`… the projects.
- **The frame shows where you are**: the bar on top names the project and the connection of
  the active tab, and the window frame takes the color of its type, green for development,
  blue for test and red for production. On Windows and Linux the bar is also the window's title
  bar, with its buttons (Settings › Appearance › Title bar puts the system's back).
- **Command palette** (`Ctrl+K`): actions, connections, tables and recent queries, found by
  typing a few letters.
- **Themes**: Platypus Dark and Light (what "System" follows), Classic Light and Dark, Nord,
  Dracula, Solarized Light and high contrast; your own accent, background, sidebar and text
  colors, interface size, row density, corner radius and editor font size. Inter and
  JetBrains Mono are bundled, so the app looks the same everywhere and works offline.
- **Settings** (`Ctrl+,`) show one section at a time, in tabs, and open again on the last one.
  Their search shows the matching settings of every section, grouped by section, and finds the
  keyboard shortcuts by their action's name.
- **Resizable navigator**: drag the edge between the connections and the work (or focus it and
  use the arrows); a double-click brings back the default width. `Ctrl+B`, or the button at the
  left of the top bar, hides and shows it.
- Every shortcut can be changed; the toolbars show icons (each says what it does on hover), and
  the SQL editor's buttons can be hidden or given their words back; `Ctrl+Shift+T` reopens the
  last closed tab.
- **Tabs**: a right-click pins a tab (it stays first, as an icon), opens it beside the active
  one or closes the others or those to its right; Settings › Tabs groups them under their
  connection's name, and a click on the name folds the group.
- **Split editor** (`Ctrl+Shift+D`): two tabs side by side; a click in one makes it the active
  one.
- **Drag a table or a column** from the navigator into the SQL editor: its name, or a `SELECT`
  in an empty editor or with `Shift` held.
- **Italian or English** (Settings › Appearance › Language; System follows Windows).
  `Ctrl+=` / `Ctrl+-` / `Ctrl+0` change the interface size; the editor and the result grid
  have their own font size.
- **Activity log** (the pulse icon on top): the errors shown and the work done in the
  background, such as connections and catalog loads, with their time, to copy into a report.
  A query that runs longer than 10 seconds (Settings › SQL editor) ends with a system
  notification when the window is in the background.
- From the keyboard: arrows, Enter and the menu key in the navigator, the tab bar and the
  context menus; dialogs keep the focus inside them and give it back when they close.

## Agents (MCP) and the CLI

Claude Code, Codex, Cursor and other MCP clients use your saved connections through
`platypus-mcp`, an MCP server they start themselves (the app need not be open).

- Each connection gives agents **no access**, **read only** (the default) or **read and
  write**; production and read-only connections are never more than read only.
- Passwords stay in the keychain and no answer shows a host, a user or a secret.
- Agents' queries are in the history with the agent's name.

Settings → Agents shows the commands that register the server, with Copy buttons:

```bash
claude mcp add platypus -- "C:\Program Files\PlatypusDB\platypus-mcp.exe"
```

and for Codex, in `~/.codex/config.toml`:

```toml
[mcp_servers.platypus]
command = 'C:\Program Files\PlatypusDB\platypus-mcp.exe'
```

<details>
<summary>How the MCP server works</summary>

`platypus-mcp` sits next to the app's executable. It reads the connections of the app's
profile, or of `PLATYPUS_DATA_DIR` when set. With a master password the saved passwords stay
locked to agents: such connections answer with an error saying so, and work with a password
command or `.pgpass` instead. A connection that asks for its password every time is not
available to agents.

Tools: `list_connections`, `list_schemas`, `list_objects`, `describe_table`, `get_ddl`,
`search_objects` and `run_query`. `run_query` runs one statement and returns 100 rows,
32 KB and 30 s at most by default (`max_rows` up to 10,000, `max_bytes` up to 1 MB,
`timeout_seconds` up to 300), as an aligned text table or JSON, saying when rows or values
longer than 1,000 characters were left out.

On read-only connections only statements that change nothing run, inside the database's
read-only transaction (PostgreSQL, MySQL and MariaDB `READ ONLY`, SQLite `query_only`; DuckDB
and SQL Server roll the transaction back), and functions that act outside it
(`pg_terminate_backend`, `dblink`, `LOAD_FILE`…) are refused. DuckDB sessions of agents
cannot read or write other files nor load extensions, and close after each call so the app
can open the file.

On read-write connections changes commit at once; `BEGIN`, `COMMIT` and `ROLLBACK` are
refused, and so are `UPDATE` or `DELETE` without `WHERE` and `TRUNCATE` unless Settings →
Safety runs them without asking.

</details>

Agents without MCP use the `platypus` CLI (crate `platypus-cli`) with `--connection`, which
follows the same access and prints one JSON document:

```bash
platypus connections                                  # [{"name", "id", "driver", "type", "database", "agent_access"}]
platypus --connection "Local PG" schemas              # [{"name", "system", "current"}]
platypus --connection "Local PG" objects --schema demo
platypus --connection "Local PG" table --schema demo --table orders
platypus --connection "Local PG" ddl --schema demo --name orders   # {"ddl": "CREATE TABLE ..."}
platypus --connection "Local PG" search order          # {"matches": [...], "truncated": false}
platypus --connection "Local PG" query "select * from demo.orders" --max-rows 50
```

A query prints `{"columns": [{"name", "type"}], "rows": [[...]], "row_count",
"truncated", "truncated_by": "max_rows" | "max_bytes" | null, "values_cut", "elapsed_ms"}`,
with numbers and booleans as JSON values and everything else as text; a statement without
rows prints `{"affected_rows", "elapsed_ms"}`. `-f table` prints the text table instead. A
failure prints `{"error": {"kind", "message"}}` and exits with 1; `kind` is `not_found`,
`denied`, `locked`, `invalid`, `connect`, `database`, `timeout` or `store`.

## Keyboard shortcuts

On macOS use `Cmd` instead of `Ctrl`. Every shortcut can be changed in Settings › Keyboard
shortcuts.

<details>
<summary>All shortcuts</summary>

| Keys | Action |
|---|---|
| `Ctrl+Enter` | Run the statement under the cursor (or the selection) |
| `Alt+X`, `Ctrl+Shift+Enter` | Run the script |
| `Ctrl+Space` | Autocomplete |
| `Ctrl+/` | Comment / uncomment |
| `Ctrl+Shift+F` | Format the script (or the selection) |
| `Ctrl+Shift+A` | Select the statement under the cursor |
| `Ctrl+Shift+E` | Explain the statement under the cursor |
| `Ctrl+Shift+M` | Query manager |
| `Shift+Alt+Down`, `Shift+Alt+Up` | Duplicate the line |
| `Alt+Up`, `Alt+Down` | Move the line |
| `Ctrl+Shift+K` | Delete the line |
| `Ctrl+Alt+Up`, `Ctrl+Alt+Down` | Add a cursor above / below |
| `Ctrl+D`, `Ctrl+Shift+L` | Select the next / every occurrence |
| `Alt+drag` | Rectangular selection |
| `Ctrl+F` | Find and replace |
| `Ctrl+S`, `Ctrl+O` | Save / open a `.sql` file |
| `Ctrl+K`, `Ctrl+Shift+P` | Command palette |
| `Ctrl+T`, `Ctrl+W`, `Ctrl+Shift+T` | New tab / close tab / reopen the closed tab |
| `Ctrl+Tab`, `Ctrl+Shift+Tab` | Next / previous tab |
| `Ctrl+Shift+D` | Split the editor / close the split |
| `Ctrl+=`, `Ctrl+-`, `Ctrl+0` | Bigger / smaller / normal interface |
| `Ctrl+1`, `Ctrl+2`… `Ctrl+9` | All connections / the projects in rail order |
| `Ctrl+Alt+PageDown`, `Ctrl+Alt+PageUp` | Next / previous project |
| `Ctrl+,` | Settings |
| `F2`, `Enter`, typing | Edit the cell (data viewer, one-table queries) |
| `Insert`, `Ctrl+D`, `Delete` | Add, duplicate, delete rows |
| `Shift+Delete` | Set the selected cells to NULL |
| `Ctrl+V`, `Ctrl+Shift+V` | Paste over the cells / as new rows |
| `Ctrl+S`, `Ctrl+Z` | Save / undo data changes (grid focused) |
| `Ctrl+Shift+C` | Copy the selection with headers |
| `Ctrl+F`, `F7` | Find in the results / value panel (grid focused) |
| `Ctrl+Alt+R` | Record view of the focused row; `←` `→` move through the rows |

</details>

## Performance

| | PlatypusDB 0.1.0 | Target |
|---|---|---|
| Start to interactive window | 430–500 ms | < 500 ms |
| Memory at rest, 1 connection | about 130 MB | < 150 MB |
| Scrolling 1M rows | 178 fps | 60 fps |
| 5M rows to Parquet | 3.2 s, 31 MB peak | streaming |
| Installer | 5.4 MB | < 20 MB |

Windows 11, Ryzen 7 9700X; details and method in [docs/BENCHMARKS.md](docs/BENCHMARKS.md).

## Contributing

What comes next is planned in [docs/ROADMAP.md](docs/ROADMAP.md), and the working rules are in
[CLAUDE.md](CLAUDE.md). [docs/EXTENDING.md](docs/EXTENDING.md) explains how features plug in
(actions, tab kinds, settings, driver capabilities); [docs/RELEASING.md](docs/RELEASING.md)
how releases are made, and [docs/MVP_PLAN.md](docs/MVP_PLAN.md) the scope of the first release.

### Layout

```
crates/
  platypus-core/           driver traits, metadata types, result streams
  platypus-driver-pg/      PostgreSQL (tokio-postgres)
  platypus-driver-mysql/   MySQL / MariaDB (mysql_async)
  platypus-driver-mssql/   SQL Server / Azure SQL (tiberius, vendored with patches in vendor/)
  platypus-driver-sqlite/  SQLite (rusqlite)
  platypus-driver-duckdb/  DuckDB (duckdb, bundled; Arrow results as they come)
  platypus-driver-clickhouse/ ClickHouse over HTTP (reqwest; ArrowStream results decoded as they arrive)
  platypus-driver-libsql/  libSQL servers and Turso (Hrana over HTTP; SQLite's catalog code)
  platypus-driver-mongo/   MongoDB (official driver, rustls); mongosh parser, documents to Arrow
  platypus-tls/            rustls client configuration with libpq `sslmode` semantics
  platypus-sql/            dialect-aware lexer, statement splitting, read-only check, DML
  platypus-schema/         table designs for the table editor and the DDL between them
  platypus-net/            SSH tunnels (russh)
  platypus-store/          connections (TOML + OS keychain), imports, query history and catalog notes (SQLite)
  platypus-transfer/       streaming exports, imports, copies between databases, fake rows
  platypus-docs/           schema documentation: Markdown, HTML and Mermaid ER diagrams
  platypus-conformance/    integration suite run on every driver
  platypus-cli/            `platypus` CLI for exercising the core, and for agents (`--connection`)
  platypus-mcp/            `platypus-mcp`, the MCP server for agents (stdio, rmcp)
apps/desktop/              Tauri app: React UI in src/, Rust shell in src-tauri/
docker/                    dev databases with seed data
```

### Requirements

- Rust ≥ 1.99 (`rustup`)
- Node.js ≥ 24 (LTS, see `.nvmrc`)
- Tauri system prerequisites: <https://v2.tauri.app/start/prerequisites/>
  (Windows: MSVC Build Tools + WebView2; Linux: `libwebkit2gtk-4.1-dev` and friends)
- Docker, for the dev databases

### Development

```bash
# desktop app with hot reload
cd apps/desktop
npm install
npm run tauri dev

# core crates; the conformance suite (crates/platypus-conformance) runs the same scenarios on
# every driver. Server databases that are down are skipped; set PLATYPUS_TEST_PG_URL,
# PLATYPUS_TEST_MYSQL_URL, PLATYPUS_TEST_MARIADB_URL, PLATYPUS_TEST_MSSQL_URL,
# PLATYPUS_TEST_CLICKHOUSE_URL, PLATYPUS_TEST_LIBSQL_URL or PLATYPUS_TEST_MONGO to make them
# mandatory
cargo test --workspace --exclude platypus-desktop

# CLI: table (first 200 rows), csv, ndjson, json or parquet (json and parquet need -o); Ctrl+C or --timeout cancel server-side
export PLATYPUS_URL="postgres://platypus:platypus@localhost:55432/platypus?sslmode=disable"
cargo run -p platypus-cli -- query "select * from demo.type_zoo"
cargo run --release -p platypus-cli -- query -f csv -o big.csv --stats "select * from demo.big_table"
cargo run --release -p platypus-cli -- query -f parquet -o big.parquet --stats "select * from demo.big_table"
cargo run -p platypus-cli -- query -u mysql://platypus:platypus@localhost:53306/platypus "select * from type_zoo"
cargo run -p platypus-cli -- query -u sqlite:///C:/data/app.db "select * from sqlite_master"
cargo run -p platypus-cli -- query -u duckdb:///C:/data/app.duckdb "select * from duckdb_tables()"
cargo run -p platypus-cli -- query -u clickhouse://platypus:platypus@localhost:58123/platypus "select * from system.tables"
cargo run -p platypus-cli -- query -u "libsql://my-db-acme.turso.io?authToken=$TURSO_TOKEN" "select * from sqlite_master"

# MCP server for agents; the installers ship it next to the app (tauri build with
# --config src-tauri/tauri.mcp.conf.json, as scripts/build-windows.ps1 and the release do)
cargo build --release -p platypus-mcp

# end-to-end tests (Windows): drive the release build on a throwaway profile
cd apps/desktop
npm run tauri build -- --no-bundle
npm run e2e

# the README's screenshots, from the release build (readme-postgres needs the dev PostgreSQL)
PLATYPUS_README_SHOTS=../../docs/screenshots npm run e2e -- readme
```

In VS Code, **Run PlatypusDB** (Ctrl+Shift+B) installs the frontend dependencies if they are
missing, builds the sample database and starts `tauri dev`. `.vscode/tasks.json` also has
tasks to start and stop the dev databases.

### Sample database

`node scripts/sample-db.mjs` builds `samples/wildlife.db` from
[samples/wildlife.sql](samples/wildlife.sql): a SQLite platypus field survey with regions,
rivers, researchers, tagged animals, surveys, about 15,000 sightings and a `platypus_stats`
view, with foreign keys between all of them. Open it with a new SQLite connection.

### Dev databases

```bash
# PostgreSQL 17, MySQL 8.4, MariaDB 11.4 and ClickHouse 26.3; the others with their profile
docker compose -f docker/compose.yml up -d --wait
docker compose -f docker/compose.yml --profile sqlserver up -d --wait
docker compose -f docker/compose.yml --profile libsql up -d --wait
docker compose -f docker/compose.yml --profile mongo up -d --wait
```

| Database   | Host port | User / password / db           |
|------------|-----------|--------------------------------|
| PostgreSQL | 55432     | `platypus` / `platypus` / `platypus` (schema `demo`, TLS with a self-signed cert) |
| MySQL      | 53306     | `platypus` / `platypus` / `platypus` |
| MariaDB    | 53307     | `platypus` / `platypus` / `platypus` |
| SQL Server | 51433     | `sa` / `Platypus-2026` / `platypus` (TLS with a self-signed cert; the conformance tests create schema `demo`) |
| ClickHouse | 58123     | `platypus` / `platypus` / `platypus` (HTTP; the conformance tests create the demo tables) |
| libSQL     | 58080     | HTTP, no TLS; the auth token is `LIBSQL_TOKEN` in `crates/platypus-conformance/tests/conformance.rs` (the tests create the demo tables) |
| MongoDB    | 57017     | `platypus` / `platypus` (auth database `admin`) / `platypus` (collections only, no `type_zoo` or `big_table`) |

Each database has `customers`, `orders`, a `customer_totals` view, a `type_zoo` table with one
column per notable type, and a 1M-row `big_table` for streaming and scrolling benchmarks.
These credentials are for local test containers only.
