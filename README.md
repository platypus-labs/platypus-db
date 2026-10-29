<h1 align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/brand/platypus-db-on-dark.png" />
    <img src="docs/brand/platypus-db-on-light.png" alt="PlatypusDB" width="640" />
  </picture>
</h1>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0--or--later-blue" alt="License: AGPL-3.0-or-later" /></a>
  <a href="https://github.com/PaoloCisly/platypus-db/releases/latest"><img src="https://img.shields.io/badge/release-v0.2.0-green" alt="Latest release: v0.2.0" /></a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey" alt="Platforms: Windows, macOS, Linux" />
</p>

PlatypusDB is a desktop SQL client for developers, DBAs and analysts who work with several
databases in a day. One small app runs queries, browses and edits data, designs tables,
explains slow statements and documents schemas, and keeps production connections clearly apart
from the others. It runs on Windows, macOS and Linux: a Rust core with a Tauri 2 + React
interface, an installer of about 5 MB, a window ready in under half a second and a results grid
that scrolls through millions of rows.

It works with **PostgreSQL**, **MySQL**, **MariaDB**, **SQL Server** (and Azure SQL),
**Oracle**, **ClickHouse**, **SQLite**, **DuckDB**, **libSQL/Turso** and **MongoDB**; with the
servers that speak the PostgreSQL protocol: **CockroachDB**, **YugabyteDB**, **TimescaleDB**,
**Supabase**, **Neon** and **AlloyDB**; with **Snowflake**, **BigQuery**, **Databricks** and
**Trino** through ADBC; and on Windows with **IBM Db2** and any ODBC data source.

![The data viewer sorted from its header, with the value panel](docs/screenshots/data-viewer.png)

- [At a glance](#at-a-glance)
- [Install](#install)
- [Databases](#databases)
- [Features](#features)
- [Agents (MCP) and the CLI](#agents-mcp-and-the-cli)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Performance](#performance)
- [Development](#development)
- [Contributing](#contributing)
- [Security](#security)
- [License](#license)

## At a glance

- **Browse data**: a canvas grid that reads rows a page at a time. A click on a header sorts on
  the server and writes the `ORDER BY` in the editor; quick filters, a value panel for JSON,
  XML and images, foreign keys shown by the name of the row they point to.
- **Edit data safely**: change cells in place, preview the SQL, save all or nothing, and get an
  undo script for every save.
- **Write SQL**: completion of the tables, columns and aliases in scope and of JOINs from the
  foreign keys, a formatter, parameters, snippets, a linter, a split editor and saved scripts.
- **Know the schema**: object details, local notes and tags on tables and columns (`#pii` in the
  navigator's filter), the database's own comments, and documentation as Markdown or a single
  HTML file with an ER diagram.
- **Design and tune**: a table editor that shows its `ALTER TABLE` first, ER diagrams, a schema
  check, a schema comparison that writes the migration script, a visual query builder, query plans with a heatmap and index suggestions, and a query manager with the time
  of every statement.
- **Move data around**: import and export CSV, JSON, Parquet, Excel and more, copy tables and
  whole schemas between any two databases, generate fake rows.
- **Production-aware**: connection types with the color of the window frame, confirmations
  before risky statements, SSH tunnels, keychain or encrypted passwords.
- **Organized**: projects in a rail, tabs that pin and group, a command palette, Settings in
  tabs with a search, dark and light themes, English and Italian.
- **Agent-ready**: Claude Code, Codex, Cursor and other MCP clients can use your saved
  connections, with the access you give each one.

## Install

Download the installer for your system from the
[releases page](https://github.com/PaoloCisly/platypus-db/releases): `.exe`/`.msi` for
Windows, `.dmg` for macOS, `.AppImage`/`.deb`/`.rpm` for Linux. The app updates itself.

No database at hand? **Try the sample database** on the welcome screen: a SQLite field survey
of platypuses, with linked tables, a view and four saved scripts, made in the app's data folder
at the first click.

**Portable copy**: put the executable in a folder with an empty `data` folder beside it, as
with VS Code. Connections, settings, history, scripts, logs and the web view's storage then
live in `data`, and the folder can move to another machine or a USB stick. Saved passwords stay
in the system's keychain unless a master password keeps them in the folder too. A portable
copy updates by replacing its executable; the welcome screen says `portable` beside the version.

## Databases

| Database | How it connects | Notes |
|---|---|---|
| PostgreSQL 12+ | native protocol | Several databases per connection, partitions, PostGIS, arrays and composite types |
| MySQL 5.7+, MariaDB 10.6+ | native protocol | On MySQL before 8.0.16 plans are read from `EXPLAIN FORMAT=JSON`; `EXPLAIN ANALYZE` needs 8.0.18 |
| SQL Server 2017+, Azure SQL | TDS | `TOP (n)`, `[names]`, several result sets |
| ClickHouse | HTTP interface (8123, or 8443 with TLS), Arrow results | Browsed, queried and designed; no data editing in the grid |
| SQLite | embedded | |
| DuckDB | embedded, Arrow results; DuckDB's library downloaded on first use | Also opens CSV, JSON, Parquet and Excel files as databases |
| libSQL / Turso | Hrana over HTTP | Token in the password or in `authToken` |
| MongoDB 5+ | official driver | mongosh syntax; no transactions, explain or data editing |
| CockroachDB, YugabyteDB, TimescaleDB, Supabase, Neon, AlloyDB | PostgreSQL driver | Profiles with their default port, user, database and SSL |
| Snowflake, BigQuery, Databricks, Trino | ADBC driver, downloaded on first use | Queries, results, navigator and autocompletion; no data editing, explain or manual commit yet |
| Any database with an ADBC driver | ADBC driver library you choose | Its URI and options as the driver documents them |
| Oracle | native driver with Oracle Instant Client, downloaded on first use | PL/SQL blocks (end them with a `/` line in scripts), real cancel, navigator, DDL; Windows needs the Visual C++ Redistributable, Linux `libaio` |
| IBM Db2 | IBM's ODBC driver, installed on first use (Windows) | Registering the driver asks for administrator rights (UAC) |
| Any ODBC driver or DSN (Windows) | the system's ODBC driver manager | A DSN, or a driver and its connection string |

The versions in the table are the oldest the conformance suite runs on; an older server still
connects, with a message that some features may not work. Errors name the server they come from,
e.g. `Unknown column 'x' in 'field list' (MySQL 5.7.44)`, since the reason often depends on
the version.

The ADBC drivers come from the [dbc driver registry](https://docs.columnar.tech/dbc/) (Columnar):
the first connection that needs one asks to download it, checks its SHA-256 and keeps it with
the app's data. Oracle Instant Client and IBM's ODBC driver come from Oracle's and IBM's sites
in the same way, with their license shown first. *Settings › Drivers* lists and removes them,
and shows the ODBC drivers and data sources of the system.

## Features

### Connections

![Choosing the database for a new connection](docs/screenshots/connections.png)

- A new connection starts from a grid of database logos, with a search on top (`Enter` takes
  the first match); the `+` of the navigator opens it, and so does a right-click on a connection
  or below the list. Or paste a connection under the logos: a URL (`postgres://`,
  `mysql://`, `sqlserver://`, `clickhouse://`, `libsql://`, `mongodb://`), a JDBC URL or an
  ADO.NET / ODBC string (`Server=tcp:db,1433;Database=shop;User Id=sa;…`), and the form opens
  filled in for that database. The driver is set by the choice: *Back* returns to the logos, and a
  URL pasted in the form for another database offers to go back. Windows logins and SQL Server
  named instances without a port are pointed out. *Copy as URL* in a connection's menu copies
  its URL without the password; *Duplicate* (also in the command palette) saves a copy with its
  tunnel, init script and passwords, and opens it for editing.
- **Environment variables**: any field of a connection, the password and the SSH tunnel's
  included, can read `${env:NAME}`: from the app's environment, then from the connection's
  `.env` file (*Variables file* in its dialog). The connection keeps the reference, not the
  value; a variable that is not set says which. `$${` writes a plain `${`.
- **Poolers**: a PostgreSQL connection behind PgBouncer, Supavisor or Neon's pooler in
  *transaction mode* runs its statements unnamed, so that each one keeps to the server
  session of its transaction. The dialog suggests the mode for Supabase's pooler on port
  6543 and for Neon's `-pooler` hosts; `?pgbouncer=true` in a pasted URL sets it too.
- **Import** connections from DBeaver, DataGrip, pgAdmin and `pg_service.conf`, with their
  folders. A DBeaver project export (`.dbp`) brings its projects too, optionally gathered in a
  folder of the project rail. From DBeaver (a `.dbp` or a workspace) the saved user names and
  passwords come along too, SSH tunnel ones included: the passwords go to the keychain (or the
  master-password vault) like typed ones. Untick *Import saved users and passwords* to bring
  only the user names. Passwords DBeaver protects with its own master password cannot be read;
  those connections ask for theirs on first use, as do the ones of the other tools.
- **Folders**: connections inside a project go in nested folders (drag them, or *Move to
  folder*); projects in the rail go in folders too, by dropping one project on another.
  Connections and folders keep the order you give them: drag one above or below another, or
  press Alt+Up / Alt+Down; a connection dropped onto another makes a new folder of the two.
- **TLS** with libpq `sslmode` semantics, custom CAs, and client certificates for PostgreSQL and
  MySQL.
- **SSH tunnels** with a password, a key or the SSH agent, optionally through a jump host;
  server keys are checked against `~/.ssh/known_hosts`. **Network profiles** share one tunnel
  and SSL setup between connections.
- **Proxies**: a SOCKS5 or HTTP proxy (`socks5://user:password@host:1080`, `http://host:3128`)
  in front of the database, or of the SSH server when there is a tunnel. SOCKS5 resolves the
  server's name on the proxy.
- **Commands around a connection**, e.g. to bring a VPN up and down: one before the first
  session that needs it, which waits for it (a minute at most) and fails with it, and one
  after the last of those sessions closes. They run in the system's shell, whose `${VAR}`
  they keep.
- **Passwords** from the system keychain, asked every time, read from `.pgpass` or printed by a
  shell command. **Password managers**: pick 1Password, Bitwarden, HashiCorp Vault or AWS
  Secrets Manager and give the secret's reference; the app runs the manager's CLI, which must
  be installed and signed in. A command that prints a JSON secret gives its `password` field.
  A **master password** can keep the saved ones in an encrypted file instead (Argon2id and
  AES-256-GCM).
- **Test connections**: *Test connections* in a project's menu, in the navigator's menu or in
  the command palette tries every connection of the project, four at a time, and lists how each
  went.
- **Connection types** Development, Test and Production, with colors and a read-only flag.
  Production shows a red PROD badge and opens in manual commit.
- After connecting, a default schema, a time zone and bootstrap statements run on every
  connection and reconnection.
- Idle sessions are checked every minute: the status bar shows the latency, and a connection
  the server dropped is reopened, before the next statement too. Editing a connection reopens
  the sessions of its open tabs with the new settings; a tab with an open transaction asks first.

### Navigator and objects

- Databases (PostgreSQL), schemas, tables, views, routines, triggers, types, extensions and
  events, loaded lazily, with a filter. PostgreSQL partitions sit under their table.
- Estimated rows and sizes of tables and indexes.
- Bookmarks, per-connection filters that hide schemas or objects by name (`tmp_*`), and a
  Simple view with only tables and views.
- Large schemas stay quick: with 10,000 tables the tree opens in a fraction of a second and
  renders only the rows in view.
- The tree is cached on disk: it shows at once on start, and offline. Nothing connects until
  you open it; a green dot marks the connections with an open session.
- Generate SELECT, INSERT, UPDATE, DELETE, UPSERT or DDL; rename, truncate and drop with a
  preview of the statement. `Ctrl`/`Shift`+click selects several objects, for example to get
  the DDL of many tables at once.
- **Object details**: properties, columns, indexes, constraints, foreign keys, incoming
  references and the DDL of any object.
- **Search** object names, columns and the source of views, routines and triggers across a
  database; or the text of every row of a schema's tables, opening the matching rows.

### Notes, comments and schema documentation

![The comments, notes and tags of a table and its columns](docs/screenshots/catalog.png)

- **Notes and tags** on tables, views and columns, kept on your computer and never written to
  the database: in the object tab, for the object and for each of its columns. The navigator
  marks the annotated objects and shows the note on hover, and `#pii` in its filter keeps the
  objects tagged `pii`. The data viewer marks the annotated columns in its headers, and
  completion shows the note and tags of what it offers. They follow the renames made in the
  app.
- **Database comments** show beside the notes. Those of tables and their columns edit in the
  object tab, with the statement shown first; those of views are read-only. SQLite, libSQL and
  MongoDB have none.
- **Row label**: the column that names a table's rows where foreign keys point to it, picked in
  the object tab or from a header's menu in the grid.
- **Schema documentation**: right-click a schema, *Export documentation…*, for a Markdown file
  or a single HTML file (it works offline) with the columns, keys, indexes, comments, notes,
  tags and an ER diagram of each schema of up to 100 tables.

![The exported HTML documentation with its ER diagram](docs/screenshots/schema-docs.png)

### SQL editor

![JOIN completion from the foreign keys](docs/screenshots/autocomplete.png)

- Run the statement under the cursor (`Ctrl+Enter`) or the whole script (`Alt+X`); errors are
  pointed out in the text.
- **Completion** of keywords, tables, columns and built-in functions with their signature. The
  columns offered are those of the tables the statement under the cursor reads (even when FROM
  comes later), and `alias.` completes the columns of a table, a CTE or a subquery. A column
  that several of those tables have goes in with its alias, and names that need quotes are
  quoted on insert. `JOIN ` offers the related tables with alias and condition from the
  foreign keys; `JOIN t ON ` offers the condition. Tab or Enter accepts; Enter after a word
  typed exactly as the entry starts a new line. While a snippet's fields are active, Tab moves
  to the next field.
- **Formatter** (`Ctrl+Shift+F`) with configurable keyword case and indentation; the same
  setting gives the case of completed keywords.
- Line commands, multiple cursors, folding, an outline of the statements, and `Ctrl+click` on a
  name to open its object.
- **Parameters** (`:id`, `?`) and **variables** (`${name}`) asked in a dialog and kept in a
  Variables panel; client commands `@set`, `@echo`, `@include file.sql` and
  `@export file.csv`.
- Editable snippets (`sel` then `Tab`), query history, `.sql` files.
- `Ctrl+\` runs into a new result tab; a terminal mode shows results as text; a script can run
  with the local `psql`, `mysql`, `sqlcmd` or `sqlite3`.
- **Run on connections** (editor menu): the script, or the selection, runs on each connection
  chosen (e.g. one database per customer), four at a time, with each connection's outcome;
  the last result with rows of each comes together in a new tab, with a `connection` column.
  Changes are asked once, then by each connection's Safety checks; read-only connections are
  skipped.
- **Vim mode** (Settings › SQL editor): normal, insert and visual modes, the mode shown under
  the text, `:w` to save the script. Vim's Ctrl keys (`Ctrl+W`, `Ctrl+O`, `Ctrl+F`…) are Vim's
  while the editor has the focus; the app's other shortcuts keep working.
- A **linter** marks unknown columns, joins without a condition and `SELECT *` on production;
  comments are spell checked.
- **Saved scripts**: `.sql` files per project in the Scripts panel, with folders, drag to move,
  and the connection each one opens on.

### Results and the data viewer

- The grid is drawn on a canvas and reads rows a page at a time (1,000 ahead of what is on
  screen); queries can be cancelled.
- Open a table to browse it: its `SELECT` goes in the editor, where a `WHERE` typed by hand
  filters it.
- **Sort from the header**: a click on a column header writes the `ORDER BY` of the statement
  in the editor and runs it again; a second click reverses the order, a third puts back the
  query's own `ORDER BY`, or removes the sort when it had none. It works on the results of a
  query typed in the editor too, when it is one statement that only reads.
- **Quick filters** on the focused value (equal, not equal, in the selected values, NULL or
  not) go in the editor's `WHERE`; a renamed column is filtered on the column it shows, a
  computed one is filtered in the SQL; *Count rows* counts the rows of the query in the editor.
- **Copy** with headers or as CSV, JSON, Markdown, HTML, SQL `INSERT` or an `IN` list.
- **Find** in the loaded rows (`Ctrl+F`).
- **Value panel** (`F7`): long text, JSON and XML formatted, hex and images from binary
  columns, save to file. JSON in a text column opens indented too, unless the connection's
  *Format JSON found in text columns* is off.
- Columns hidden, pinned, moved and resized, with the layout kept per table.
- **Conditional formatting** (grid menu): rules color a cell or the whole row when a column
  is equal to a value, contains it, falls in a range or is NULL, or spread a color scale over
  a number column; the first rule that matches wins. A table keeps its rules.
- Totals of the selection (count, sum, average, minimum, maximum), a record view of one row
  at a time, counts by value, statistics of the selection, and two rows compared column by
  column.
- Foreign keys open the row they point to, and the References panel lists the rows of other
  tables that point at the focused one.
- A **map panel** draws PostGIS, MySQL and WKT geometries and PostgreSQL's geometric types.
- Read-only results can refresh every few seconds. Dates, time zone, digit grouping and
  relative times follow the Settings.

### Data analysis

- **Charts**: a result switches between the grid and a chart (bars, lines, scatter, pie) from
  the status bar. The columns and the aggregation are chosen beside the chart and kept with
  the tab for each statement; the chart exports as PNG or SVG. It is drawn from the rows
  loaded, with at most 2,000 points per series and 50 categories (the rest in *Other*).
- **Column statistics**: resting the mouse on a column header shows its NULLs, distinct
  values, minimum and maximum, and a small histogram (numbers and dates) or the most frequent
  values (text), over the rows loaded.
- **SQL on the result**: the field in the bar above a result's grid runs SQL on the rows
  loaded, not on the database. A clause or a condition applies to the result
  (`WHERE code LIKE 'B%'`, `ORDER BY name DESC`, `GROUP BY code`, `code = 'X'`), or a whole
  query reads the table `result` (and `r1`, `r2`, … for each result of the tab, to join
  them). Enter loads the rows in full, up to the memory limit, into an in-memory DuckDB and
  the grid shows the output read-only, with copy, export, charts and statistics; × or Escape
  returns to the result. Ctrl+Enter opens a larger editor, which can also open the query in a
  new tab. When the statement runs again, the next query loads the new rows.
- **Compare with previous run**: when a statement runs again, its result can be compared with
  the previous one: rows added, removed and changed (matched by the table's key when it has
  one), with the old value of each changed cell.

![A result as a bar chart](docs/screenshots/chart.png)

### Data editor

![Pending changes in the data editor](docs/screenshots/data-editor.png)

- Change cells in place (double-click, `F2` or just type), set NULL or the default, add,
  duplicate and delete rows. Changes stay highlighted until you save.
- **Preview the SQL** and save it all or nothing; in manual-commit mode the changes join the
  open transaction. Every save leaves an undo script that puts the rows back.
- Tables need a primary key or a unique key on non-null columns, or a virtual key you pick;
  otherwise the grid says why it is read-only. Results of a query that reads one table edit
  the same way, with computed columns read-only.
- Paste cells from a spreadsheet or CSV over the grid or as new rows.
- **Edit a whole selection** from the grid's menu (Edit selection): one value, NULL or DEFAULT
  in every cell, checked against each column's type; fill down from the first row
  (`Ctrl+Alt+D`, `Ctrl+Shift+D` on macOS); find and replace in the text cells, as typed or
  with a regular expression, with the count shown before applying (an expression too slow is
  stopped); a number sequence down each number column. Only the rows loaded in the grid
  change. Cells that cannot take a value are listed and left as they are; the rest become
  pending changes like typed ones, so you preview, save or undo them together.
- **Load a cell from a file** (grid menu or the cell editor's `…` button): bytes into binary
  columns, UTF-8 text into text columns; files over 50 MB ask first, files over 200 MB are
  refused.
- Foreign keys show the name of the row they point to and edit with a list of those rows; the
  column that gives that name is the table's row label.
- PostgreSQL arrays and composite values edit one element or field per line.

### Table editor

![Designing a table](docs/screenshots/table-editor.png)

Right-click a table, *Edit table…*, or a schema, *Create table…*: columns (type, NOT NULL,
default, primary key, comment), indexes, foreign keys with their `ON DELETE`/`ON UPDATE`
actions, and UNIQUE and CHECK constraints. Saving shows the `CREATE TABLE` or `ALTER TABLE`
script first and runs it in one transaction where the database can roll DDL back (MySQL
commits each statement). SQLite and DuckDB, which alter little in place, get the table
created again with its rows copied.

### ER diagrams, schema check and schema comparison

- **ER diagram**: right-click a schema, *ER diagram*, or some selected tables, *Show in ER
  diagram*. Tables are laid out from their foreign keys, right of the tables they reference;
  drag them where you want them, choose the tables with *Tables…*, add notes. The diagram is
  kept with its tab and exports to SVG or PNG. A schema of more than 100 tables asks which
  ones to show first.
- **Check schema…**: tables without a primary key, foreign keys without an index, duplicate
  indexes and indexes that another one starts with, foreign keys of another type than the
  column they reference. The `CREATE INDEX` or `DROP INDEX` that fixes a finding opens in the
  SQL editor, to review and run there.
- **Compare with…**: pick another schema of the same kind of database, on this connection or
  another one: the script that makes it like this one (tables created, altered and dropped,
  in an order the foreign keys allow) opens in the SQL editor of that connection. Tables and
  their parts are matched by name, so a renamed column is a dropped column and a new one.

### Visual query builder

![Two tables joined on their foreign key](docs/screenshots/query-builder.png)

Drag tables onto a canvas: they join on their foreign keys. Tick the columns, add a `WHERE`,
then run the SELECT or open it in the editor.

### Query plans and performance

![Explain analyze with the heatmap and an index suggestion](docs/screenshots/explain.png)

- `Ctrl+Shift+E` opens the plan of the statement under the cursor as a tree with costs,
  estimated rows and each step's details, as indented text or as a table.
- **Explain analyze** runs the statement for actual rows and times, then rolls back what it
  changed (tables that ignore transactions, like MySQL's MyISAM, keep the changes).
  PostgreSQL, MySQL 8.0.16+ and MariaDB analyze; SQLite gives its query plan only.
- A **heatmap** tints each step by its share of the time it spends itself, and a badge marks
  row estimates 10 times off or more.
- A table read in full (10,000 rows or more) to keep the rows matching a condition gets a
  suggestion to index that condition's columns.
- The **graph** view draws the plan as boxes joined by lines: each step tinted by its share
  of the work, each line as thick as the rows it carries; Fit shrinks a large plan to the
  view.
- **Compare…** lines a plan up with its previous run (after Refresh, e.g. once an index is
  added) or with another plan: the whole plan's time or cost before and after, and each
  step's own, with the steps replaced, added or gone.
- **Open query plan…** (command palette, or the connections list's menu) reads a plan pasted
  or opened from a file, from a server log or a colleague, without a connection:
  PostgreSQL's JSON or text (as psql prints it), CockroachDB's, MySQL's tree, MariaDB's JSON,
  SQLite's `QUERY PLAN` and SQL Server's `SHOWPLAN_TEXT`, recognized or chosen.

![The query manager with the trace of a statement](docs/screenshots/query-manager.png)

- **Query manager** (`Ctrl+Shift+M`): every statement, metadata request, commit and rollback
  the app sent, with connection, duration, rows and outcome; the last 2,000 are kept in
  memory. A statement's trace splits its time into network, server and fetch.
- Statements slower than a threshold (5 s by default) are marked in the status bar and listed
  apart.

### Server activity

- **Server activity…** on a PostgreSQL, MySQL, MariaDB or SQL Server connection lists the
  server's sessions: user, database, client, application, state, the statement running and
  for how long, what it waits for and which sessions it waits for. It refreshes every few
  seconds (or on demand), filters by any field, and opens a statement in the editor.
- **Cancel statement** stops the statement of another session and keeps the session
  (PostgreSQL, MySQL, MariaDB); **End session** closes it, rolling back its transaction.
  Both ask first, are refused on read-only connections, and are written to the query
  manager. The app's own session cannot be ended from there.
- **Locks** shows the waiting chains (who blocks whom, a deadlock cycle once) and the locks
  held or waited for, with their table where the server tells it. Seeing other users'
  sessions and locks needs the server's rights: `pg_read_all_stats` or superuser,
  MySQL's `PROCESS` and `SELECT` on `performance_schema`, SQL Server's
  `VIEW SERVER STATE`.

### Import, export and transfer

- **Export** to CSV, JSON, NDJSON, Parquet, Excel, SQL `INSERT`, Markdown, HTML or XML,
  streamed straight to the file; several tables or a whole schema at once. Optional gzip or
  zstd compression, and upload to a signed URL or to an S3, GCS (HMAC keys), R2 or MinIO
  bucket; the secret key is never saved.
- **Import** CSV, TSV, JSON, NDJSON, Parquet and Excel into a table, with a preview and a
  mapping of columns, adding the rows, replacing them or updating those with the same key; or
  into a new table created with the types DuckDB infers. *Paste CSV as a new table…* imports
  the clipboard.
- **Copy a table to another database** (PostgreSQL to MySQL, SQL Server to SQLite and every
  other pair), streamed, into a new table with the types mapped or into an existing one.
- **Copy a schema** to another database: each table with its columns, keys and identity
  column, then its defaults, indexes, foreign keys and CHECK constraints; between databases of
  one kind the views, functions, procedures and triggers too.
- **Fake data** for a table: names, emails, numbers, dates, enum values, foreign keys that
  point at existing rows, distinct values for unique columns.
- **Data files as databases**: drop a CSV, JSON, Parquet or Excel file on the window to query
  it with SQL; each Excel sheet is a table.
- **Run SQL file…** on a connection or database runs a `.sql` file (or `.sql.gz`, `.sql.zst`)
  of any size statement by statement, with progress and Stop, by default in one transaction
  that a failure rolls back. **Dump to SQL file…** on a schema or on chosen objects writes their
  DDL and, if wanted, their rows to one file that runs again elsewhere.
- Imports and copies into PostgreSQL use `COPY`, and into MySQL `LOAD DATA LOCAL` where the
  server allows it.

### MongoDB

Statements in mongosh syntax: `db.orders.find({ status: 'paid' }).sort({ placed_at: -1 }).limit(10)`,
`aggregate`, `countDocuments`, `distinct`, `insertOne`/`insertMany`, `updateOne`/`updateMany`,
`replaceOne`, `deleteOne`/`deleteMany`, `createIndex`, `db.runCommand`, `show dbs`,
`show collections` and `use`, with the shell's values (`ObjectId(...)`, `ISODate(...)`,
`NumberLong(...)`, `/regex/i`) and Extended JSON. Results are a grid of the top-level fields
with the focused document as a tree beside it (`F7`). The navigator lists databases,
collections, fields sampled from the documents and indexes; the editor completes collections,
methods and fields. Generate data fills a collection with fake documents: new ObjectIds or
numbers after the largest `_id`, lists of words for arrays, small documents for nested fields.
`mongodb+srv://` and connection string options go in the Database field
(`shop?authSource=admin&replicaSet=rs0`).

### Transactions and safety

- Auto-commit or manual commit with Commit/Rollback; an open transaction shows how long it has
  been open and warns after a while (10 minutes by default). Clicking it lists the changes that
  wait for Commit, with Commit and Rollback.
- An `UPDATE` or `DELETE` without `WHERE`, or a `TRUNCATE`, asks first and says about how many
  rows it changes.
- On production connections every change is listed and confirmed before it runs.
- The lock button makes one editor tab read-only without changing the saved connection.
- Settings › Safety can add a `LIMIT` (`TOP` on SQL Server) to queries run from the editor that
  have no row cap of their own; off by default.

### Workspace

| | |
|---|---|
| ![Command palette](docs/screenshots/palette.png) | ![Platypus Light](docs/screenshots/light-theme.png) |

- **Projects**: a rail of colored squares on the left, one per project (a client, an
  environment, a side job). A project shows only its connections, tabs and recent queries; a
  connection can be in several projects. `Ctrl+1` is All connections, `Ctrl+2`… the projects.
- **The frame shows where you are**: the bar on top names the project and the connection of
  the active tab, and the window frame takes the color of its type, green for development,
  blue for test and red for production. On Windows and Linux the bar is also the window's title
  bar, with its buttons (Settings › Appearance › Title bar puts the system's back).
- **Command palette** (`Ctrl+K`): actions, connections, tables and recent queries, found by
  typing a few letters.
- **Resizable navigator**: drag the edge between the connections and the work (or focus it and
  use the arrows); a double-click brings back the default width. `Ctrl+B`, or the button at the
  left of the top bar, hides and shows it.
- **Tabs**: a right-click pins a tab (it stays first, as an icon), opens it beside the active
  one or closes the others or those to its right; Settings › Tabs groups them under their
  connection's name, and a click on the name folds the group. `Ctrl+Shift+T` reopens the last
  closed tab.
- **Split editor** (`Ctrl+Shift+D`): two tabs side by side; a click in one makes it the active
  one.
- **Drag a table or a column** from the navigator into the SQL editor: its name, or a `SELECT`
  in an empty editor or with `Shift` held.
- **Activity log** (the pulse icon on top): the errors shown and the work done in the
  background, such as connections and catalog loads, with their time, to copy into a report.
  A query that runs longer than 10 seconds (Settings › SQL editor) ends with a system
  notification when the window is in the background.
- From the keyboard: arrows, Enter and the menu key in the navigator, the tab bar and the
  context menus; dialogs keep the focus inside them and give it back when they close.

### Settings and themes

![Settings, one section per tab](docs/screenshots/settings.png)

- **Settings** (`Ctrl+,`) show one section at a time, in tabs, and open again on the last one.
  Their search shows the matching settings of every section, grouped by section, and finds the
  keyboard shortcuts by their action's name.
- **Themes**: Platypus Dark and Light (what "System" follows), Classic Light and Dark, Nord,
  Dracula, Solarized Light and high contrast; your own accent, background, sidebar and text
  colors, interface size, row density, corner radius and editor font size. Inter and
  JetBrains Mono are bundled, so the app looks the same everywhere and works offline.
- **Theme files** (Settings → Themes): Export saves the theme in use, with your custom colors,
  as JSON (`{"platypus_theme": 1, "name", "light": {…}, "dark": {…}}`, both variants for
  System); Import previews a file's theme, lists the keys and colors it skips, and applies it
  or goes back. An imported theme with both variants follows the system's light or dark mode.
- **Seven languages**: English, Italian, Spanish, German, French, Portuguese (Brazil) and
  Chinese (Settings › Appearance › Language; System follows the operating system's). Numbers
  and shortcuts follow the language (`Strg` in German); messages from the databases stay in
  theirs. The dictionaries are in `apps/desktop/src/i18n/`, keyed by the English text, and
  open to corrections.
  `Ctrl+=` / `Ctrl+-` / `Ctrl+0` change the interface size; the editor and the result grid
  have their own font size.
- Every shortcut can be changed; the toolbars show icons (each says what it does on hover),
  and the SQL editor's buttons can be hidden or given their words back.
- **Logs and bug reports**: the app logs its starts, errors and crashes to `logs/` in its data
  folder (three files of 1 MB at most); nothing is sent anywhere. After a crash it offers a
  diagnostic package, also in Settings › Diagnostics: a zip with the version, the system, the
  settings without passwords, connection counts by database and the logs, to attach to a bug
  report. Error messages in the logs may name a server or a table: look before sharing.

## Agents (MCP) and the CLI

Claude Code, Codex, Cursor and other MCP clients use your saved connections through
PlatypusDB's MCP server: the app itself, started with `--mcp`, which they start themselves
(no window opens, and the app need not be open).

- Each connection gives agents **no access**, **read only** (the default) or **read and
  write**; production and read-only connections are never more than read only.
- Passwords stay in the keychain and no answer shows a host, a user or a secret.
- Agents' queries are in the history with the agent's name, and every tool call is in the
  agents' call log (Settings → Agents → Activity), by session, with a filter and a CSV export.
- An agent can propose SQL with `propose_sql`: it opens in a new editor tab, for you to
  review and run.

Settings → Agents → Setup lists Claude Desktop, Claude Code, Codex, Cursor, Windsurf,
Antigravity and Gemini CLI with their configuration file and whether PlatypusDB is in it:
**Install** writes the server there, after asking and after a copy of the file
(`<file>.platypus-backup-<date>`), keeping the rest of the file as it was. The same entry is
shown to copy by hand, and so are the commands that register the server. Security lists the
access of each connection. Agents
registered with `platypus-mcp.exe` by version 0.2 need registering again with these commands:
the installers no longer ship that second executable.

```bash
claude mcp add platypus -- "C:\Program Files\PlatypusDB\platypus-db.exe" --mcp
```

and for Codex, in `~/.codex/config.toml`:

```toml
[mcp_servers.platypus]
args = ["--mcp"]
command = 'C:\Program Files\PlatypusDB\platypus-db.exe'
```

<details>
<summary>How the MCP server works</summary>

`platypus-db --mcp` talks MCP over stdin and stdout. It reads the connections of the app's
profile, or of `PLATYPUS_DATA_DIR` when set. With a master password the saved passwords stay
locked to agents: such connections answer with an error saying so, and work with a password
command or `.pgpass` instead. A connection that asks for its password every time is not
available to agents.

Tools: `list_connections`, `list_schemas`, `list_objects`, `describe_table`, `get_ddl`,
`search_objects`, `run_query`, `sample_rows`, `explain` and `propose_sql`. `run_query` runs one statement and returns 100 rows,
32 KB and 30 s at most by default (`max_rows` up to 10,000, `max_bytes` up to 1 MB,
`timeout_seconds` up to 300), as an aligned text table or JSON, saying when rows or values
longer than 1,000 characters were left out. In the text table SQL `NULL` reads `NULL` and a
text `NULL` reads `'NULL'`. `list_objects` returns 500 objects at most (`limit` up to 2,000);
a larger schema comes in pages (`truncated`, `total`, `next_offset`, then `offset`). The
client's `notifications/cancelled` cancels the running statement.

`sample_rows` returns the first rows of a table, view or collection (20 by default) within
the limits of `run_query`. `explain` returns a statement's plan as an indented tree (or JSON)
with costs, rows and index suggestions, within the byte limit and the timeout; the statement
is checked as `run_query` checks it, and runs only with `analyze`, rolled back: on read-only
connections that needs a read-only transaction that refuses writes (PostgreSQL, MySQL,
MariaDB, SQLite). `propose_sql` runs nothing: the app opens the SQL in an editor tab, at once
if it is open, else when it starts (within 24 hours).

On read-only connections only statements that change nothing run, inside the database's
read-only transaction (PostgreSQL, MySQL and MariaDB `READ ONLY`, SQLite `query_only`; DuckDB
and SQL Server roll the transaction back), and functions that act outside it
(`pg_terminate_backend`, `dblink`, `LOAD_FILE`…) are refused. DuckDB sessions of agents
cannot read or write other files nor load extensions, and close after each call so the app
can open the file.

On read-write connections changes commit at once; `BEGIN`, `COMMIT` and `ROLLBACK` are
refused, and so are `UPDATE` or `DELETE` without `WHERE` and `TRUNCATE` unless Settings →
Safety runs them without asking.

</details>

Agents without MCP use the `platypus` CLI (crate `platypus-cli`) with `--connection`, which
follows the same access and prints one JSON document:

```bash
platypus connections                                  # [{"name", "id", "driver", "type", "database", "agent_access"}]
platypus --connection "Local PG" schemas              # [{"name", "system", "current"}]
platypus --connection "Local PG" objects --schema demo
platypus --connection "Local PG" table --schema demo --table orders
platypus --connection "Local PG" ddl --schema demo --name orders   # {"ddl": "CREATE TABLE ..."}
platypus --connection "Local PG" search order          # {"matches": [...], "truncated": false}
platypus --connection "Local PG" query "select * from demo.orders" --max-rows 50
```

A query prints `{"columns": [{"name", "type"}], "rows": [[...]], "row_count",
"truncated", "truncated_by": "max_rows" | "max_bytes" | null, "values_cut", "elapsed_ms"}`,
with numbers and booleans as JSON values and everything else as text; a statement without
rows prints `{"affected_rows", "elapsed_ms"}`. `-f table` prints the text table instead. A
failure prints `{"error": {"kind", "message"}}` and exits with 1; `kind` is `not_found`,
`denied`, `locked`, `invalid`, `connect`, `database`, `timeout` or `store`.

## Keyboard shortcuts

On macOS use `Cmd` instead of `Ctrl`. Every shortcut can be changed in Settings › Keyboard
shortcuts.

<details>
<summary>All shortcuts</summary>

| Keys | Action |
|---|---|
| `Ctrl+Enter` | Run the statement under the cursor (or the selection) |
| `Alt+X`, `Ctrl+Shift+Enter` | Run the script |
| `Ctrl+\` | Run into a new result tab |
| `Ctrl+Space` | Autocomplete |
| `Ctrl+/` | Comment / uncomment |
| `Ctrl+Shift+F` | Format the script (or the selection) |
| `Ctrl+Shift+A` | Select the statement under the cursor |
| `Ctrl+Shift+E` | Explain the statement under the cursor |
| `Ctrl+Shift+M` | Query manager |
| `Shift+Alt+Down`, `Shift+Alt+Up` | Duplicate the line |
| `Alt+Up`, `Alt+Down` | Move the line |
| `Ctrl+Shift+K` | Delete the line |
| `Ctrl+Alt+Up`, `Ctrl+Alt+Down` | Add a cursor above / below |
| `Ctrl+D`, `Ctrl+Shift+L` | Select the next / every occurrence |
| `Alt+drag` | Rectangular selection |
| `Ctrl+F` | Find and replace |
| `Ctrl+S`, `Ctrl+O` | Save / open a `.sql` file |
| `Ctrl+K`, `Ctrl+Shift+P` | Command palette |
| `Ctrl+B` | Hide / show the navigator |
| `Ctrl+T`, `Ctrl+W`, `Ctrl+Shift+T` | New tab / close tab / reopen the closed tab |
| `Ctrl+Tab`, `Ctrl+Shift+Tab` | Next / previous tab |
| `Ctrl+Shift+D` | Split the editor / close the split |
| `Ctrl+=`, `Ctrl+-`, `Ctrl+0` | Bigger / smaller / normal interface |
| `Ctrl+1`, `Ctrl+2`… `Ctrl+9` | All connections / the projects in rail order |
| `Ctrl+Alt+PageDown`, `Ctrl+Alt+PageUp` | Next / previous project |
| `Ctrl+,` | Settings |
| `F2`, `Enter`, typing | Edit the cell (data viewer, one-table queries) |
| `Insert`, `Ctrl+D`, `Delete` | Add, duplicate, delete rows |
| `Shift+Delete` | Set the selected cells to NULL |
| `Ctrl+Alt+D` (macOS: `⌃⇧D`) | Fill the selected cells down from the first row |
| `Ctrl+V`, `Ctrl+Shift+V` | Paste over the cells / as new rows |
| `Ctrl+S`, `Ctrl+Z` | Save / undo data changes (grid focused) |
| `Ctrl+Shift+C` | Copy the selection with headers |
| `Ctrl+F`, `F7` | Find in the results / value panel (grid focused) |
| `Ctrl+Alt+R` | Record view of the focused row; `←` `→` move through the rows |
| `F5`, `F2`, `F4`, `Delete` | Navigator: refresh, rename, edit the table, drop (after its confirmation) |

</details>

## Performance

| | PlatypusDB 0.1.0 | Target |
|---|---|---|
| Start to interactive window | 430–500 ms | < 500 ms |
| Memory at rest, 1 connection | about 130 MB | < 150 MB |
| Scrolling 1M rows | 178 fps | 60 fps |
| 5M rows to Parquet | 3.2 s, 31 MB peak | streaming |
| Installer | 5.4 MB | < 20 MB |

Windows 11, Ryzen 7 9700X; details and method, the first Linux numbers and the scale tests
(10,000 tables, a million rows, cells of tens of MB) in [docs/BENCHMARKS.md](docs/BENCHMARKS.md).

## Development

What comes next is planned in [docs/ROADMAP.md](docs/ROADMAP.md), and the working rules are in
[CONTRIBUTING.md](CONTRIBUTING.md). [docs/EXTENDING.md](docs/EXTENDING.md) explains how features
plug in (actions, tab kinds, settings, driver capabilities); [docs/RELEASING.md](docs/RELEASING.md)
how releases are made, and [docs/MVP_PLAN.md](docs/MVP_PLAN.md) the scope of the first release.
All the documents are listed in [docs/README.md](docs/README.md), and the changes of each
release in [CHANGELOG.md](CHANGELOG.md).

### Layout

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
  platypus-driver-oracle/  Oracle (oracle crate on Oracle Instant Client)
  platypus-driver-adbc/    Snowflake, BigQuery, Databricks, Trino and any ADBC driver, loaded at run time
  platypus-driver-odbc/    Db2 and any ODBC driver or DSN, through the system's driver manager (Windows)
  platypus-drivers/        drivers downloaded on first use: the dbc registry, Instant Client, IBM's ODBC driver, DuckDB
  platypus-tls/            rustls client configuration with libpq `sslmode` semantics
  platypus-sql/            dialect-aware lexer, statement splitting, read-only check, DML
  platypus-schema/         table designs, the DDL between them, schema linter and comparison
  platypus-net/            SSH tunnels (russh), proxies, commands around a connection
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
vendor/                    tiberius, patched for the SQL Server driver
site/                      static presentation page
```

### Requirements

- Rust ≥ 1.99 (`rustup`)
- Node.js ≥ 24 (LTS, see `.nvmrc`)
- Tauri system prerequisites: <https://v2.tauri.app/start/prerequisites/>
  (Windows: MSVC Build Tools + WebView2; Linux: `libwebkit2gtk-4.1-dev` and friends)
- Docker, for the dev databases

### Development

```bash
# desktop app with hot reload
cd apps/desktop
npm install
npm run tauri dev

# core crates; the conformance suite (crates/platypus-conformance) runs the same scenarios on
# every driver. Server databases that are down are skipped; set PLATYPUS_TEST_PG_URL,
# PLATYPUS_TEST_MYSQL_URL, PLATYPUS_TEST_MARIADB_URL, PLATYPUS_TEST_MSSQL_URL,
# PLATYPUS_TEST_CLICKHOUSE_URL, PLATYPUS_TEST_LIBSQL_URL or PLATYPUS_TEST_MONGO to make them
# mandatory
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
```

In VS Code, **Run PlatypusDB** (Ctrl+Shift+B) installs the frontend dependencies if they are
missing, builds the sample database and starts `tauri dev`. `.vscode/tasks.json` also has
tasks to start and stop the dev databases.

### Sample database

`node scripts/sample-db.mjs` builds `samples/wildlife.db` from
[samples/wildlife.sql](samples/wildlife.sql): a SQLite platypus field survey with regions,
rivers, researchers, tagged animals, surveys, about 15,000 sightings and a `platypus_stats`
view, with foreign keys between all of them. Open it with a new SQLite connection, or from the
welcome screen, which builds it with the scripts in `samples/scripts/`.

### Dev databases

```bash
# PostgreSQL 17, MySQL 8.4, MariaDB 11.4 and ClickHouse 26.3; the others with their profile
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

Each database has `customers`, `orders`, a `customer_totals` view, a `type_zoo` table with one
column per notable type, and a 1M-row `big_table` for streaming and scrolling benchmarks.
These credentials are for local test containers only.

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
