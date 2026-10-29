# Changelog

All notable changes to PlatypusDB are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Databases**: SQL Server and Azure SQL, DuckDB (also opening CSV, JSON, Parquet and XLSX
  files as a database), ClickHouse, libSQL and Turso, MongoDB with a mongosh editor, Redis (commands in the editor, key types
  as tables edited in the grid), Oracle;
  CockroachDB, YugabyteDB and the other PostgreSQL-compatible servers; Snowflake, BigQuery,
  Databricks and Trino through ADBC drivers downloaded on first use; Db2 and any ODBC source on
  Windows. A picker with the database logos for new connections.
- **Connections**: new connections from a pasted URL or JDBC string and Copy as URL; DBeaver
  project import (`.dbp`) with folders, saved users and passwords (optional); connection and
  project folders, reordered by drag and drop; network profiles, session setup statements,
  password commands and `.pgpass`, client certificates, a master password for saved passwords,
  keep-alive with latency and reconnection.
- **Projects**: a project rail with project-scoped connections, tabs, navigator and history.
- **Navigator**: sizes and estimated rows, partitions, filters, bookmarks, metadata and data
  search, a disk cache of the metadata, actions on several objects.
- **SQL editor**: parameters and variables, client commands (`@set`, `@include`, `@export`…),
  snippets, a linter, saved scripts, a visual query builder, running a script in `psql`,
  `mysql` or `sqlite3`, a split editor.
- **Results and data**: results read in pages with a loading bar, editing of PostgreSQL arrays
  and composite values, column metadata and calc panels, row comparison, auto-refresh, a GIS
  viewer, mass edit of a selection, cells loaded from files.
- **Analysis**: charts of results with PNG and SVG export, column statistics under the headers,
  comparison with the previous run of a statement, Query with SQL on the results of a tab.
- **Query plans and performance**: explain as a tree, as text and as a table, a cost heatmap,
  index suggestions, a query manager with the trace of every statement and slow-query marks.
- **Import, export and transfer**: import of data files into tables (add, replace or upsert);
  export to XLSX, SQL, Markdown, HTML and XML, of several tables or a whole schema, compressed
  with gzip or zstd, uploaded to S3, GCS or an S3-compatible bucket; copying and migrating
  tables between databases; fake rows; pasting CSV as a new table.
- **Schema tools**: a graphical table editor with an ALTER preview.
- **Catalog and documentation**: notes and tags on objects, table and column comments, display
  columns for foreign keys, schema documentation exported as Markdown or HTML with ER diagrams.
- **Agents**: `platypus-mcp`, an MCP server on the saved connections, with agent access set per
  connection and agent queries marked in the history; `platypus` CLI on saved connections.
- **Interface**: a new look with the Platypus Dark and Light themes, bundled fonts and icons; a
  title bar framed by the connection type; pinned and grouped tabs, an activity log, zoom keys;
  a resizable navigator and sidebar; Settings in tabs with a search; Italian translation.

### Changed

- The data viewer sorts and filters by rewriting the statement that ran, instead of a separate
  `WHERE` bar.
- PlatypusDB is now licensed under AGPL-3.0-or-later.

### Fixed

- Generate data on a MongoDB collection ran SQL and failed: it now inserts documents with
  `insertMany`, with generators for ObjectIds, arrays and nested documents.
- Many fixes to completion, the data viewer's sorting and quick filters, the catalog cache,
  array display, imports from DBeaver and the HTML documentation export.

## [0.2.0] - 2026-10-24

### Added

- **Data editor**: edit cells in the data viewer, add, duplicate and delete rows, preview the
  SQL, save all or nothing.
- **Result grid**: copy with headers and "Copy as" CSV, JSON, Markdown, HTML or SQL; find in
  the results; a value panel for JSON, XML, hex and images; quick filters.
- **Navigator and objects**: object tabs with properties and generated SQL.
- **SQL editor**: a formatter, editor commands and JOIN completion from the foreign keys.
- **Connections**: SSH tunnels with keys, agent and jump hosts; connection types (development,
  test, production); import from DBeaver, DataGrip, pgAdmin and `pg_service.conf`.
- **Safety**: checks before risky statements and a clock for the open transaction.
- **Shell**: a command palette, configurable shortcuts and toolbar, appearance settings with
  theme presets and custom colors.

## [0.1.0] - 2026-10-23

First release: a desktop SQL client for PostgreSQL, MySQL, MariaDB and SQLite.

### Added

- Saved connections with TLS, a navigator of schemas and objects.
- SQL editor with statement splitting, completion of keywords, tables and columns, and a
  searchable query history.
- Data viewer on an Arrow grid with server-side sorting, several result sets per script, Fetch
  all and cancel.
- Transactions with autocommit, commit and rollback.
- Streaming export to CSV, JSON, NDJSON and Parquet.
- Dark and light themes, installers for Windows, macOS and Linux, and automatic updates.
- `platypus` CLI to run queries from the terminal.

[Unreleased]: https://github.com/PaoloCisly/platypus-db/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/PaoloCisly/platypus-db/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/PaoloCisly/platypus-db/releases/tag/v0.1.0
