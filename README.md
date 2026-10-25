# PlatypusDB

Fast, lightweight desktop SQL client for **PostgreSQL**, **MySQL/MariaDB**, **SQL Server**
(and Azure SQL), **ClickHouse**, **SQLite**, **DuckDB** and **libSQL/Turso**, and for **MongoDB** in
mongosh syntax.
Rust core, Tauri 2 + React UI: a 5 MB installer that starts in under half a second and
scrolls through millions of rows.

![Data viewer](docs/screenshots/data-viewer.png)

## Features

- **Connections** started from a grid of database logos, with TLS (libpq `sslmode` semantics), passwords in the system keychain,
  colors and a read-only flag; paste a `postgres://`, `mysql://`, `sqlserver://`, `clickhouse://`, `libsql://`
  or `mongodb://` URL to fill the form.
  ClickHouse connects to its HTTP interface (port 8123, or 8443 with `sslmode=require`) and
  reads results as Arrow; its tables are browsed and queried, not edited in the grid.
  **SSH tunnels** with a password, a key or the SSH agent, optionally through a jump host;
  server keys are checked against `~/.ssh/known_hosts`. Connection **types** Development,
  Test and Production: production shows a PROD badge in red and opens in manual commit.
  **Import** connections from DBeaver, DataGrip, pgAdmin and `pg_service.conf`.
  Client certificates for PostgreSQL and MySQL; **network profiles** share one SSH tunnel
  and SSL setup between connections. Passwords from the keychain, asked every time, from
  `.pgpass` or printed by a shell command (a password manager's CLI); a **master password**
  can keep the saved ones in an encrypted file instead (Argon2id and AES-256-GCM). After
  connecting, a default schema, a time zone and bootstrap statements run on every
  connection and reconnection. Idle sessions are checked every minute: the status bar shows
  the latency, and a connection the server dropped is reopened.
- **MongoDB**: statements in mongosh syntax (`db.orders.find({ status: 'paid' }).sort({ placed_at: -1 }).limit(10)`,
  `aggregate`, `countDocuments`, `distinct`, `insertOne`/`insertMany`, `updateOne`/`updateMany`,
  `replaceOne`, `deleteOne`/`deleteMany`, `createIndex`, `db.runCommand`, `show dbs`, `show collections`,
  `use`), with the shell's values (`ObjectId(...)`, `ISODate(...)`, `NumberLong(...)`, `/regex/i`) and
  Extended JSON. Results are a grid of the top-level fields with the focused row's document as a tree
  beside it (F7); nested documents and arrays are JSON. Databases and collections in the navigator,
  fields sampled from the documents, indexes, and the collection's `createCollection`/`createIndex`
  script as its DDL. The editor completes collections after `db.`, their methods and the fields of the
  collection. `mongodb+srv://` and connection string options go in the Database field
  (`shop?authSource=admin&replicaSet=rs0`). No transactions, data editor or explain on MongoDB.
- **Navigator**: databases (PostgreSQL), schemas, tables, views, routines, triggers, types,
  extensions and events, loaded lazily, with a filter. Estimated rows and sizes of tables
  and indexes; PostgreSQL partitions under their table. Bookmarks, per-connection filters
  that hide schemas or objects by name (`tmp_*`), and a Simple view with only tables and
  views. The tree is cached on disk: it shows at once on start, and offline. Generate SELECT,
  INSERT, UPDATE, DELETE, UPSERT or DDL; rename, truncate and drop with a preview of the
  statement. A click selects a row (`Ctrl`/`Shift`+click for several, e.g. to generate the
  DDL of many tables at once), the arrow expands it, a double-click opens it (or goes back to
  its tab when already open). A green dot marks the connections with an open session; none
  connects before you open it.
- **Search**: object names, columns and the source of views, routines and triggers across a
  database; or the text of every row of a schema's tables, opening the matching rows.
- **Object details**: properties, columns, indexes, constraints, foreign keys, incoming
  references and the DDL of any object.
- **SQL editor** (CodeMirror): run the statement under the cursor (`Ctrl+Enter`) or the
  whole script (`Alt+X`), errors pointed out in the text, autocompletion of keywords,
  tables and columns (including `alias.column`), query history, `.sql` files.
  `JOIN` completes from foreign keys: `JOIN ` offers the related tables with alias and
  condition, `JOIN t ON ` offers the condition. A formatter (`Ctrl+Shift+F`) with
  configurable keyword case and indentation. Line commands, multiple cursors, and a
  context menu with every editor command.
  Parameters (`:id`, `?`) and variables (`${name}`) asked in a dialog and kept in a
  Variables panel; client commands `@set`, `@echo`, `@include file.sql` and
  `@export file.csv`. Built-in functions complete with signature and description; snippets
  (`sel` then `Tab`) are editable. Folding, an outline of the statements, `Ctrl+click` on a
  name to open its object, `Ctrl+\` to run into a new result tab, a terminal mode with
  results as text, and running the script with the local `psql`, `mysql`, `sqlcmd` or `sqlite3`.
  A linter marks unknown columns, joins without a condition and `SELECT *` on production;
  comments are spell checked.
- **Query plans**: `Ctrl+Shift+E` opens the plan of the statement under the cursor in its own
  tab, as a tree with costs, estimated rows and each step's details, as indented text or as
  a table. Explain analyze (editor menu) runs the statement for actual rows and times, then
  rolls back what it changed; tables that ignore transactions (MySQL's MyISAM) keep the
  changes. PostgreSQL, MySQL 8.0.16+ and MariaDB analyze; SQLite gives its query plan only.
  A heatmap tints each step by its share of the time it spends itself (of the cost, without
  analyze), and a badge marks the steps whose row estimate was 10 times off or more. A table
  read in full (10,000 rows or more) to keep the rows matching a condition gets a
  suggestion to index that condition's columns; on PostgreSQL with Explain analyze.
- **Query manager** (`Ctrl+Shift+M`): every statement, metadata request and commit or
  rollback the app sent to the databases, newest first, with its connection, duration, rows
  and outcome; filter by text, hide the metadata. It keeps the last 2000 in memory; scripts
  run with `psql`, `mysql` or `sqlite3` are not in it. A statement's trace splits its time
  into network (the connection's last keep-alive round trip), server until the first answer,
  and fetching the rows.
- **Slow queries**: statements slower than a threshold (Settings, Performance; 5 s by
  default) are marked in the editor's status bar and listed apart in the query manager.
- **Saved scripts**: `.sql` files per project in the Scripts panel under the navigator, with
  folders, rename, drag to move, and the connection each one opens on.
- **Visual query builder**: tables dragged onto a canvas, joined on their foreign keys,
  columns ticked; the generated SELECT opens in the editor.
- **Results grid** on canvas: rows are read a page at a time (1,000 ahead of what is on
  screen) and more as you scroll, with a loading bar; millions of rows, cancellable queries.
- **Data viewer**: open a table, sort by clicking a header, filter with a `WHERE`, count rows.
- **Data editor**: change cells in place (double-click, `F2` or just type), set NULL or the
  default, add, duplicate and delete rows, preview the generated SQL and save it all or
  nothing; in manual-commit mode the changes join the open transaction. Tables need a primary
  key or a unique key on non-null columns, or a virtual key you pick; otherwise the grid says
  why it is read-only. Results of a query that reads one table edit the same way, with
  computed columns read-only. Paste cells from a spreadsheet or CSV over the grid or as new
  rows; foreign keys show the name of the row they point to and edit with a list of those
  rows; PostgreSQL arrays and composite values edit one element or field per line, each
  with its own NULL; every save leaves an undo script that puts the rows back.
- **Results**: copy with headers or as CSV, JSON, Markdown, HTML, SQL `INSERT` or an `IN`
  list; find in the loaded rows (`Ctrl+F`); a value panel (`F7`) with long text, JSON and XML
  formatted, hex and images from binary columns, and save to file; quick filters on the
  focused value from the grid's menu (data viewer). Columns can be hidden, pinned to the
  left, dragged to another place and resized, and data viewer tables keep that layout. The
  totals of the selection (count, sum, average, minimum, maximum) show under the grid; a
  record view shows one row at a time; a panel counts the rows by value. Foreign keys open
  the row they point to, and the References panel lists the rows of other tables that point
  at the focused one. Side panels show the focused column's type, nullability and source
  table, and statistics of the selection (count, NULLs, distinct values, sum, average,
  extremes, median, standard deviation); two selected rows are compared column by column.
  A map panel draws PostGIS, MySQL and WKT geometries and PostgreSQL's geometric types,
  with zoom and pan. Read-only results can run again every few seconds, without filling the
  history. Dates, timestamps, the time zone, digit grouping and relative times follow the
  Settings.
- **Export** to CSV, JSON, NDJSON or Parquet, streamed straight to the file.
- **Transactions**: auto-commit or manual commit with Commit/Rollback; an open transaction
  shows how long it has been open and warns after a while (10 minutes by default).
- **Safety**: an `UPDATE` or `DELETE` without `WHERE`, or a `TRUNCATE`, asks first and says
  about how many rows it changes; on production connections every change is listed and
  confirmed before it runs, and the data editor shows its SQL before saving.
- **Appearance** in the Settings: the Platypus Dark and Platypus Light themes (what
  "System" follows), Classic Light and Dark, Nord, Dracula, Solarized Light and high
  contrast; your own accent, background, sidebar and text colors, interface size, grid row
  density, corner radius and the SQL editor's font size. Inter and JetBrains Mono are
  bundled, so the app looks the same everywhere and works offline.
- **Projects** as workspaces: a rail of colored squares on the left, one per project
  (a client, an environment, a side job). A project shows only its connections, its own tabs
  and its recent queries; the window title names it. A connection can be in several
  projects; `Ctrl+1` is All connections, `Ctrl+2`… the projects in rail order.
- **Command palette** (`Ctrl+K` or `Ctrl+Shift+P`): actions, connections, tables and recent
  queries, found by typing a few letters of them. In a project it lists only the project's.
- **Shell**: `Ctrl+Shift+T` reopens the last closed tab, the mouse wheel scrolls the tab bar,
  every shortcut can be changed in the Settings and the toolbar's buttons hidden or shown as
  icons only.
- Automatic reconnection, in-app updates.

| | |
|---|---|
| ![Autocomplete](docs/screenshots/autocomplete.png) | ![Transactions](docs/screenshots/transaction.png) |
| ![Platypus Light](docs/screenshots/light-theme.png) | ![Command palette](docs/screenshots/palette.png) |
| ![Projects](docs/screenshots/projects.png) | |

## Install

Download the installer for your system from the
[releases page](https://github.com/PaoloCisly/platypus-db/releases): `.exe`/`.msi` for
Windows, `.dmg` for macOS, `.AppImage`/`.deb`/`.rpm` for Linux. The app updates itself.

### Keyboard shortcuts

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

On macOS use `Cmd` instead of `Ctrl`. Every shortcut can be changed in Settings › Keyboard
shortcuts.

## Performance

| | PlatypusDB 0.1.0 | Target |
|---|---|---|
| Start to interactive window | 430–500 ms | < 500 ms |
| Memory at rest, 1 connection | about 130 MB | < 150 MB |
| Scrolling 1M rows | 178 fps | 60 fps |
| 5M rows to Parquet | 3.2 s, 31 MB peak | streaming |
| Installer | 5.4 MB | < 20 MB |

Windows 11, Ryzen 7 9700X; details and method in [docs/BENCHMARKS.md](docs/BENCHMARKS.md).

---

The rest of this file is for contributors. Scope and roadmap: [docs/MVP_PLAN.md](docs/MVP_PLAN.md);
releases: [docs/RELEASING.md](docs/RELEASING.md).


## Layout

```
crates/
  platypus-core/           driver traits, metadata types, result streams
  platypus-driver-pg/      PostgreSQL (tokio-postgres)
  platypus-driver-mysql/   MySQL / MariaDB (mysql_async)
  platypus-driver-mssql/   SQL Server / Azure SQL (tiberius, vendored with patches in vendor/)
  platypus-driver-sqlite/  SQLite (rusqlite)
  platypus-driver-duckdb/  DuckDB (duckdb, bundled; Arrow results as they come)
  platypus-driver-clickhouse/ ClickHouse over HTTP (reqwest; ArrowStream results decoded as they arrive)
  platypus-tls/            rustls client configuration with libpq `sslmode` semantics
  platypus-driver-libsql/  libSQL servers and Turso (Hrana over HTTP; SQLite's catalog code)
  platypus-driver-mongo/   MongoDB (official driver, rustls); mongosh parser, documents to Arrow
  platypus-sql/            dialect-aware lexer, statement splitting, read-only check, DML
  platypus-net/            SSH tunnels (russh)
  platypus-store/          connections (TOML + OS keychain), imports, query history (SQLite)
  platypus-export/         streaming CSV, JSON, NDJSON and Parquet writers
  platypus-conformance/    integration suite run on every driver
  platypus-cli/            `platypus` CLI for exercising the core
apps/desktop/              Tauri app: React UI in src/, Rust shell in src-tauri/
docker/                    dev databases with seed data
```

## Requirements

- Rust ≥ 1.99 (`rustup`)
- Node.js ≥ 24 (LTS, see `.nvmrc`)
- Tauri system prerequisites: <https://v2.tauri.app/start/prerequisites/>
  (Windows: MSVC Build Tools + WebView2; Linux: `libwebkit2gtk-4.1-dev` and friends)
- Docker, for the dev databases

## Development

What comes next is planned in [docs/ROADMAP.md](docs/ROADMAP.md); [docs/EXTENDING.md](docs/EXTENDING.md)
explains how features plug in (actions, tab kinds, settings, driver capabilities).

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

# end-to-end tests (Windows): drive the release build on a throwaway profile
cd apps/desktop
npm run tauri build -- --no-bundle
npm run e2e

# dev databases (PostgreSQL 17, MySQL 8.4, MariaDB 11.4, ClickHouse 26.3); SQL Server 2022 with
# its profile
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
