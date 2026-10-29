<h1 align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/brand/platypus-db-on-dark.png" />
    <img src="docs/brand/platypus-db-on-light.png" alt="PlatypusDB" width="640" />
  </picture>
</h1>

<p align="center">
  <strong>A fast, small SQL client for people who work with many databases.</strong>
</p>

<p align="center">
  <a href="https://github.com/PaoloCisly/platypus-db/releases/latest"><img src="https://img.shields.io/github/v/release/PaoloCisly/platypus-db" alt="Latest release" /></a>
  <a href="https://github.com/PaoloCisly/platypus-db/releases"><img src="https://img.shields.io/github/downloads/PaoloCisly/platypus-db/total" alt="Downloads" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0--or--later-blue" alt="License: AGPL-3.0-or-later" /></a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey" alt="Platforms: Windows, macOS, Linux" />
</p>

<p align="center">
  <b>English</b> · <a href="README.it.md">Italiano</a>
</p>

PlatypusDB is a desktop SQL client for developers, DBAs and analysts who work with several
databases in a day. One small app runs queries, browses and edits data, designs tables,
explains slow statements and documents schemas, and keeps production connections clearly apart
from the others. A Rust core with a Tauri 2 + React interface: an installer of about 5 MB, a
window ready in under half a second, and a results grid that scrolls through millions of rows.

![A million rows opened from the navigator, scrolled and sorted from the header](docs/screenshots/hero.gif)

## Why PlatypusDB

- **Light.** About 20 MB to download on Linux against 122 MB for DBeaver, 346 MB for
  Beekeeper Studio and about 1 GB for DataGrip; a window in under 500 ms and about 130 MB of memory with one connection.
- **Safe with production.** Connection types color the window frame, risky statements ask
  first, production opens in manual commit, and every save of edited rows leaves an undo script.
- **Many databases, one app.** PostgreSQL, MySQL, MariaDB, SQL Server, Oracle, ClickHouse,
  SQLite, DuckDB, MongoDB, Snowflake, BigQuery and more, with the same editor and grid.
- **Ready for agents.** Claude Code, Codex, Cursor and other MCP clients use your saved
  connections with the access you give each one, and their changes wait for your OK.
- **Open source.** AGPL-3.0, no account, no telemetry.

## Install

Download the installer for your system from the
[releases page](https://github.com/PaoloCisly/platypus-db/releases/latest): `.exe`/`.msi` for
Windows, `.dmg` for macOS, `.AppImage`/`.deb`/`.rpm` for Linux. The app updates itself.

1. Open PlatypusDB and click **New connection**, or paste a connection URL
   (`postgres://…`, `mysql://…`, a JDBC URL or an ADO.NET string).
2. No database at hand? **Try the sample database** on the welcome screen: a SQLite field
   survey of platypuses, with linked tables, a view and saved scripts.
3. Press `Ctrl+K` for the command palette: every action, connection and table is a few letters
   away.

**Portable copy**: put the executable in a folder with an empty `data` folder beside it.
Connections, settings, history and scripts then live in `data`, and the folder can move to
another machine or a USB stick ([more](docs/FEATURES.md#portable-copy)).

## Databases

| Database | How it connects | Notes |
|---|---|---|
| PostgreSQL 12+ | native protocol | Several databases per connection, partitions, PostGIS, arrays and composite types |
| MySQL 5.7+, MariaDB 10.6+ | native protocol | `EXPLAIN ANALYZE` needs MySQL 8.0.18 |
| SQL Server 2017+, Azure SQL | TDS | Windows (SSPI) and Microsoft Entra ID sign-in |
| Oracle | Oracle Instant Client, downloaded on first use | PL/SQL blocks, real cancel |
| ClickHouse | HTTP, Arrow results | No data editing in the grid |
| SQLite, DuckDB | embedded | DuckDB also opens CSV, JSON, Parquet and Excel files as databases |
| libSQL / Turso | Hrana over HTTP | |
| MongoDB 5+ | official driver | mongosh syntax; no transactions, explain or data editing |
| CockroachDB, YugabyteDB, TimescaleDB, Supabase, Neon, AlloyDB | PostgreSQL driver | Profiles with their defaults |
| Snowflake, BigQuery, Databricks, Trino, any ADBC driver | ADBC, downloaded on first use | Queries, results, navigator and completion; no data editing or explain yet |
| IBM Db2, any ODBC driver or DSN | ODBC (Windows) | |

The versions are the oldest the conformance suite runs on. More on each database in
[docs/FEATURES.md](docs/FEATURES.md#databases).

## Features

| Write SQL | Edit data safely |
|---|---|
| ![A JOIN completed from the foreign key, run and drawn as a chart](docs/screenshots/editor.gif) | ![Cells changed and a row added, the SQL previewed, then saved](docs/screenshots/data-editor.gif) |
| **Find slow steps** | **Browse the data** |
| ![Explain analyze with the heatmap and an index suggestion](docs/screenshots/explain.gif) | ![The data viewer sorted from its header, with the value panel](docs/screenshots/data-viewer.png) |
| **Design tables** | **Find anything** |
| ![Designing a table](docs/screenshots/table-editor.png) | ![The command palette](docs/screenshots/palette.png) |

- **Connections**: SSH tunnels, proxies, Kubernetes and AWS port forwards, cloud sign-in
  (AWS, Google Cloud, Entra ID), passwords from the keychain or a password manager,
  `${env:NAME}` variables, import from DBeaver, DataGrip and pgAdmin.
- **SQL editor**: completion of tables, columns and aliases in scope and of JOINs from the
  foreign keys, a formatter, parameters, snippets, a linter, Vim mode, a split editor, saved
  scripts and runs on several connections at once.
- **Data viewer and editor**: a canvas grid that reads rows a page at a time; sort and filter
  from the header, a value panel for JSON, XML and images; edit cells in place, preview the SQL
  and save all or nothing.
- **Analysis**: charts, column statistics, SQL on a result's rows, comparison with the
  previous run, and **notebooks** of SQL and Markdown cells that export to HTML.
- **Schema tools**: a table editor that shows its `ALTER TABLE` first, ER diagrams, a schema
  check, schema comparison with the migration script, a visual query builder, notes and tags,
  and documentation as Markdown or HTML.
- **Performance**: query plans as a tree, a graph or a table, with a heatmap, index
  suggestions and plan comparison; a query manager with the time of every statement.
- **Server activity**: sessions, locks, the costliest statements, table and index health,
  maintenance, backups and scheduled tasks.
- **Import, export and transfer**: CSV, JSON, Parquet, Excel and more, copies of tables and
  whole schemas between any two databases, fake data.
- **Workspace**: projects, pinned and grouped tabs, tabs in their own window, a quick-query
  window from the tray, themes, and seven languages (English, Italian, Spanish, German, French,
  Portuguese and Chinese).

Every feature, area by area: **[docs/FEATURES.md](docs/FEATURES.md)**. Keyboard shortcuts:
[docs/SHORTCUTS.md](docs/SHORTCUTS.md).

## Agents (MCP) and the CLI

Claude Code, Codex, Cursor and other MCP clients use your saved connections through
PlatypusDB's MCP server. Each connection gives agents no access, read only (the default) or
read and write; passwords never leave the keychain, and a change an agent wants to run opens a
dialog in the app first. Settings → Agents → Setup registers the server in your agents with one
click.

```bash
claude mcp add platypus -- "C:\Program Files\PlatypusDB\platypus-db.exe" --mcp
```

Tools, limits, the HTTP bridge and the `platypus` CLI: [docs/AGENTS.md](docs/AGENTS.md).

## Performance

| | PlatypusDB 0.1.0 | Target |
|---|---|---|
| Start to interactive window | 430–500 ms | < 500 ms |
| Memory at rest, 1 connection | about 130 MB | < 150 MB |
| Scrolling 1M rows | 178 fps | 60 fps |
| 5M rows to Parquet | 3.2 s, 31 MB peak | streaming |
| Installer | 5.4 MB | < 20 MB |

| | PlatypusDB | DBeaver CE | DataGrip | TablePlus | Beekeeper Studio |
|---|---|---|---|---|---|
| Runtime | Rust and the system's web view | Java, JRE bundled | Java, JetBrains Runtime bundled | Native (GTK on Linux) | Electron |
| Linux download | 19.5 MiB | 122 MiB | 1,047 MiB | 60 MiB | 346 MiB |
| On disk | 46 MiB | 206 MiB | not measured | 180 MiB | 1,105 MiB |
| License | AGPL-3.0 | Apache 2.0 | Proprietary | Proprietary | GPLv3 |

Windows 11, Ryzen 7 9700X. Sizes of the official Linux downloads of PlatypusDB 0.2.0, DBeaver CE
26.2.3, DataGrip 2026.2.6, TablePlus 2.2.0 and Beekeeper Studio 6.2.0; start and memory side by side are still to be measured.
Method, scale tests and the first Linux numbers in [docs/BENCHMARKS.md](docs/BENCHMARKS.md).

## Development

Build, test, project layout and the dev databases: [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).
What comes next is in [docs/ROADMAP.md](docs/ROADMAP.md); every document is listed in
[docs/README.md](docs/README.md), and the changes of each release in
[CHANGELOG.md](CHANGELOG.md).

## Contributing

Bug reports, ideas and pull requests are welcome: open an
[issue](https://github.com/PaoloCisly/platypus-db/issues) first for anything larger than a small
fix. [CONTRIBUTING.md](CONTRIBUTING.md) explains how to build, test and submit a change. Everyone
taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## Security

Please report vulnerabilities privately, not in a public issue: see [SECURITY.md](SECURITY.md).

## License

PlatypusDB is licensed under the [GNU Affero General Public License v3.0 or later](LICENSE)
(AGPL-3.0-or-later). You can use, study, change and share it freely; if you distribute it, or
run a changed version as a service for others, you must release your source code under the
same license. Third-party code and assets keep their own licenses:
see [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
