# Changelog

All notable changes to PlatypusDB are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Grid**: a cell that holds a file offers to save it, in any type of column: binary, or text with a
  `data:` URL, base64 or hex. A binary cell shows the type and size (`PDF · 2.3 MB`), and a
  selection saves its files to a folder.
- **Agents**: Settings › Agents › Activity has a summary per agent and per session (calls, tables
  read, rows returned, changes allowed and refused, errors), folded above the call list, that
  follows its filters and exports as Markdown or HTML next to the CSV. Calls from before this
  version show –. It loads when opened.
- **Help**: the top bar's "?" and the command palette open a user guide that comes with the app and
  reads offline, with a page per database, what is new in this version and a way to report a bug.
  The connection form, Settings › Agents and Settings › Safety link to their page. The guide is
  also in the repository, in `docs/guide/`. It loads the first time it is opened, not at each start.

### Changed

- **Agents**: sensitive columns read `••••••` in `run_query`, `sample_rows` and `get_user_context`,
  in the CLI too, as the grid shows them while it masks. Settings › Agents › Security can show them
  on one connection.
- **Agents**: the `get_er_diagram` tool draws a schema, or the tables near some tables, as a Mermaid
  `erDiagram`, or with `format: "svg"` as the image the diagram's export makes, drawn by the open
  app (which asks you nothing and shows nothing). An image over the byte limit is refused, not cut.
- **Agents**: `describe_table`, `list_objects` and the table resource return the user's notes, tags
  and sensitive marks, and `search_objects` also finds objects by comment, note and `#tag`.
- **Agents**: Settings › Agents › Security limits rows, bytes, time and calls per minute per
  connection, and can refuse a read whose estimated rows or plan cost are too high, with the plan
  in the error. A limit raised by hand in the file waits for the user.
- **Agents**: new tools: `compare_schemas` (differences and migration script, nothing runs),
  `profile_table` (per column figures from a sample, counts only for sensitive columns),
  `estimate_change` (what a change would show the user, without running it), `server_activity` and
  `top_queries` (read-only, with the values in other sessions' statements masked).
- **Agents**: `propose_edits` opens rows to add, change or delete in the data viewer as pending edits, which the user saves or discards. `next_page` reads long results in pages, `query_across_databases` joins tables of several connections with sensitive columns copied as NULL, and MCP prompts and schema change notifications are offered.
- Choosing a database for a new connection offers its driver at once, when it needs one that is
  not installed; Settings is not needed. Cancelling leaves the form as it is.

### Fixed

- **Agents**: the first page of a long `run_query` result no longer waits for the whole read-ahead
  for `next_page`: the cursor keeps rows for at most 250 ms after the page is full, 10,000 rows or
  4 MB, and its last page says the rest was not kept and to narrow the query.
- **Agents**: while a change proposed by an agent waits to apply, its data viewer tab says
  "Applying the changes proposed by <agent>…" until the rows show as pending edits or the reason
  they could not be applied.
- **Security**: `profile_table` on MongoDB no longer shows sensitive fields inside a document. A
  mark on `user.password`, or a rule that matches the nested name `password`, now keeps those
  values out of the figures of the parent object or array (top values, minimum, maximum). `run_query`
  and `sample_rows` mask the same nested fields in a document or array column, at any depth.
- **Security**: `get_user_context` follows the user's statement to the columns it reads, as
  `run_query` does. `SELECT password AS p …` on the open tab no longer shares the value under the
  alias, and a column that only has a sensitive name, from a table with no mark, is no longer
  masked for nothing when the statement or the viewer's table says which table it is. A subquery
  that names a sensitive column, or a statement that does not parse and names one, masks the whole
  shared result.
- Snowflake, BigQuery, Databricks, Trino and other ADBC connections no longer offer to edit rows in
  the grid: a save needs a transaction, which they do not have. Agents can no longer propose rows
  to edit on them, or on ClickHouse.
- IBM Db2 connections are imported from DBeaver and DataGrip, and `jdbc:db2:` URLs are read.
- On Windows, saving connections or settings no longer fails with "Access denied" when another
  program holds the file for a moment, as antivirus scanners and backup tools do.
- **Security**: a connection URL pasted or typed over the one the URL field already shows no
  longer leaves its password in clear in `connections.toml`. A paste takes the field's place, and
  two URLs together are refused. Saving also refuses a name, database or user that holds a URL
  with its password. A connection saved this way before keeps the URL in its Database field:
  remove it there, and change that password if the file was shared.

## [0.4.1] - 2026-11-04

### Added

- **Connections**: a PostgreSQL or SQL Server connection that names its database shows only that
  one in the navigator, as in DBeaver; *Show all databases*, beside the Database field, lists
  the server's other databases too (imported from DBeaver's PostgreSQL setting).
- Shorter menus: a connection's, a schema's and the SQL editor's actions are grouped in
  submenus (*New*, *Open*, *Tools*, *Export*; *Data*, *Analyze*; *Run*, *Explain*).
- A field left empty or out of range shows the app's own message under it, in the interface's
  language, instead of the browser's bubble.

### Fixed

- **Ctrl+Enter** runs only the statement under the cursor when statements are apart by a blank
  line and have no `;`, instead of the whole script.
- A new SQL editor, or one showing a DDL, has no empty results panel: it appears with the first
  run.
- The first color swatch means no ring around the connection's logo, whatever its type.
- Scrolling the Settings no longer lags: dialogs are solid instead of blurred glass.
- Check boxes keep their size beside a long label and show the pointer; the Projects help of
  the connection dialog sits beside its title.
- The welcome screen no longer says to double-click a connection before the saved ones are read.

## [0.4.0] - 2026-11-02

### Added

- **Connections**: export connections to a `.platypusconn` file, with or without their projects
  and folders and, encrypted behind a password, their saved passwords; import it back from
  Import connections › PlatypusDB.
- The main window opens where it was left, on the same screen, or centred on the primary
  screen when that one is gone.
- **Usability review**: a title, axes and a line of explanation on every chart (histograms,
  profiles, a right-hand axis for series of different scales); a toast for every copy, save
  and export; a shared waiting indicator for slow actions; empty states that lead to the action
  that fills them; consistent spacing in forms and dialogs.
- Resizable side and detail panels (editor history, explain, administration, charts, agents,
  query builder), with their size remembered.
- Shorter menus: related items in submenus (a table's *Analyze* and *Copy*, the editor's
  *Lines*, *Cursors* and *Folding*), destructive actions last, and the app's menu everywhere
  instead of the browser's.
- The notebook shows the running cell, stops it, and runs all cells.
- The command palette finds saved scripts by name.
- The result grid's menu opens from the keyboard (menu key or Shift+F10).

### Fixed

- Numbers wider than 38 digits and SQL Server `time(7)` keep every digit in Query with SQL and
  queries across databases.
- CockroachDB tables without a primary key are edited by their hidden `rowid`.
- SQL Server gives a disabled constraint without a name one of its own, so it can be disabled.
- MariaDB schema compare reads and writes `PERIOD FOR` clauses.
- SQLite hides the shadow tables of FTS5 tables.
- On DuckDB an edited row of a table with a one-column primary key keeps its place after saving.
- Renaming or moving a saved script updates the editor tabs open on it, so saving writes to the
  new file.
- PostgreSQL: a server that accepts the connection and never answers ends the connect at the
  timeout instead of hanging, and the `connect_timeout` option of a saved connection is used.
- One Escape is enough before Tab leaves the SQL editor.
- The open-transaction label keeps its time and change count in a tight toolbar.
- The result's SQL filter shows its whole border; toasts stack closer together; the connection
  color is a ring around its logo; the top bar shows the app's icon.
- Submenus stay inside the window, also in a menu that scrolls.

## [0.3.0] - 2026-11-01

### Added

- **Databases**: Redis (commands in the editor, key types as tables edited in the grid, a
  `redis-cli` terminal), Elasticsearch and OpenSearch (console requests, SQL, paged searches,
  grid edits, index export), Cassandra and ScyllaDB over CQL; several versions of a downloaded
  driver, with activation and update lookup.
- **Schema tools**: the ER diagram full screen, with a minimap and an edit mode that draws the
  schema and generates the DDL; views, functions, triggers and sequences created from a form;
  data comparison of two tables with the script that syncs them; schema comparison written as a
  Flyway migration or a Liquibase changelog; schema history with snapshots and the changes
  between two of them; migration status of a project folder (Flyway, Liquibase, Prisma, Alembic,
  Rails).
- **Server administration**: server parameters and a server dashboard; users, roles and
  privileges with create, edit, drop, grant and revoke; a PL/pgSQL debugger.
- **Analysis**: pivot of results, profile of a whole table with patterns, outliers and
  duplicate rows, queries across databases, saved results as Parquet files opened offline,
  dashboards of widgets from saved scripts, computed columns in the result grid.
- **Import, export and transfer**: saved transfer profiles, run by hand or from a scheduled
  task; bulk loading with the SQL Server bulk copy and the DuckDB appender.
- **Collaboration**: Git status, diff, commit, pull and push for saved scripts; queries shared
  as a link or a `.platypus` file; encrypted folder sync; shared team projects, whose init
  statements need approval before they run.
- **Plugins**: sandboxed WebAssembly plugins for actions and drivers, in Settings > Plugins.
- **Catalog**: sensitive columns, from name and type rules or the user's mark, masked in
  exports and for agents; where a column is used (views, routines, triggers, saved scripts).
- **Agents**: schemas and tables as MCP resources, read-only tools over saved scripts and query
  history, the user's context and active tab shared on consent, a ready-made plugin and Codex
  configuration, client cards with logos in Settings > Agents.
- **Interface**: a redesign of the welcome page, the connection dialog, Settings and the object
  pages; a choice of where the navigator, results, tab bar and side panels go; Lucide, Tabler or
  Phosphor icons; animations off or reduced; Settings pages in sub-tabs; Enter confirms every
  dialog; driver logos in the navigator; recent scripts on the welcome page.
- **Distribution**: winget, Homebrew, AUR and Flatpak manifests filled from a release; updates
  left to the package manager that installed the app.

### Fixed

- Over 230 fixes from the final testing: two rounds of testers by area and on every database, a
  code review and a security review. Among them, each profile keeps its sync passphrase and
  transfer keys under its own keychain name.
- Drags inside the app use pointer events, so they work on Windows; projects reorder in the rail.
- Snapshot restore quotes names and restores rows in order; SQL files count their statements
  exactly; a script run on several connections merges its results.

## [0.2.1] - 2026-10-29

### Added

- **Databases**: SQL Server and Azure SQL, DuckDB (also opening CSV, JSON, Parquet and XLSX
  files as a database, named in-memory databases, the library downloaded on first use),
  ClickHouse, libSQL and Turso, MongoDB with a mongosh editor, Oracle;
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
- **More connection options**: `${env:NAME}` and `.env` files in connection settings; SOCKS5
  and HTTP proxies; commands before connecting and after disconnecting; passwords from
  1Password, Bitwarden, Vault and AWS Secrets Manager; cloud tokens and Windows sign-in; port
  forwards through kubectl or AWS SSM; the pooler mode of PostgreSQL connections (Supabase,
  Neon); a test of every connection of a project; restrictions per connection (no exports, no
  data editing). Errors name the server's product and version, and a server older than the
  oldest version tested says so.
- **Server administration**: a Server activity tab with sessions, waiting chains and locks, to
  cancel or end a session; the costliest statements, table statistics and maintenance; backup
  and restore with `pg_dump`, `pg_restore`, `mysqldump` and `mysql`; scheduled scripts, exports
  and backups.
- **Safety**: pending changes, read-only tabs and an automatic `LIMIT`; snapshots of the rows an
  `UPDATE` or `DELETE` changes, with their restore scripts; a transaction log of each session;
  the app locked after idle minutes; sensitive columns masked for screen sharing.
- **Editor and results**: Vim mode; a script run on several connections at once; conditional
  formatting of cells and rows; JSON in text columns shown indented; notebooks of SQL and
  Markdown cells.
- **Query plans**: pasted plans of PostgreSQL, CockroachDB, MySQL, MariaDB, SQLite and SQL
  Server; two plans compared step by step; a graph view.
- **More import, export and schema tools**: bulk loading with `COPY` and `LOAD DATA`; SQL files
  of any size run, a schema dumped to an SQL file; an ER diagram, a schema linter and schema
  comparison.
- **More for agents**: a bridge with the open app, which confirms agents' changes; the
  `explain`, `sample_rows` and `propose_sql` tools; a call log of every tool call; one-click
  install of the server in the MCP clients; `platypus-db --mcp` serving MCP from the app.
- **More interface**: tabs and the quick query in windows of their own, a tray icon; panel
  sizes kept for the next start; a sample database on the welcome screen; portable mode;
  Spanish, German, French, Portuguese and Chinese; themes exported and imported as JSON; a
  rotated log file and a diagnostic package.

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

[Unreleased]: https://github.com/PaoloCisly/platypus-db/compare/v0.4.1...HEAD
[0.4.1]: https://github.com/PaoloCisly/platypus-db/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/PaoloCisly/platypus-db/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/PaoloCisly/platypus-db/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/PaoloCisly/platypus-db/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/PaoloCisly/platypus-db/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/PaoloCisly/platypus-db/releases/tag/v0.1.0
