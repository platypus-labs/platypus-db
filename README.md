# PlatypusDB

Fast, lightweight desktop SQL client. Rust core, Tauri 2 + React UI.

> Status: **M5 — Data viewer, export, transactions**: tables open in a viewer with server-side sort, filter and row count; results export to CSV, JSON, NDJSON or Parquet in streaming (5M rows to Parquet in about 3 s with ~30 MB of memory); manual-commit mode with Commit/Rollback on all drivers. Built on M4 (navigator, MySQL/MariaDB and SQLite drivers, conformance suite), M3 (CodeMirror editor, script execution, history) and M2 (TLS connections, keychain, virtualized Arrow grid). See [docs/MVP_PLAN.md](docs/MVP_PLAN.md) for scope and roadmap.

## Layout

```
crates/
  platypus-core/           driver traits, metadata types, result streams
  platypus-driver-pg/      PostgreSQL (tokio-postgres)
  platypus-driver-mysql/   MySQL / MariaDB (mysql_async)
  platypus-driver-sqlite/  SQLite (rusqlite)
  platypus-sql/            dialect-aware lexer, statement splitting, read-only check
  platypus-store/          connections (TOML + OS keychain), query history (SQLite)
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
