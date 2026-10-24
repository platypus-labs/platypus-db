# PlatypusDB

Fast, lightweight desktop SQL client for **PostgreSQL**, **MySQL/MariaDB** and **SQLite**.
Rust core, Tauri 2 + React UI: a 5 MB installer that starts in under half a second and
scrolls through millions of rows.

![Data viewer](docs/screenshots/data-viewer.png)

## Features

- **Connections** with TLS (libpq `sslmode` semantics), passwords in the system keychain,
  colors and a read-only flag; paste a `postgres://` or `mysql://` URL to fill the form.
- **Navigator**: schemas, tables, views, routines, columns, indexes and foreign keys, loaded
  lazily, with a filter.
- **SQL editor** (CodeMirror): run the statement under the cursor (`Ctrl+Enter`) or the
  whole script (`Alt+X`), errors pointed out in the text, autocompletion of keywords,
  tables and columns (including `alias.column`), query history, `.sql` files.
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
- **Export** to CSV, JSON, NDJSON or Parquet, streamed straight to the file.
- **Transactions**: auto-commit or manual commit with Commit/Rollback.
- Light and dark theme, automatic reconnection, in-app updates.

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
| `Ctrl+F` | Find and replace |
| `Ctrl+S`, `Ctrl+O` | Save / open a `.sql` file |
| `Ctrl+T`, `Ctrl+W` | New tab / close tab |
| `Ctrl+Tab`, `Ctrl+Shift+Tab` | Next / previous tab |
| `Ctrl+,` | Settings |
| `F2`, `Enter`, typing | Edit the cell (data viewer, one-table queries) |
| `Insert`, `Ctrl+D`, `Delete` | Add, duplicate, delete rows |
| `Shift+Delete` | Set the selected cells to NULL |
| `Ctrl+V`, `Ctrl+Shift+V` | Paste over the cells / as new rows |
| `Ctrl+S`, `Ctrl+Z` | Save / undo data changes (grid focused) |

On macOS use `Cmd` instead of `Ctrl`.

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
  platypus-store/          connections (TOML + OS keychain), query history (SQLite)
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
