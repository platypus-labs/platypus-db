# PlatypusDB

Fast, lightweight desktop SQL client. Rust core, Tauri 2 + React UI.

> Status: **M0 — foundations**. See [docs/MVP_PLAN.md](docs/MVP_PLAN.md) for scope and roadmap.

## Layout

```
crates/
  platypus-core/           driver traits, metadata types, result streams
  platypus-driver-pg/      PostgreSQL (tokio-postgres)
  platypus-driver-mysql/   MySQL / MariaDB (mysql_async)
  platypus-driver-sqlite/  SQLite (rusqlite)
  platypus-sql/            statement splitting, autocomplete
  platypus-store/          connection config, keychain, query history
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

# core crates
cargo test --workspace --exclude platypus-desktop
cargo run -p platypus-cli

# dev databases (PostgreSQL 17, MySQL 8.4, MariaDB 11.4)
docker compose -f docker/compose.yml up -d --wait
```

| Database   | Host port | User / password / db           |
|------------|-----------|--------------------------------|
| PostgreSQL | 55432     | `platypus` / `platypus` / `platypus` (schema `demo`) |
| MySQL      | 53306     | `platypus` / `platypus` / `platypus` |
| MariaDB    | 53307     | `platypus` / `platypus` / `platypus` |

Each database has `customers`, `orders`, a `customer_totals` view, a `type_zoo` table with one
column per notable type, and a 1M-row `big_table` for streaming and scrolling benchmarks.
These credentials are for local test containers only.
