[← README](../README.md)

# Development

What comes next is planned in [ROADMAP.md](ROADMAP.md), and the working rules are in
[CONTRIBUTING.md](../CONTRIBUTING.md). [EXTENDING.md](EXTENDING.md) explains how features
plug in (actions, tab kinds, settings, driver capabilities); [RELEASING.md](RELEASING.md)
how releases are made, and [MVP_PLAN.md](MVP_PLAN.md) the scope of the first release.
All the documents are listed in [the documentation index](README.md), and the changes of each
release in [CHANGELOG.md](../CHANGELOG.md).

## Layout

```
crates/
  platypus-core/           driver traits, metadata types, result streams
  platypus-driver-pg/      PostgreSQL (tokio-postgres)
  platypus-driver-mysql/   MySQL / MariaDB (mysql_async)
  platypus-driver-mssql/   SQL Server / Azure SQL (tiberius, vendored with patches in vendor/)
  platypus-driver-sqlite/  SQLite (rusqlite)
  platypus-driver-duckdb/  DuckDB (duckdb, library downloaded on first use; Arrow results as they come)
  platypus-duckdb-loader/  loads DuckDB's library for the duckdb crate (the only unsafe code)
  platypus-driver-clickhouse/ ClickHouse over HTTP (reqwest; ArrowStream results decoded as they arrive)
  platypus-driver-libsql/  libSQL servers and Turso (Hrana over HTTP; SQLite's catalog code)
  platypus-driver-mongo/   MongoDB (official driver, rustls); mongosh parser, documents to Arrow
  platypus-driver-redis/   Redis (`redis` crate, RESP3, rustls); commands, key types as tables, edits
  platypus-driver-oracle/  Oracle (oracle crate on Oracle Instant Client)
  platypus-driver-adbc/    Snowflake, BigQuery, Databricks, Trino and any ADBC driver, loaded at run time
  platypus-driver-odbc/    Db2 and any ODBC driver or DSN, through the system's driver manager (Windows)
  platypus-drivers/        drivers downloaded on first use: the dbc registry, Instant Client, IBM's ODBC driver, DuckDB
  platypus-tls/            rustls client configuration with libpq `sslmode` semantics
  platypus-sql/            dialect-aware lexer, statement splitting, read-only check, DML
  platypus-schema/         table designs, the DDL between them, schema linter and comparison
  platypus-net/            SSH tunnels (russh), proxies, cloud port forwards and tokens, commands around a connection
  platypus-store/          connections (TOML + OS keychain), imports, query history and catalog notes (SQLite)
  platypus-transfer/       streaming exports, imports, copies between databases, fake rows
  platypus-docs/           schema documentation: Markdown, HTML and Mermaid ER diagrams
  platypus-local/          analysis of results in DuckDB: column profiles, charts, run diffs
  platypus-conformance/    integration suite run on every driver
  platypus-cli/            `platypus` CLI for exercising the core, and for agents (`--connection`)
  platypus-mcp/            `platypus-mcp`, the MCP server for agents (stdio, rmcp)
apps/desktop/              Tauri app: React UI in src/, Rust shell in src-tauri/
docker/                    dev databases with seed data
samples/                   the SQLite sample database (wildlife.sql)
scripts/                   build-windows.ps1 (installers; build-windows.cmd runs it), sample-db.mjs,
                           bench.py (benchmarks with thresholds), clean-target.mjs
vendor/                    tiberius, tokio-postgres and scylla, patched for their drivers
site/                      static presentation page
```

## Requirements

- Rust ≥ 1.99 (`rustup`)
- Node.js ≥ 24 (LTS, see `.nvmrc`)
- Tauri system prerequisites: <https://v2.tauri.app/start/prerequisites/>
  (Windows: MSVC Build Tools + WebView2; Linux: `libwebkit2gtk-4.1-dev` and friends)
- Docker, for the dev databases

## Build, run and test

```bash
# desktop app with hot reload
cd apps/desktop
npm install
npm run tauri dev

# core crates; the conformance suite (crates/platypus-conformance) runs the same scenarios on
# every driver. Server databases that are down are skipped; set PLATYPUS_TEST_PG_URL,
# PLATYPUS_TEST_MYSQL_URL, PLATYPUS_TEST_MARIADB_URL, PLATYPUS_TEST_MSSQL_URL,
# PLATYPUS_TEST_CLICKHOUSE_URL, PLATYPUS_TEST_LIBSQL_URL, PLATYPUS_TEST_MONGO, PLATYPUS_TEST_REDIS or
# PLATYPUS_TEST_SEARCH to make them mandatory. Elasticsearch and OpenSearch are in the `search` profile
# (a JVM each, 512 MB of heap): `docker compose -f docker/compose.yml up -d --wait elasticsearch`
# (default URL elasticsearch://elastic:platypus@localhost:59200); PLATYPUS_TEST_SEARCH=opensearch://localhost:59201
# runs the same tests on the `opensearch` service
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

# scale: 10,000 tables, a million rows to files, cells of tens of MB (prints the timings)
cargo test -p platypus-conformance --test scale -- --nocapture

# benchmarks with thresholds (scripts/bench-thresholds.json); `app` needs an X display on Linux
cargo bench -p platypus-driver-pg --bench decode && python3 scripts/bench.py decode
xvfb-run -a -s "-screen 0 1600x1000x24" python3 scripts/bench.py app target/release/platypus-db
# the same start and memory of another client, for docs/BENCHMARKS.md "Side by side" (no limits)
xvfb-run -a -s "-screen 0 1600x1000x24" python3 scripts/bench.py other dbeaver ~/dbeaver/dbeaver

# frees the space `target` gathers: incremental caches, builds unused for 7 days (--days N)
node scripts/clean-target.mjs

# MCP server for agents on its own, for development (the app serves it with --mcp)
cargo build --release -p platypus-mcp

# end-to-end tests (Windows): drive the release build on a throwaway profile
cd apps/desktop
npm run tauri build -- --no-bundle
npm run e2e

# the README's screenshots, from the release build: readme-sqlite and readme-duckdb make their
# own databases, readme-postgres needs the dev PostgreSQL
PLATYPUS_README_SHOTS=../../docs/screenshots npm run e2e -- readme

# the README's GIFs, the same way: gifs-sqlite makes its database, gifs-postgres needs the dev
# PostgreSQL; PLATYPUS_GIF_FRAMES=1 also writes a few frames of each as PNG, to look at
PLATYPUS_README_GIFS=../../docs/screenshots npm run e2e -- gifs
```

In VS Code, **Run PlatypusDB** (Ctrl+Shift+B) installs the frontend dependencies if they are
missing, builds the sample database and starts `tauri dev`. `.vscode/tasks.json` also has
tasks to start and stop the dev databases.

## Sample database

`node scripts/sample-db.mjs` builds `samples/wildlife.db` from
[samples/wildlife.sql](../samples/wildlife.sql): a SQLite platypus field survey with regions,
rivers, researchers, tagged animals, surveys, about 15,000 sightings and a `platypus_stats`
view, with foreign keys between all of them. Open it with a new SQLite connection, or from the
welcome screen, which builds it with the scripts in `samples/scripts/`.

## Dev databases

```bash
# PostgreSQL 17, MySQL 8.4, MariaDB 11.4, ClickHouse 26.3 and Redis Stack 7.4; the others with
# their profile
docker compose -f docker/compose.yml up -d --wait
docker compose -f docker/compose.yml --profile sqlserver up -d --wait
docker compose -f docker/compose.yml --profile libsql up -d --wait
docker compose -f docker/compose.yml --profile mongo up -d --wait
# Older versions (PostgreSQL 12/14/16, MySQL 5.7/8.0, MariaDB 10.6, SQL Server 2017, MongoDB 5),
# then the conformance suite on each
docker compose -f docker/compose.yml --profile versions up -d --wait
node scripts/conformance-versions.mjs
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
| Redis      | 56379     | default user, password `platypus` (Redis Stack, with RedisJSON; the tests fill their own databases) |

Each database has `customers`, `orders`, a `customer_totals` view, a `type_zoo` table with one
column per notable type, and a 1M-row `big_table` for streaming and scrolling benchmarks.
These credentials are for local test containers only.
