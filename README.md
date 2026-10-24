# PlatypusDB

Fast, lightweight desktop SQL client for **PostgreSQL**, **MySQL/MariaDB** and **SQLite**.
Rust core, Tauri 2 + React UI: a 5 MB installer that starts in under half a second and
scrolls through millions of rows.

![Data viewer](docs/screenshots/data-viewer.png)

## Features

- **Connections** with TLS (libpq `sslmode` semantics), passwords in the system keychain,
  colors and a read-only flag; paste a `postgres://` or `mysql://` URL to fill the form.
  **SSH tunnels** with a password, a key or the SSH agent, optionally through a jump host;
  server keys are checked against `~/.ssh/known_hosts`. Connection **types** Development,
  Test and Production: production shows a PROD badge in red and opens in manual commit.
  **Import** connections from DBeaver, DataGrip, pgAdmin and `pg_service.conf`.
- **Navigator**: databases (PostgreSQL), schemas, tables, views, routines, triggers, types,
  extensions and events, loaded lazily, with a filter. Generate SELECT, INSERT, UPDATE,
  DELETE, UPSERT or DDL; rename, truncate and drop with a preview of the statement.
- **Object details**: properties, columns, indexes, constraints, foreign keys, incoming
  references and the DDL of any object.
- **SQL editor** (CodeMirror): run the statement under the cursor (`Ctrl+Enter`) or the
  whole script (`Alt+X`), errors pointed out in the text, autocompletion of keywords,
  tables and columns (including `alias.column`), query history, `.sql` files.
  `JOIN` completes from foreign keys: `JOIN ` offers the related tables with alias and
  condition, `JOIN t ON ` offers the condition. A formatter (`Ctrl+Shift+F`) with
  configurable keyword case and indentation. Line commands, multiple cursors, and a
  context menu with every editor command.
- **Results grid** on canvas: millions of rows fetched on demand, cancellable queries.
- **Data viewer**: open a table, sort by clicking a header, filter with a `WHERE`, count rows.
- **Data editor**: change cells in place (double-click, `F2` or just type), set NULL or the
  default, add, duplicate and delete rows, preview the generated SQL and save it all or
  nothing; in manual-commit mode the changes join the open transaction. Tables need a primary
  key or a unique key on non-null columns, or a virtual key you pick; otherwise the grid says
  why it is read-only. Results of a query that reads one table edit the same way, with
  computed columns read-only. Paste cells from a spreadsheet or CSV over the grid or as new
  rows; foreign keys show the name of the row they point to and edit with a list of those
  rows; every save leaves an undo script that puts the rows back.
- **Results**: copy with headers or as CSV, JSON, Markdown, HTML, SQL `INSERT` or an `IN`
  list; find in the loaded rows (`Ctrl+F`); a value panel (`F7`) with long text, JSON and XML
  formatted, hex and images from binary columns, and save to file; quick filters on the
  focused value from the grid's menu (data viewer). Columns can be hidden, pinned to the
  left, dragged to another place and resized, and data viewer tables keep that layout. The
  totals of the selection (count, sum, average, minimum, maximum) show under the grid; a
  record view shows one row at a time; a panel counts the rows by value. Foreign keys open
  the row they point to, and the References panel lists the rows of other tables that point
  at the focused one. Dates, timestamps, the time zone, digit grouping and relative times
  follow the Settings.
- **Export** to CSV, JSON, NDJSON or Parquet, streamed straight to the file.
- **Transactions**: auto-commit or manual commit with Commit/Rollback; an open transaction
  shows how long it has been open and warns after a while (10 minutes by default).
- **Safety**: an `UPDATE` or `DELETE` without `WHERE`, or a `TRUNCATE`, asks first and says
  about how many rows it changes; on production connections every change is listed and
  confirmed before it runs, and the data editor shows its SQL before saving.
- **Appearance** in the Settings: themes (system, light, dark, Nord, Dracula, Solarized
  Light, high contrast), your own accent, background, sidebar and text colors, interface
  size, grid row density, corner radius and the SQL editor's font size.
- **Command palette** (`Ctrl+K` or `Ctrl+Shift+P`): actions, connections, tables and recent
  queries, found by typing a few letters of them.
- **Shell**: `Ctrl+Shift+T` reopens the last closed tab, the mouse wheel scrolls the tab bar,
  every shortcut can be changed in the Settings and the toolbar's buttons hidden or shown as
  icons only.
- Automatic reconnection, in-app updates.

| | |
|---|---|
| ![Autocomplete](docs/screenshots/autocomplete.png) | ![Transactions](docs/screenshots/transaction.png) |

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
  platypus-driver-sqlite/  SQLite (rusqlite)
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
# PLATYPUS_TEST_MYSQL_URL or PLATYPUS_TEST_MARIADB_URL to make them mandatory
cargo test --workspace --exclude platypus-desktop

# CLI: table (first 200 rows), csv, ndjson, json or parquet (json and parquet need -o); Ctrl+C or --timeout cancel server-side
export PLATYPUS_URL="postgres://platypus:platypus@localhost:55432/platypus?sslmode=disable"
cargo run -p platypus-cli -- query "select * from demo.type_zoo"
cargo run --release -p platypus-cli -- query -f csv -o big.csv --stats "select * from demo.big_table"
cargo run --release -p platypus-cli -- query -f parquet -o big.parquet --stats "select * from demo.big_table"
cargo run -p platypus-cli -- query -u mysql://platypus:platypus@localhost:53306/platypus "select * from type_zoo"
cargo run -p platypus-cli -- query -u sqlite:///C:/data/app.db "select * from sqlite_master"

# end-to-end tests (Windows): drive the release build on a throwaway profile
cd apps/desktop
npm run tauri build -- --no-bundle
npm run e2e

# dev databases (PostgreSQL 17, MySQL 8.4, MariaDB 11.4)
docker compose -f docker/compose.yml up -d --wait
```

| Database   | Host port | User / password / db           |
|------------|-----------|--------------------------------|
| PostgreSQL | 55432     | `platypus` / `platypus` / `platypus` (schema `demo`, TLS with a self-signed cert) |
| MySQL      | 53306     | `platypus` / `platypus` / `platypus` |
| MariaDB    | 53307     | `platypus` / `platypus` / `platypus` |

Each database has `customers`, `orders`, a `customer_totals` view, a `type_zoo` table with one
column per notable type, and a 1M-row `big_table` for streaming and scrolling benchmarks.
These credentials are for local test containers only.
