# PlatypusDB

Fast, lightweight desktop SQL client. Rust core, Tauri 2 + React UI.

> Status: **M3 — SQL editor**: CodeMirror editor, statement/script execution with dialect-aware splitting, query history, restored workspace. Built on M2 (PostgreSQL connections with TLS and keychain, virtualized Arrow grid). See [docs/MVP_PLAN.md](docs/MVP_PLAN.md) for scope and roadmap.

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

# core crates (PostgreSQL integration tests skip if the dev database is down;
# set PLATYPUS_TEST_PG_URL to make them mandatory)
cargo test --workspace --exclude platypus-desktop

# CLI: table (first 200 rows), csv or ndjson; Ctrl+C or --timeout cancel server-side
export PLATYPUS_URL="postgres://platypus:platypus@localhost:55432/platypus?sslmode=disable"
cargo run -p platypus-cli -- query "select * from demo.type_zoo"
cargo run --release -p platypus-cli -- query -f csv -o big.csv --stats "select * from demo.big_table"

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
