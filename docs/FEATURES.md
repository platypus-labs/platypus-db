[← README](../README.md)

# Features

Everything PlatypusDB does, area by area. The [README](../README.md) has the short version.

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
| Redis 6+ | `redis` crate (RESP3, rustls) | Commands in the editor; key types as tables, edited in the grid; JSON with RedisJSON |
| Elasticsearch 8 and OpenSearch 2 | HTTP API (`reqwest`, rustls) | Requests as Kibana's console writes them, and SQL; indices as tables, edited in the grid |
| Apache Cassandra 4+ and ScyllaDB 5.4+ | `scylla` crate (rustls) | CQL; keyspaces as schemas, tables, materialized views, types and functions in the navigator; edited in the grid by full primary key; no row count, sort or filter in the data viewer |
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
and shows the ODBC drivers and data sources of the system. A downloaded ADBC driver can be
updated from there (the app looks for new versions once a day, or on *Check for updates*;
*Driver updates* in Settings can install them by itself), and *Versions…* installs or goes
back to another version, keeping the others. After a change the saved connections that use
the driver are tried (connect, version, schemas, `SELECT 1`), with *Go back* if one fails. The
built-in drivers update with the app.

## Portable copy

Put the executable in a folder with an empty `data` folder beside it, as
with VS Code. Connections, settings, history, scripts, logs and the web view's storage then
live in `data`, and the folder can move to another machine or a USB stick. Saved passwords stay
in the system's keychain unless a master password keeps them in the folder too. A portable
copy updates by replacing its executable; the welcome screen says `portable` beside the version.

## Package managers

Besides the installers of each release, the app can come from winget
(`winget install PaoloCisly.PlatypusDB`), Homebrew (`brew install --cask platypusdb`), the AUR
(`platypusdb-bin`) and Flathub (`dev.platypusdb.desktop`), once each package is published.
Installed from Flatpak or the AUR, the app says when a new version is out but leaves the update
to the package manager.

## Connections

![Choosing the database for a new connection](screenshots/connections.png)

- A new connection starts from a grid of database logos, with a search on top (`Enter` takes
  the first match); the `+` of the navigator opens it, and so does a right-click on a connection
  or below the list. Or paste a connection under the logos: a URL (`postgres://`,
  `mysql://`, `sqlserver://`, `clickhouse://`, `libsql://`, `mongodb://`, `redis://`, `rediss://`, `elasticsearch://`, `opensearch://`), a JDBC URL or an
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
- **Port forwards** from a cloud's CLI: `kubectl port-forward` to a Kubernetes service or pod,
  or an AWS Systems Manager session through an EC2 instance to a private host. The app starts
  it on a free local port while the connection is open; the CLI must be installed and signed in.
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
- **Cloud sign-in** without a password: AWS IAM (RDS) and Google Cloud IAM (Cloud SQL) for
  PostgreSQL and MySQL, Microsoft Entra ID for PostgreSQL, MySQL and SQL Server. The token comes
  from `aws`, `gcloud` or `az` at each connection and is never saved. SQL Server also signs in
  as the Windows user (SSPI: Kerberos or NTLM) on Windows. Imported or pasted Windows and
  Entra ID logins keep their way to sign in.
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

## Navigator and objects

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
  the DDL of many tables at once: the tab opens at once and fills a block of objects at a
  time, with how far it is ("DDL: 30 of 300 objects") and *Stop*; an object whose DDL cannot
  be read becomes a comment saying why. PostgreSQL reads the tables and views of a block with
  a few catalog queries for all of them.
- **Object details**: properties, columns, indexes, constraints, foreign keys, incoming
  references and the DDL of any object.
- **Search** object names, columns and the source of views, routines and triggers across a
  database; or the text of every row of a schema's tables, opening the matching rows.

## Notes, comments and schema documentation

![The comments, notes and tags of a table and its columns](screenshots/catalog.png)

- **Notes and tags** on tables, views and columns, kept on your computer and never written to
  the database: in the object tab, for the object and for each of its columns. The navigator
  marks the annotated objects and shows the note on hover, and `#pii` in its filter keeps the
  objects tagged `pii`. The data viewer marks the annotated columns in its headers, and
  completion shows the note and tags of what it offers. They follow the renames made in the
  app.
- **Database comments** show beside the notes. Those of tables and their columns edit in the
  object tab, with the statement shown first; those of views are read-only. SQLite, libSQL and
  MongoDB, Redis and Elasticsearch have none.
- **Row label**: the column that names a table's rows where foreign keys point to it, picked in
  the object tab or from a header's menu in the grid.
- **Sensitive columns** (personal data, secrets): the rules of Settings › Safety name them by
  name or type (emails, phones, passwords…), and the *Sensitive* column of the object tab marks
  a column sensitive or not over the rules, kept with the notes. Masking hides them, the data
  viewer's headers say which they are, and an export can keep, mask or leave them out.
- **Used by**, in a table's tab: for each column, the views, routines and triggers of its schema
  and the saved scripts of the project that name it, with the line, and the columns named
  nowhere. A text counts when it names the table and the column (a column of the same name of
  another table in it counts too). Comments and strings do not; a PostgreSQL routine's `$$`
  body does.
- **Schema documentation**: right-click a schema, *Export documentation…*, for a Markdown file
  or a single HTML file (it works offline) with the columns, keys, indexes, comments, notes,
  tags and an ER diagram of each schema of up to 100 tables.

![The exported HTML documentation with its ER diagram](screenshots/schema-docs.png)

## SQL editor

![JOIN completion from the foreign keys](screenshots/autocomplete.png)

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

## Results and the data viewer

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
- **Computed columns** (grid menu, *Columns › Computed columns…*): a name and an expression on
  the other columns of the same row, such as `price * qty` or
  `upper(customer) || ' ' || coalesce(city, '?')`, shown after the result's columns. The
  expressions take numbers, `'text'`, NULL, `+ - * / %`, `||`, comparisons, AND, OR, NOT,
  `IS NULL` and the functions upper, lower, trim, length, substr, replace, abs, round, floor,
  ceil, coalesce, nullif, concat and if; NULL in an operation gives NULL, as in SQL. They are
  only on screen: `Ctrl+C` copies them with the rest of the selection, but *Copy as*, totals,
  search, sorting and editing work on the result's own columns. They follow the edits not
  saved yet, read numbers with the digit grouping of the settings and show as dots when they
  read a masked column. They work on the text the grid holds, so a text longer than 500
  characters counts only that much. A table keeps its computed columns.
- Totals of the selection (count, sum, average, minimum, maximum), a record view of one row
  at a time, counts by value, statistics of the selection, and two rows compared column by
  column.
- Foreign keys open the row they point to, and the References panel lists the rows of other
  tables that point at the focused one.
- A **map panel** draws PostGIS, MySQL and WKT geometries and PostgreSQL's geometric types.
- Read-only results can refresh every few seconds. Dates, time zone, digit grouping and
  relative times follow the Settings.

## Data analysis

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
- **Notebooks**: *New notebook* in a connection's menu opens a tab of cells, text in Markdown
  and SQL, run one at a time (Ctrl+Enter, Run) or all in order (*Run all*, which stops at the
  first failure). Each SQL cell shows its result as a grid or a chart, and its plan with
  *Plan*. The SQL cells are named `c1`, `c2`, … in their order: a cell *On the results of the
  other cells* runs in an in-memory DuckDB where their loaded rows are tables of those names,
  without running them again on the server. Parameters (`:name`, `?`, `${name}`) are shared by
  the cells: a value missing shows a field above the cells. Changes go through the Safety
  checks, and a read-only connection refuses them. *Save* writes a `.pnb` file (JSON), which
  *Open notebook…* opens again on a connection; *Export HTML…* writes a standalone page with
  the text, the SQL, up to 500 rows of each result, the charts, the plans and the parameters'
  values. Notebooks stay with the workspace; their results run again.
- **Pivot…**: on a result's *Query with SQL* bar (or the grid menu): rows, a column whose
  values become columns, a value and its aggregate (count, distinct, sum, average, min, max),
  totals and sorting. The DuckDB `PIVOT` it writes stays in the SQL field, to edit and run
  again; up to 200 columns.
- **Query across databases…**: right-click tables (of any SQL connections, each with an
  optional `WHERE`) and query them together in one SQL editor: they are copied into an
  in-memory DuckDB, so a PostgreSQL table joins a MySQL one. *Reload sources* reads them again.
  Up to 512 MB of rows in all.
- **Save result…**: keeps a whole result as a Parquet file in the app's data folder. The
  *Saved results* panel under Scripts lists, renames and deletes them; *Open* shows one with
  grid, charts, statistics and SQL, without connecting to its database.
- **Dashboards**: right-click a connection, *New dashboard*: widgets from saved scripts shown
  as a table, a chart or one big value, side by side in up to three columns, each refreshed on
  its own, all at once, or every 1 to 60 minutes. Only scripts that just read are run.
  Dashboards are saved per project and reopened from *Dashboards…*.
- **Profile data…**: right-click a table or view for a card per column over the first 10,000
  to 1,000,000 rows (or all of them): NULLs, empty strings, distinct values, range, mean,
  quartiles, a histogram, the most frequent values, the formats of text values (emails, URLs,
  UUIDs, dates, numbers stored as text, phone numbers, stray spaces) with the values that do
  not fit, and outliers. Click a value to see its rows.

![A result as a bar chart](screenshots/chart.png)

## Data editor

![Pending changes in the data editor](screenshots/data-editor.png)

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

## Table editor

![Designing a table](screenshots/table-editor.png)

Right-click a table, *Edit table…* (F4), or a schema, *Create table…*; *Edit table…* is also
in a table's tab, next to *Open data*, and in the data viewer's bar, next to *Structure*, which
opens the table's tab. Either brings forward the editor already open on the table. Columns (type, NOT NULL,
default, primary key, comment), indexes, foreign keys with their `ON DELETE`/`ON UPDATE`
actions, and UNIQUE and CHECK constraints. Saving shows the `CREATE TABLE` or `ALTER TABLE`
script first and runs it in one transaction where the database can roll DDL back (MySQL
commits each statement). SQLite and DuckDB, which alter little in place, get the table
created again with its rows copied.

## ER diagrams, schema check, comparison and history

- **ER diagram**: right-click a schema, *ER diagram*, or some selected tables, *Show in ER
  diagram*. Tables are laid out from their foreign keys, right of the tables they reference,
  each group of linked tables in its own block, in an order that keeps lines from crossing,
  with the lines running between the tables rather than over them; drag them where you want them, choose the tables with *Tables…*, add notes. The diagram is
  kept with its tab and exports to SVG or PNG. A schema of more than 100 tables asks which
  ones to show first. *Full screen* gives the diagram the whole window (Esc leaves it);
  *Refresh* reads the schema again and keeps every table where it is, new tables below.
- **Draw the schema**: *Edit* on the diagram (not on read-only connections) adds tables,
  changes or drops existing ones in the table editor's form, and draws a foreign key by
  dragging the small circle of a column onto another table (to its primary key). The edits
  stay with the tab until discarded; *Generate DDL* opens the script that makes them, tables
  in the order their foreign keys need, in the SQL editor.
- **Create view, function, trigger or sequence**: right-click a schema, *Create*, or a table,
  *Create trigger…*: a form for the object with the SQL previewed as you type, then *Open in
  editor* or *Create*. PostgreSQL triggers get their trigger function written too.
- **Check schema…**: tables without a primary key, foreign keys without an index, duplicate
  indexes and indexes that another one starts with, foreign keys of another type than the
  column they reference. The `CREATE INDEX` or `DROP INDEX` that fixes a finding opens in the
  SQL editor, to review and run there.
- **Compare with…**: pick another schema of the same kind of database, on this connection or
  another one: the script that makes it like this one (tables created, altered and dropped,
  in an order the foreign keys allow) opens in the SQL editor of that connection. Tables and
  their parts are matched by name, so a renamed column is a dropped column and a new one.
  *Output* can also save the script as a Flyway migration (`V<timestamp>__<description>.sql`)
  or a Liquibase changelog in formatted SQL, one changeset per statement.
- **Compare data with…**: right-click a table and pick a table of another database of the
  same kind (or of this one): rows are matched by primary key, optionally filtered with a
  `WHERE` condition, and counted as same, only in the source, changed or only in the target,
  with the first 200 differences shown cell by cell. *Open script* gives the `INSERT`,
  `UPDATE` (changed columns only) and, if asked, `DELETE` statements that make the target
  like the source. Up to 500,000 rows per table.
- **Schema history**: snapshots of a schema's tables, taken by hand or by a scheduled task
  (only when something changed); tick two, or one and *Now*, to see what changed by table
  and open the script between them.
- **Migration status…**: point it at a project folder and see which Flyway, Liquibase,
  Prisma, Alembic or Rails migrations the database has applied, which are pending, which
  were changed after being applied (Flyway and Prisma checksums), failed, or exist only in
  the database.

## Visual query builder

![Two tables joined on their foreign key](screenshots/query-builder.png)

Drag tables onto a canvas: they join on their foreign keys. Tick the columns, add a `WHERE`,
then run the SELECT or open it in the editor.

## Query plans and performance

![Explain analyze with the heatmap and an index suggestion](screenshots/explain.png)

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

![The query manager with the trace of a statement](screenshots/query-manager.png)

- **Query manager** (`Ctrl+Shift+M`): every statement, metadata request, commit and rollback
  the app sent, with connection, duration, rows and outcome; the last 2,000 are kept in
  memory. A statement's trace splits its time into network, server and fetch.
- Statements slower than a threshold (5 s by default) are marked in the status bar and listed
  apart.

## Server activity

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
- **Costliest statements**: the statements that took the server most time, summed over
  their runs (PostgreSQL's `pg_stat_statements`, MySQL's and MariaDB's `performance_schema`,
  SQL Server's plan cache), ordered by total time, mean time or calls, with their rows; one
  opens in the editor or in a query plan. Without the statistics the tab says how to turn
  them on.
- **Tables and indexes**: size, rows, dead rows or unused space, full scans and index reads,
  last vacuum and analyze, each index under its table. "Only those worth a look" keeps the
  unused indexes, the big tables read whole more than through an index, those with many
  dead rows or unused space, and those never analyzed.
- **Maintenance** of a table, from its details there or from its menu in the navigator:
  `VACUUM`, `ANALYZE` and `REINDEX` on PostgreSQL; `ANALYZE`, `OPTIMIZE` and `CHECK TABLE` on
  MySQL and MariaDB; `UPDATE STATISTICS`, an index rebuild and `DBCC CHECKTABLE` on SQL
  Server. Each asks first and shows what the server said.
- **Parameters**: the server's settings with their value, default, source and description,
  searchable, with *Only changed*; *Open in the editor* writes the statement that changes one
  (`ALTER SYSTEM SET`, `SET GLOBAL`, `sp_configure`…) for you to run.
- **Dashboard**: connections, transactions and statements per second, cache hit ratio,
  database sizes, replication and uptime, with their recent history; what the server does not
  let the login read shows "—" and why.
- **Users and roles**: the server's users and roles with their attributes, memberships and
  privileges; create, change and drop them, grant and revoke privileges on databases, schemas,
  tables and routines (with grant option) from a form, with the SQL previewed first. Passwords
  are masked in the preview and never reach the query log. PostgreSQL, MySQL, MariaDB, SQL
  Server and ClickHouse.
- **Debug…** a PL/pgSQL function or procedure (PostgreSQL with the `pldebugger` extension):
  arguments in a form, then step into, step over, continue (F11, F10, F5), breakpoints on the
  source, the stack, variables you can change, and the result at the end.
- **Back up… and Restore…** of a PostgreSQL or MySQL database (or a PostgreSQL schema) with
  its own tools on the PATH (`pg_dump`, `pg_restore`, `psql`, `mysqldump`, `mysql`), through
  the connection's tunnel or proxy, table by table with a progress bar. A `pg_dump` archive
  restores with `pg_restore`, optionally dropping its objects first; a SQL file runs through
  `psql` in one transaction, or through `mysql`, where views, triggers and routines are made
  by the connection's user instead of their original definer.
- **Scheduled tasks** (command palette, or *Schedule a task…* in a connection's menu): run a
  SQL script, export a query to CSV, JSON, Parquet or Excel, or back up the database (with
  its tools for PostgreSQL and MySQL, as the SQL dump of a schema for the others) every
  hour, day, week or month at a time in a time zone. Exports and backups go to a folder that
  keeps the newest files; a failed run is tried again a few minutes later if asked. The list
  shows each task's next run and its last runs, with *Run now*. Tasks run while the app is
  open, a time missed while it was closed at the next start, with the connection's saved
  password.

## Import, export and transfer

- **Export** to CSV, JSON, NDJSON, Parquet, Excel, SQL `INSERT`, Markdown, HTML or XML,
  streamed straight to the file; several tables or a whole schema at once. Optional gzip or
  zstd compression, and upload to a signed URL or to an S3, GCS (HMAC keys), R2 or MinIO
  bucket; the secret key is never saved (a saved profile keeps it in the password store).
- **Import** CSV, TSV, JSON, NDJSON, Parquet and Excel into a table, with a preview and a
  mapping of columns, adding the rows, replacing them or updating those with the same key; or
  into a new table created with the types DuckDB infers. *Paste CSV as a new table…* imports
  the clipboard.
- **Copy a table to another database** (PostgreSQL to MySQL, SQL Server to SQLite and every
  other pair), streamed, into a new table with the types mapped or into an existing one.
- **Copy a schema** to another database: each table with its columns, keys and identity
  column, then its defaults, indexes, foreign keys and CHECK constraints; between databases of
  one kind the views, functions, procedures and triggers too.
- **Transfer profiles**: an export, import, table copy or schema copy can be saved with all its
  choices (*Save as profile…* in its dialog; *Save profile* replaces the one that was opened)
  and run again from *Transfer profiles…* (command palette, or a connection's menu for its own),
  which also opens a profile in its dialog, renames, deletes and schedules it. A bucket's secret
  key and a signed upload URL are kept in the password store, never in the profile file. A
  schema copy saved for a whole schema copies the tables it has when it runs. A profile used by
  a scheduled task cannot be deleted; opening one tells you about tables and columns that are
  gone.
- **Fake data** for a table: names, emails, numbers, dates, enum values, foreign keys that
  point at existing rows, distinct values for unique columns.
- **Data files as databases**: drop a CSV, JSON, Parquet or Excel file on the window to query
  it with SQL; each Excel sheet is a table.
- **Run SQL file…** on a connection or database runs a `.sql` file (or `.sql.gz`, `.sql.zst`)
  of any size statement by statement, with progress and Stop, by default in one transaction
  that a failure rolls back. **Dump to SQL file…** on a schema or on chosen objects writes their
  DDL and, if wanted, their rows to one file that runs again elsewhere.
- Imports and copies into PostgreSQL use `COPY`, into MySQL `LOAD DATA LOCAL` where the
  server allows it, into SQL Server its bulk copy (`OPENJSON` inserts for tables with `money`,
  `xml` or legacy columns) and into DuckDB its appender.

## MongoDB

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

## Redis

The editor runs Redis commands, one per line, quoted as `redis-cli` quotes them (`SET k "a b"`,
`"\xff"` for bytes); `#` and `--` lines are comments. Replies are rows: a value, a map's
fields, a list's items, nested replies as JSON in the document tree beside the grid (`F7`).
`SELECT n` moves the session to another database; `MULTI` queues commands until `EXEC`, which
sends them as one transaction and shows each reply. Subscriptions and `MONITOR` are refused:
they would take over the connection. Cancel stops a blocking command (`BLPOP`, `XREAD BLOCK`).

The navigator lists the databases with keys (`db0`, `db1`…, and the session's), each with a
table per key type: `string`, `hash`, `list`, `set`, `zset`, `json` (with RedisJSON) and
`stream`, with how many keys each has when the database has up to 10,000. Opening one runs
`BROWSE db0.hash`, which finds the keys with `SCAN … TYPE` (never `KEYS`) and reads big keys a
chunk at a time as the grid scrolls: a row per value, `key, value, ttl` for strings and JSON,
`key, field, value` for hashes, `key, index, value` for lists, `key, member` for sets,
`key, member, score` for sorted sets, `key, id, fields` for streams. `MATCH user:*` filters
the keys, `LIMIT n` caps the rows. Values that are not UTF-8 show quoted with escapes.

The grid edits the rows: a value (`SET … KEEPTTL`, `HSET`, `LSET`, `JSON.SET`), a TTL
(`EXPIRE`, `PERSIST` for NULL), a string key's name (`RENAMENX`), a hash field or a set or
sorted set member (moved), a score; new rows add keys and items (`SET … NX`, `HSETNX`, `RPUSH`,
`SADD`, `ZADD NX`, `XADD`), deleted rows remove them (list items by their index). A new or
renamed key, field or member that already exists stops the save before anything runs, unless
the same save deletes it. The preview shows the commands, which run as one `MULTI`/`EXEC` (a
command the server refuses does not undo the others), and Undo script puts the values back.
`FLUSHDB` and `FLUSHALL` ask first as an `UPDATE` without `WHERE` does, and on production
connections commands that change the server (`CONFIG SET`, `SHUTDOWN`) ask like DDL. Read-only connections run only the
commands that read. `rediss://` URLs and SSL modes from `require` up use TLS; the user and
password are the ACL's. Terminal mode runs `redis-cli` with the password in its environment.

## Elasticsearch and OpenSearch

One driver for both; the server says which it is, and the connection shows it (`Connected:
OpenSearch 2.17.1`). Tested on Elasticsearch 8.15 and OpenSearch 2.17, not on Elasticsearch 7.x.
Log in with a user and password (Basic), or leave the user empty and put an API key (the
`encoded` value) in the password field. `elasticsearch://user:pass@host:9200/logs-*` and
`opensearch://` URLs work; `elasticsearchs://` and SSL modes from `require` up use TLS. The
optional index pattern of the connection (`logs-*`, or several separated by commas) narrows
what the navigator lists.

The editor takes requests as Kibana's Dev Tools console writes them: a line `GET /orders/_search`
(the slash is optional; methods `GET`, `POST`, `PUT`, `DELETE`, `HEAD`), then its JSON body, which
ends where its braces close. The NDJSON bodies of `_bulk` and `_msearch` end at a blank line or the
next request. Anything else is SQL (`_sql` on Elasticsearch, `_plugins/_sql` on OpenSearch), ended
by a `;` or a blank line, with typed columns (numbers, booleans, dates) and the cursor of the
server for long results. `#`, `//` and `--` lines are comments, `Ctrl+Enter` runs the statement
under the cursor and `Alt+X` the script. Highlighting follows the kind of statement; completion
offers the methods, the indices, the APIs after an index, the fields of the index where a key of a
body goes, and the same names in SQL. A search gives documents, laid out as MongoDB's: a column for
each top-level field, objects and arrays as JSON in the document tree beside the grid (`F7`).
Aggregations come as results of their own (the metrics in one row, a table per bucket
aggregation), a `_msearch` gives a result per search, and a `_bulk` with failing items lists the
items with the server's reason. Errors keep the server's reason (`no such index [x]`), and an SQL
error points at its place in the statement. Cancel stops a search on the server.

The navigator lists the indices as tables, with their health, shards and replicas in the tooltip
(system and closed ones say so), and the aliases and data streams as views. Hidden indices stay
hidden. The columns of an index are the fields of its mapping (`customer.address.zip` for objects,
`title.raw` for multi-fields). Opening an index runs a search of all its documents and reads them
a page at a time (1,000 rows, then *Fetch all*) with a point in time and `search_after` (a scroll on
OpenSearch), so a read does not stop at the first 10,000 hits. The columns are the same on every
page: a field that no mapped column has goes into a last `_other` column as JSON, and a value that
does not fit its mapped type shows as text.

The grid edits the documents: a changed cell is an `_update` by `_id`, an object replaces the
old one, a new row is a `_create` when it has an `_id` (an `_id` that exists is refused by the
server) and a generated document without one, a deleted row is a `DELETE`. The preview shows the
requests, which run one after the other and wait for the refresh; a request the server refuses
stops the rest, and the ones before it stay. Undo script puts the old values back, or the whole
`_source` of a deleted document (a document with a generated `_id` is left to delete by hand).
`_delete_by_query` and `_update_by_query` without a query (or with `match_all`), and
`DELETE /_all`, ask first like a `DELETE` without `WHERE`, and say how many documents the index
has. Read-only connections run only the requests and the SQL that read. Agents get the same checks
through the MCP server.

## Transactions and safety

- Auto-commit or manual commit with Commit/Rollback; an open transaction shows how long it has
  been open and warns after a while (10 minutes by default). Clicking it lists the changes that
  wait for Commit, with Commit and Rollback.
- An `UPDATE` or `DELETE` without `WHERE`, or a `TRUNCATE`, asks first and says about how many
  rows it changes.
- On production connections every change is listed and confirmed before it runs.
- The lock button makes one editor tab read-only without changing the saved connection.
- Settings › Safety can add a `LIMIT` (`TOP` on SQL Server) to queries run from the editor that
  have no row cap of their own; off by default.
- The **transaction log** (toolbar) lists what the tab ran, gathered by transaction with how each
  ended.
- Before an `UPDATE` or `DELETE` of one table runs from the editor, the rows it changes (up to
  10,000) are kept as the statements that put them back: **Restore…** in the transaction log, or
  *Snapshots before changes…* in the palette, opens that script. On by default.
- A connection can forbid **exports** (no file, other database or agent gets its data) and
  **data editing** (rows change only by SQL: no grid edits, imports or generated rows).
- **Mask sensitive data** (Ctrl+Alt+M) shows the sensitive columns (named in Settings › Safety
  by name or type, or marked in the table's tab) as dots while the screen is shared; copies keep
  the values.
- With a master password, the app can **lock** after some idle minutes, closing the production
  connections; *Lock PlatypusDB* in the palette locks it at once.

## Workspace

| | |
|---|---|
| ![Command palette](screenshots/palette.png) | ![Platypus Light](screenshots/light-theme.png) |

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
- **Panels you can size**: drag the bar between the editor and its results, the edge of the
  side panels of a result, or the line between the halves of a split editor; the sizes stay for
  the next start, and a double-click brings back the default.
- **A tab in its own window** (right-click on the tab › *Open in a new window*), for a second
  monitor: it runs there on a session of its own, and *Move to the main window* brings it back;
  closing the window keeps it for `Ctrl+Shift+T`.
- **Quick query**: the tray icon (Settings › Appearance; on Linux it needs AppIndicator), or
  *Quick query…* in the palette, opens a small query window over the others, on the
  connection used last.
- **Drag a table or a column** from the navigator into the SQL editor: its name, or a `SELECT`
  in an empty editor or with `Shift` held.
- **Activity log** (the pulse icon on top): the errors shown and the work done in the
  background, such as connections and catalog loads, with their time, to copy into a report.
  A query that runs longer than 10 seconds (Settings › SQL editor) ends with a system
  notification when the window is in the background.
- From the keyboard: arrows, Enter and the menu key in the navigator, the tab bar and the
  context menus; dialogs keep the focus inside them and give it back when they close.

## Sharing and sync

- **Git for scripts**: when a project's scripts folder is a Git repository (or after *Turn into
  a Git repository*), the Scripts panel shows the branch, changes per script, *Commit…* of the
  chosen files, *Pull* (fast-forward, or merge when both sides moved), *Push* and the diff of
  a script. Git must be installed.
- **Share query…** from an editor tab: a link (`platypus://…`) or a `.platypus` file with the
  SQL, its title and, if asked, the parameter values, naming the database but never a password.
  Opening one (palette, drag and drop, double click) finds the matching saved connection, or
  offers a new one filled in, and shows the query without running it.
- **Sync** (Settings): settings, connections without passwords, projects and scripts kept in
  one end-to-end encrypted file in a folder you choose (a cloud drive, a network share); other
  computers with the same passphrase merge it with their own, keeping both versions of
  something changed on two sides.
- **Shared projects**: *Share project…* writes a project's connections (without credentials)
  and scripts to a folder a team keeps in Git or on a shared drive; *Open shared project…*
  follows it, and each person enters their own password.
  SQL a teammate sets to run at every connect (a connection's init statements) is shown
  first and runs only after you approve that exact text on your machine; if the file changes it,
  you are asked again, and you can always connect without it. Your own init statements never ask.
- **Plugins** (Settings › Plugins): WebAssembly plugins add actions to the editor, the grid
  and the navigator, or a driver for a database reached over HTTP; each asks for its
  permissions at install. See [PLUGINS.md](PLUGINS.md).

## Settings and themes

![Settings, one section per tab](screenshots/settings.png)

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
