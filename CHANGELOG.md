# Changelog

All notable changes to PlatypusDB are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.1] - 2026-11-13

### Added

- **Trademarks**: [TRADEMARKS.md](TRADEMARKS.md) says what anyone may do with the PlatypusDB name and
  logo, and [NOTICE](NOTICE) lists the logo and icon files, which stay outside the AGPL. The code's
  license does not change.
- **Agents**: `propose_edits` works on MongoDB collections. The agent names documents by `_id` (an
  `ObjectId` as its 24 hexadecimal digits, read by the type of the collection's `_id`); the collection
  opens in its data viewer with a `find` of those `_id`s and the changes pending, checked by the rules
  of the grid's save, and Save writes them as the grid does (all or nothing on a replica set). Views are
  refused.

### Security

- **SSH**: russh, the SSH library behind tunnels, is updated to 0.65, which fixes unbounded memory
  use on crafted server packets and two flaws of its agent server (RUSTSEC advisories of russh before
  0.65).
- **SQL Server**: TLS goes through rustls 0.23, like the other drivers, instead of rustls 0.21 and
  webpki 0.101, which had security advisories. Trusting the server certificate works as before.
- **Updates**: the updater plugin no longer pulls zip 4, and the Pageant client (SSH keys from
  PuTTY's agent) no longer buffers frames without bound.

### Fixed

- **Editor**: Explain plan and Explain analyze are no longer offered on databases whose driver has no
  query plans (ADBC, ODBC and Db2 among them); asked for before the session says so, they explain why
  instead of opening a tab that fails.
- **Elasticsearch**: testing an edited connection with the password field left empty uses the saved
  password even when only the index pattern changed, as saving it already did.

## [0.6.0] - 2026-11-08

### Changed

- **Agents**: the dialog that asks about an agent's change shows the statement in the editor's colors and,
  on SQL databases, laid out with its indentation, whatever single line the agent sent. Only spaces and
  line breaks can differ from what runs: anything else shows the statement as sent, which is also a click
  away.

### Added

- **MongoDB**: the grid edits the documents of a plain `find` on one collection (the data viewer, or a
  statement you type, with its `use` or `getSiblingDB` database and a projection that keeps `_id`), by
  `_id`. Each change is one `updateOne`, `insertOne` or `deleteOne`, shown by Preview SQL and written with
  the BSON type of the column (`int`, `long`, `double`, `decimal`, `bool`, `string`, `objectId`, `date`,
  `document`, `array`); a changed number keeps the numeric type the document stores (`int`, `long`,
  `double`, `decimal`) when that type holds it exactly, in a pipeline update. A change to `_id`, a column of mixed types or binary data, a field with a `.` or a
  leading `$`, and a value that does not fit are refused with the reason. A command that finds no
  document stops the save. On a replica set a save is all or nothing, in a transaction of its own or in
  the one the tab has open; on a standalone server the error says how many commands were saved. The undo
  script puts the documents back.
- **MongoDB**: the Auto-commit button is shown on every MongoDB connection, disabled with its reason on a
  standalone server; a production connection to a replica set opens in manual commit with Commit and
  Rollback, which were hidden before.
- **MongoDB**: manual commit on replica sets and sharded clusters. The reads and writes of documents,
  `createCollection` and `createIndex` run in a transaction of the connection's session, opened at the
  first change (reads before it see what is committed and open none), and so do the server commands that write documents (`runCommand({ insert: … })` and
  the like); metadata, server information and the navigator run outside it. What MongoDB does not run in
  a transaction (`drop`, `dropDatabase`, `renameCollection`, `dropIndex`, `createView`, `$out`, `$merge`)
  is refused in manual commit, and the transaction stays as it was. When the server ends a transaction
  (after `transactionLifetimeLimitSeconds`, 60 by default, or on a duplicate key or a write conflict),
  the error says its changes were discarded and the transaction shows as closed. A cancel rolls it back.
  A standalone server still has no transactions, and says why.
- **MongoDB**: `bulkWrite`, with `insertOne`, `updateOne`, `updateMany`, `replaceOne`, `deleteOne` and
  `deleteMany`, ordered or not. As in mongosh it is not atomic: what ran before a failure stays, and the
  error says how far it got. A `deleteMany` or `updateMany` with an empty filter inside it asks, and is
  refused to agents, as it is alone.
- **MongoDB**: the rest of mongosh's statements: `findOneAndUpdate`, `findOneAndReplace`,
  `findOneAndDelete`, `findAndModify`, `createIndexes`, `dropIndexes`, `hideIndex`, `stats`, `validate`, the
  size methods, `db.serverStatus`, `db.currentOp`, users and roles, the profiler, `rs.status`, `show users`
  and more, each in the editor's completions.
- **MongoDB**: query plans: `.explain()`, `db.c.explain().find(…)` and the editor's **Explain**, shown as a
  tree like every other database's. A plan never applies a write.

### Fixed

- **MongoDB**: options were read and dropped. `arrayFilters`, `collation`, `hint` and `let` on an update or
  a delete, `ordered` on `insertMany`, and `hint`, `collation` and `maxTimeMS` on a read now reach the
  server. An update without its `arrayFilters`, or a filter without its `collation`, could change other
  documents than the ones asked for. An option not known is refused by name.
- **MongoDB**: `updateOne` with `{ multi: true }` changed every matching document; it is now refused, as
  `multi` belongs to the old `update()`.
- **MongoDB**: a long beyond 2^53 was written as `NumberLong(9007199254740993)`, which reads back as a
  double and loses digits (generated data, copied values); it is now `NumberLong("…")`.
- **MongoDB**: the data viewer matched a collection's name ignoring case, so `db.Orders.find()` could be
  taken for `orders`; MongoDB's names are case-sensitive, and so is the match now.
- **MongoDB**: an `ISODate("…")` or a date cell with a character beyond ASCII in its first characters
  crashed the statement's reader; it is now a syntax error.

## [0.5.0] - 2026-11-06

### Added

- **Plugins**: ABI 2 (`api = 2`; plugins of ABI 1 work as before). A manifest can declare event
  hooks, tabs, side panels, grid extensions, a theme with no module and a settings page, with the
  new permissions `hooks`, `tabs` and `grid` shown in plain words at install; whatever it does not
  know is refused by name, and an update lists what it asks anew.
- **Plugins**: event hooks, before and after a statement, on connect and before saving edited rows.
  A "before" hook can block with a message, after the Safety checks, which it cannot lift, and cannot
  change the statement. It sees the statement with passwords and every string and number masked,
  and never a value of a result; on drivers of plugins it gets no text. Hooks also run for scheduled
  scripts, statement by statement, and `after_query` hears of a database's own client and of `EXPLAIN
  ANALYZE`; agents, exports and the rest they do not cover are listed in `docs/user/PLUGINS.md`. A hook
  that crashes or hangs does not stop anything: the action goes ahead and the user is told once, as
  they are when `plugins.json` cannot be read. Without hooks a statement pays one atomic read.
- **Plugins**: a settings page per plugin from its manifest (text, number, boolean, choice, secret),
  saved in the data folder, secrets in the system's keychain and never in a file, read by the module
  through a host function limited to its own values.
- **Plugins**: tabs and side panels with an HTML page of the plugin. A page runs in a sandboxed frame
  on an opaque origin, with a strict policy of its own and no network, and cannot reach the app's
  DOM, globals or storage. It talks to the app through a short list of requests, each checked for its
  permission, size and rate in the page and the backend; none runs SQL or reads connections, passwords
  or rows. A page that leaves its document, floods the app or stops responding is stopped and the user
  told. On WebKit (macOS, Linux) a page that loops forever still freezes the window: see the plugin
  documentation.
- **Plugins**: computed columns, cell renderers and cell commands in the result grid, in the WebAssembly
  sandbox with fuel and time limits and no network. They read a masked cell as dots, and a renderer
  answers text and colours, never HTML. A column that fails shows its error in its cells. A frame never
  calls a module: answers are cached and fetched in batches, away from drawing, and without an extension
  the grid is as it was.
- **Plugins**: themes as plugins, a package with no code that adds a card to the theme picker. Colours
  are checked against the app's tokens and must be colours, nothing that could reach a resource or
  leave its declaration; fonts come only from the package; poor contrast gets a warning. A theme
  whose plugin is gone falls back to the default, and a theme not chosen costs nothing.
- **Plugins**: a catalog (Settings › Plugin catalog) from an index you choose: no address is built in
  and nothing is contacted until you set one, and it is read only when you open the page or ask. A
  package passes HTTPS, size, SHA-256, a minisign signature by a key you trust, zip paths, identity
  and downgrade checks, then the review of a manual install. A package with no valid signature or
  an older version installs only from its own question. An index on the network reaches neither this
  computer nor a private network, however the address is written and whatever a name resolves to. A
  plugin's signer is pinned: another signer is said in the review, and installing clears its secrets
  and what its pages keep.
- **Plugins**: removing a plugin asks whether to keep its settings, and always clears its secrets from
  the keychain; what cannot be cleared is said, and the rest still is. A plugin whose files are gone
  stays listed, and can be removed.
- **Plugins**: an `id` or `version` that is not the same folder on every system (a trailing dot, a device
  name of Windows, a file of the plugins' folder) is refused, and a plugin's files must be the ones its
  entry in the list names. An update keeps a plugin turned off, a same-version reinstall that fails
  leaves the old files, and a setting that changes kind leaves nothing behind.
- **Plugins**: a grid extension reads sensitive cells as dots whether or not the screen masks them. A
  plugin turned off or removed stops in every window at once. A `.zip`'s hidden files are skipped, as a
  folder's are, and an updated theme loads its new fonts at once.

### Changed

- **Settings**: the Plugins page is loaded when it opens.
- **Connections**: PostgreSQL signs in with a Kerberos ticket, under Password from: on Linux from
  `kinit` or a keytab (`KRB5CCNAME`, `KRB5_CLIENT_KTNAME`), on Windows the session's (SSPI). A field
  sets the service name, `postgres` by default. No password is asked, saved or sent, and a server
  that does not ask for Kerberos is refused. Without a ticket, the test says what to run. `psql`,
  `pg_dump` and `pg_restore` sign in the same way. Not in the macOS builds yet. A real ticket has
  logged in from Linux against a test KDC; the login through SSPI has not been tried with one.

### Fixed

- **Connections**: selected connections can be dragged into a folder and deleted together; before, only
  the one under the pointer moved, and there was no way to delete several.
- **Connections**: folders of connections stay as you left them, open or closed, after a restart.
- **Connections**: Test connection on an edited connection says why it needs the password. A saved
  password still goes only to the server it was saved for: on another server the form now asks for it,
  instead of failing with "password missing" beside a field that says it is saved. Another database of
  the same PostgreSQL or MySQL server, or another way of signing in to the same SSH server, no longer
  counts as another server.
- **Grid**: a selection of more than 20,000 rows shows **Save files to a folder…** greyed out, saying why,
  instead of losing the item. A text cell that holds a file shows in the menu and the value panel only,
  as the guide now says.
- **Import**: a Db2 connection by alias (`jdbc:db2:ALIAS`) says it has no host, so it is filled in before
  connecting.
- **Agents**: when the connection of a proposed change to data cannot be opened, the message names the
  agent and the table.
- **Updates**: `latest.json` points Windows at the NSIS installer, as the app installs.
- **Guide**: Back returns to where the reader was on the page. A line under a list item with no block of its
  own continues the item, as on GitHub.

## [0.4.2] - 2026-11-05

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
  also in the repository, in `docs/user/guide/`. It loads the first time it is opened, not at each start.

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
- **Security**: an agent with file access can no longer lift the masking by editing the rules in
  `settings.toml` or the sensitive marks in `catalog.db`. Rules and marks are sealed with the rest
  of the agent policy: one removed or narrowed outside the app waits until you accept it in Settings
  › Agents › Security, and one added applies at once. A seal that no longer matches masks every
  value for agents until you answer. This does not stop an agent that can run commands as your
  user, which can read the database by other routes or the keychain (see `docs/user/AGENTS.md`).

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

[Unreleased]: https://github.com/platypus-labs/platypus-db/compare/v0.6.1...HEAD
[0.6.1]: https://github.com/platypus-labs/platypus-db/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/platypus-labs/platypus-db/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/platypus-labs/platypus-db/compare/v0.4.2...v0.5.0
[0.4.2]: https://github.com/platypus-labs/platypus-db/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/platypus-labs/platypus-db/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/platypus-labs/platypus-db/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/platypus-labs/platypus-db/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/platypus-labs/platypus-db/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/platypus-labs/platypus-db/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/platypus-labs/platypus-db/releases/tag/v0.1.0
