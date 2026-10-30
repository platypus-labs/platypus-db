# Extending PlatypusDB

The roadmap ([ROADMAP.md](ROADMAP.md)) splits work into groups that run in parallel.
Each group owns its own folders and plugs into the app through the extension points below,
instead of editing shared files. If an extension point is missing something, extend it in
a small separate change rather than adding feature logic to a shared file.

## Backend

### Tauri commands

`apps/desktop/src-tauri/src/commands/` has one module per area: `connections`, `query`,
`meta`, `data`, `grid`, `summary`, `transactions`, `export`, `sql`, `history`, `files` and
`settings`. Shared state
(`AppState`, sessions, `live`) is in `commands/mod.rs`. The analysis commands
(`commands/analysis/`) read a result's rows with `analysis::loaded`; the computing is in the
pure crate `platypus-local`.

To add commands:
1. Create `commands/<area>.rs`.
2. Declare it with `pub mod <area>;` in `commands/mod.rs`.
3. List its commands in `generate_handler!` in `lib.rs`.

Child modules can read `AppState`'s private fields.

### Optional driver features

A database feature that not every driver supports is an optional method on the
`Connection` trait (`crates/platypus-core/src/connection.rs`), plus a flag in
`Capabilities` (`capabilities.rs`). The method's default returns `Error::Unsupported`, and
the flag says whether the driver overrides it. `open_session` returns the capabilities, so
the UI can hide what a connection cannot do.

Existing optional methods are `explain`, `object_ddl`, `databases`, `server_sessions`,
`server_parameters`, `server_metrics` and `bulk_load`.
`principals` and `principal_privileges` (flag `security`) list the users, roles and privileges
of a server; the statements that change them are not driver methods: `platypus_sql::security`
writes them per dialect.
`object_properties`
and `catalog_foreign_keys` are optional too, but need no flag: their default is an empty
list. `object_comment` has a default too: it takes the comment from the object list, and a
driver whose list is expensive overrides it with a direct query. `objects_ddl` (the DDL of
several objects, each with its own result) calls `object_ddl` one object at a time by
default; a driver that reads many objects with a few catalog queries overrides it, as
PostgreSQL does, and a wrapping connection forwards it. To add one:
1. Add the method, with a default that calls `unsupported("…")`.
2. Add a flag in `Capabilities`, defaulting to `false`, and the same field in
   `Capabilities` in `api.ts`.
3. Add a check to `capabilities_match_optional_methods` in
   `crates/platypus-conformance/tests/conformance.rs`.
4. Forward it in the two connection wrappers, which otherwise return the default:
   `Held` in `crates/platypus-net/src/lib.rs` and `Logged` in
   `apps/desktop/src-tauri/src/querylog.rs`.

A driver that implements the method sets its flag in `fn capabilities`.

`debugger` (flag `debugger`) is the one optional method that returns an object rather than
a result: a `platypus_core::Debugger` for the database's routines (PostgreSQL's
`PgDebugger`, on the pldebugger extension). It says whether the database can debug now
(`availability`, and `install` for a missing extension), reads a routine and its
parameters (`routine`), and `start`s a call on two sessions of its own, which it is given:
the *target*, blocked by the call, and the *debugger*, which drives it. The
`DebugSession` it returns steps, reads the stack and the variables, changes a variable,
sets breakpoints, and `stop`s: the call is aborted, and the target's server process ended
when it does not answer, so that no session is left blocked. The desktop opens the two
sessions from the saved connection (`commands/debugger.rs`), never the user's own ones. A
wrapping connection forwards `debugger`; the statements are logged through the sessions
it is given.

`explain` returns the server's plan document and the same plan as a tree of
`platypus_core::PlanNode` (operation, detail, cost, estimated and actual rows, time, loops,
other properties), which the explain tab shows for every database alike. A new driver runs
its plan request through `platypus_core::explain_document`, which with `analyze` wraps it
in a transaction (or a savepoint) rolled back afterwards, and parses the result in its own
`explain` module, then builds the result with `Explain::new`, which fills each step's own
cost and time for the heatmap. A driver that recognises a filtered full scan of a table sets
`PlanNode::full_scan`; `Explain::new` turns the large ones into index suggestions.

### Query log

Every session and export wraps its connection in `Logged`
(`apps/desktop/src-tauri/src/querylog.rs`), which records each request into the in-memory
`QueryLog` that the query manager reads with `query_log`. A new place that opens a
connection for the user wraps it the same way (`Logged::wrap`).

Metadata every driver must answer is a required method instead, checked by
`navigates_metadata` in the conformance tests: `schemas`, `objects`, `table_details`,
`referencing_keys` (the foreign keys of other tables that point at a table) and
`catalog_columns`.

### Display formats

The grid's text is rendered in Rust (`display.rs`) with the user's formats from
`formats.rs`: dates and timestamps, the time zone of zoned timestamps, digit grouping and
relative times. Their settings are declared in `apps/desktop/src/grid/formats.ts`. Editing,
copying, searching and exports read the plain values of `display::format_options` instead.

### Network paths

`platypus_net::connect` opens a driver's connection through the SSH tunnel of its
`ConnectionConfig` (`ssh`) and its proxy (option `proxy`, `platypus_net::proxy`), and keeps
the path open as long as the connection. `Tunnel::of` picks the path: a local port forwarded
through the SSH server, whose first hop may go through the proxy, or through the proxy
alone. Drivers only see `connect_addr`, the local end of the path, while `host` still names
the server for TLS. A new kind of path (a port-forward) is a new constructor of `Tunnel`
that hands each local connection to `listen`, and a branch of `Tunnel::of`.

The options `before_connect` and `after_disconnect` (`platypus_net::hooks`) are shell
commands run around the connection; like `proxy`, they are taken out of the options before
the driver sees them.

TLS for a network driver comes from `platypus_tls::client_config`: a rustls `ClientConfig`
(`ring` provider) with libpq `sslmode` semantics, root and client certificates from the
config, or `None` with `disable`. The driver decides how a mode maps to its protocol:
PostgreSQL negotiates TLS in-band, ClickHouse's HTTP picks `https` from `require` up,
libSQL's tries it first from `prefer` up.

### Session setup

`ConnectionConfig.init` (`platypus_core::SessionInit`) holds the default schema, the time
zone and the bootstrap statements. Every driver calls `platypus_core::run_init` at the end
of `connect`, so a reconnect sets the session up again; `SessionInit::sql` gives the
statements per driver. A new driver does the same in its `connect`, and adds its case to
`SessionInit::sql`.

### Passwords

Where a connection's password comes from, in order: a shell command
(`platypus_store::command_password`), `.pgpass` (`pgpass_password`), then the store's
`SecretStore`: the OS keychain (`KeyringSecrets`), or with a master password the encrypted
`secrets.vault` (`VaultSecrets`, Argon2id key, AES-256-GCM). `ConnectionStore::move_secrets`
moves every saved secret from one store to the other; `commands/vault.rs` turns the master
password on and off. A locked vault makes `password` fail with `StoreError::Locked`, which
the UI answers with the unlock dialog.

`commands/health.rs` pings an idle session (`ping_session`): it measures the latency and
reopens a connection the server dropped. Any answer from the server, an error included,
counts as alive.

### SQL generation

Anything that builds SQL goes through `platypus_sql::Dialect` (`crates/platypus-sql/src/dialect.rs`)
rather than checking the driver. It provides:
- `quote_ident`, `qualified`, `quote_literal`
- `bool_literal`, `binary_literal`, `has_default_keyword`, `insert_defaults`
- `limit_clause`, and `limited` for queries without `ORDER BY` (`SELECT TOP (n)` in
  Transact-SQL, whose `OFFSET ... FETCH` needs one)
- statements for the navigator (`generate.rs`): `select`, `insert`, `update`, `delete`,
  `upsert`, `drop`, `rename`, `truncate`. `drop` and `rename` return `None` for an object
  kind the dialect cannot drop or rename; `dialect_support` reports the kinds to the UI.
- `column_type`: the column type that holds an Arrow type, for tables created by an import;
  `upsert_rows`, the upsert of a `VALUES` list (`ON CONFLICT`, `ON DUPLICATE KEY`, `MERGE`)

`platypus_sql::dml` builds the data editor's `UPDATE`, `INSERT` and `DELETE` statements from
text values and the columns' Arrow types, and `dml::apply` runs them all or none.
`platypus_transfer::import` copies a data file's rows into a table the same way, read through a
DuckDB connection on the file (`platypus_driver_duckdb::files`, which also opens data files as
databases); a new file format is a reader there.
`platypus_sql::analyze` tells what a statement does (query, DML, DDL) and which table an
unfiltered `UPDATE`, `DELETE` or `TRUNCATE` changes in every row; the checks before running
(`apps/desktop/src/guards/`) use it. `platypus_sql::source_table` tells which table a `SELECT` reads, when its rows are that
table's, so the editor can change the results of such queries too.
`platypus_sql::pg_literal` splits PostgreSQL array and composite literals into their
elements and joins them back, for the data editor's list of elements; `ColumnMeta.fields`
names a composite type's fields.

Add dialect rules there, with tests. `apps/desktop/src/sqlNames.ts` mirrors `quote_ident`
for the UI.

`platypus-schema` writes the DDL of the table editor, and the object tab's comment changes
through it. A new dialect adds its branch to `Ddl::table_comment` and `Ddl::comments`
(`crates/platypus-schema/src/ddl.rs`); a database without comments returns `None` from
`table_comment` and goes in `NO_COMMENTS` in `apps/desktop/src/objects/ObjectTab.tsx`, which
keeps its comment fields read-only. The forms for views, functions, triggers and sequences
go through `platypus_schema::create_object` (`crates/platypus-schema/src/objects.rs`): a new
dialect adds its branch to each object kind there, or returns the error the dialog shows, and
its driver goes in the `CREATABLE` list of `apps/desktop/src/schema/actions.ts`. A new
migration tool for *Migration status…* adds a `Tool` to
`crates/platypus-schema/src/migrations.rs`: how its files are found and how its history table
is read.

### Drivers without SQL (MongoDB, Redis)

A driver whose language is not SQL still implements `Connection`: `execute` takes the
statement in its own language and returns Arrow batches. `platypus-driver-mongo` is the
example:
- `shell` parses the mongosh subset into a `Command`, and `split`/`statement_at` cut a script
  into statements; `kind` and `unfiltered` feed the checks before running (the desktop
  `analyze_sql` command calls them instead of `platypus_sql` for `DriverKind::Mongo`).
- `docs` turns documents into batches: one column per top-level field of the first batch,
  nested values as JSON text, the BSON type in the `META_DB_TYPE` field metadata
  (`document`, `array`, `objectId`, ...), which the UI reads (`DocumentPanel`).
- `generate` writes the navigator's object statements in mongosh; the desktop
  `generate_sql` and `dialect_support` commands route there.
- `Dialect::from(DriverKind::Mongo)` maps to an inert dialect: commands that build SQL
  (`lint_sql`, `format_sql`, `query_source`) return early for MongoDB, and the UI hides SQL-only
  features with `isMongo` (`apps/desktop/src/api.ts`): the data editor, explain, the visual query
  builder, data search and the grid's SQL filters. `sqlNames.ts` writes the data viewer's query
  (`viewerSql`) and collection references in mongosh; `editor/mongo.ts` is the editor language.

Conformance scenarios that are not SQL-specific have a MongoDB version in
`crates/platypus-conformance/tests/mongo.rs`.

`platypus-driver-redis` follows the same pattern for Redis commands (`command` parses and
classifies a line; `analyze_sql`, `only_reads` and `query_source` route there), with two
additions:
- its tables are virtual: each key type, read by the client-side `BROWSE` command
  (`browse`); `query_source` maps a `BROWSE` to its table, so the data editor loads the
  table's key columns from `table_details` as for SQL tables;
- grid edits are not SQL: `edit::script` turns the changes into commands, which the desktop
  `preview_changes` and `save_changes` use instead of `platypus_sql::dml` for Redis, and the
  session's `execute_all` runs them as one `MULTI`/`EXEC` on the database a leading
  `SELECT n` names.

`platypus-driver-search` (Elasticsearch and OpenSearch, `DriverKind::Elasticsearch`) is the third:
- the editor language (`command`) is Kibana's console form (`GET /index/_search` and a JSON body) plus
  SQL for the product's SQL endpoint; `split`, `statement_at`, `parse` and `Command::kind` (`Query`,
  `Dml`, `Ddl`, `Other`, from the method and the API called) feed the checks before running;
- a cluster is one schema, its indices the tables (aliases and data streams views), the columns the
  fields of the mapping; `browse` writes the statement that opens an index;
- searches return documents as the MongoDB driver does (`doc`), and page with a point in time (a
  scroll on OpenSearch) as the grid asks; SQL results page with the cursor;
- `edit::script` turns the grid's changes into `_update`, `_create`, `_doc` and `DELETE` requests, run by
  `execute_all` one by one, with the requests that undo them.

`platypus-driver-cassandra` (Apache Cassandra and ScyllaDB, `DriverKind::Cassandra`, on the `scylla`
crate) is the fourth, and the only one whose language is SQL-like:
- the editor language is CQL: `cql::split` cuts a script (`$$` strings, `//` comments, a
  `BEGIN BATCH … APPLY BATCH` is one statement) and the desktop `analyze_sql`, `only_reads` and the
  MCP `check_cql` read each statement with the standard SQL analysis (`Dialect::Ansi`, which `Dialect::from`
  gives the driver), so `TRUNCATE` and the writes without `WHERE` are flagged as for SQL; the
  data viewer's rewrites (`rewritable`) are off, as CQL has no `OFFSET` and puts `LIMIT` before
  `ALLOW FILTERING`;
- keyspaces are the schemas; `meta` reads `system_schema` (Cassandra 3+ and ScyllaDB share it) for tables,
  materialized views, types, functions and aggregates, the partition and clustering keys (the
  primary key, with the role of each column in its comment) and the secondary indexes, and rebuilds
  the DDL from it;
- results are typed Arrow columns (`columns`; `varint`, `decimal`, uuids and durations as text,
  collections, tuples and user-defined types as JSON text), read page by page with the server's paging state;
- the connection reaches every node at the address it was given (`session`): clusters advertise
  addresses a container, a NAT or an SSH tunnel hides;
- `edit::script` turns the grid's changes into `UPDATE`, `INSERT` and `DELETE` statements by the full
  primary key, with the statements that undo them; the desktop `preview_changes` and `save_changes` run
  them one by one through `execute_all`, as for Elasticsearch.

The UI hides SQL-only features with `isNoSql` (MongoDB, Redis, Elasticsearch and Cassandra), and keeps
MongoDB results out of the data editor with `isMongo`; `editor/redis.ts` is the Redis editor language
and `editor/cql.ts` the CQL dialect of CodeMirror.

### Drivers loaded at run time (ADBC)

Snowflake, BigQuery, Databricks and Trino are reached through ADBC drivers that the app
downloads instead of building in:
- `DriverKind::bridge()` says a kind needs one, `DriverKind::adbc_package()` names its package
  in the `dbc` driver registry. `ConnectionConfig::options` / `SavedConnection::options` hold the
  fields of the kind beyond host, port, database and user (file version 6).
- `platypus-drivers` reads the registry's `index.yaml` (cached under `<data>/drivers/`), picks
  the newest package for the platform, refuses `LicenseRef-*` licenses, checks the SHA-256 and
  unpacks to `<data>/drivers/adbc/<name>/<version>/` (library named by the package's
  `MANIFEST`, recorded in `driver.toml`). `set_data_dir` is called at start by the app and by
  `platypus-mcp`; agents never download.
- `platypus-driver-adbc`: `profile::options` turns a config into the driver's database options
  (one function per kind, with tests); `session` runs the ADBC connection on a thread of its
  own; batches cross from arrow 59 (ADBC 0.24) to arrow 60 as IPC (`ipc`); `GetObjects` feeds
  the navigator and autocompletion (`meta`). A missing driver is `Error::DriverMissing`, which
  the desktop sends as the `driver_missing` error kind with the kind in `code`:
  `withDriver` (`apps/desktop/src/drivers/DriverPopup.tsx`) shows the download popup and runs
  the command again.
- Adding a database: a `DriverKind` with its package, a branch in `profile::options` with a
  test, its fields in `apps/desktop/src/drivers/profiles.ts`, a tile in `DriverPicker.tsx`,
  and its dialect (`Dialect::Ansi`, `Snowflake`, `BigQuery` or `Databricks`, or a new one).
- Cancelling: ADBC drivers written in Go do not stop a running statement on a connection-level
  cancel, and the statement's own cancel waits for the lock `execute` holds; a statement still
  waiting to start is given up with its session (`is_closed`), and the app opens another.

Conformance: `crates/platypus-conformance/tests/bridges.rs` runs Trino (`--profile trino`)
through the bridge, downloading its driver to `target/conformance-drivers`.

### Vendor client libraries: Oracle, ODBC and DuckDB

Oracle, IBM Db2, DuckDB and any ODBC driver need a library that the vendor publishes and the
app installs, or that the system has:
- `DriverKind::vendor_package()` names it (`oracle`: Oracle Instant Client, `db2`: IBM's
  Data Server Driver for ODBC and CLI, `duckdb`: DuckDB's shared library).
  `platypus_drivers::vendor_package` describes it per platform: archive URLs with the
  SHA-256 the vendor publishes (or computed once for versioned archives), the library, and
  for an ODBC driver the program that registers it with the system. `install_vendor`
  downloads, checks and unpacks to `<data>/drivers/vendor/<name>/<version>/` (zip archives
  with their folders and symbolic links; `.tar.gz` ones flat);
  `register` runs the vendor's program as administrator through PowerShell `-Verb RunAs`
  (Windows shows the UAC prompt; a declined prompt is `DriverError::ElevationDeclined`).
  A new version means new URLs and hashes in `vendor.rs`.
- `platypus-driver-oracle`: the `oracle` crate (ODPI-C) loads Instant Client from
  `<data>/drivers/vendor/oracle/…` with `InitParams::oracle_client_lib_dir`, or the
  system's client without one; with neither, `Error::DriverMissing`. A connection runs on a
  thread of its own; the canceller shares it (`Arc<Connection>`) and calls
  `break_execution`, so autocommit is done by the session (a commit after each change)
  instead of the connection's own flag, which needs `&mut`. Metadata comes from the `ALL_*`
  views (`meta.rs`), DDL from `DBMS_METADATA`. A `;` after plain SQL is dropped
  (`statement_text`), PL/SQL keeps it; the splitter ends PL/SQL at a `/` line.
- `platypus-driver-odbc` (Windows only: unixODBC would have to be linked into the app, which
  would then not start without it; elsewhere the driver reports it): `odbc-api` and
  `arrow-odbc`, one thread per connection, metadata from the ODBC catalog functions,
  `profile::connection_string` per kind (Db2's vendor driver found by name among the
  system's drivers; generic: a DSN, a driver and a connection string). `SQLCancel` needs
  unsafe code, which the workspace forbids: a statement still waiting to start is given up
  with its session, as with ADBC.
- DuckDB: the `duckdb` crate is built with `loadable-extension`, as for a DuckDB extension:
  it calls DuckDB through a table of function pointers instead of linking it, so the app
  carries no DuckDB. `platypus-duckdb-loader`, the only crate where `unsafe` is allowed
  (`deny`, each block with its SAFETY note), loads `libduckdb` and fills that table by
  name (`api_functions.rs`, generated from libduckdb-sys's loadable bindings; a missing or
  unknown name fails the build). `platypus_driver_duckdb::library::load` finds the library
  under `<data>/drivers/vendor/duckdb/…` before a database or data file opens, else
  `Error::DriverMissing`. The library must be the DuckDB version of the bindings: updating
  the `duckdb` crate (pinned with `=`) means regenerating `api_functions.rs` and new URLs and
  hashes in `vendor.rs`. Tests call `platypus_driver_duckdb::testing::load()` (feature
  `testing`), which downloads the library once into `target/test-drivers` for the whole
  workspace, or loads `PLATYPUS_DUCKDB_LIBRARY` offline; the E2E runner links that copy into
  each profile unless a scenario sets `duckdb = false`.
- Oracle and ODBC run a bare `BEGIN`, `COMMIT` or `ROLLBACK` through their client API
  (`platypus_core::TxControl`): Oracle reads `BEGIN` as a PL/SQL block, and ODBC drivers take
  it differently.
- Desktop: `commands/vendor.rs` (`vendor_driver_status`, `install_vendor_driver` with
  progress and a `registering` event, `cancel_vendor_install`, `remove_vendor_driver`,
  `odbc_system`); `apps/desktop/src/drivers/VendorDrivers.tsx` holds the popup, which
  `askDriver` opens for these kinds, the client's line in the connection dialog and the
  Settings › Drivers section.

Conformance: `tests/oracle.rs` (`--profile oracle`, `gvenzl/oracle-free`; skipped without
Instant Client, which `PLATYPUS_TEST_ORACLE_CLIENT=download` fetches to
`target/conformance-drivers`) and `tests/odbc.rs` (SQL Server through Windows' own `SQL Server`
ODBC driver, `--profile sqlserver`).

### Connections file

`platypus_store::ConnectionStore` keeps `connections.toml`, now `version = 6`: a list of
`[[projects]]` (`id`, `name`, `color` as `#rrggbb`, `initials`, `connections`, the ids of
its connections, with their `folders` and `placement`), then the `[[project_folders]]` of
the rail, then the `[[network_profiles]]` (an SSH tunnel and SSL settings that
connections reference by `network_profile`; `ConnectionStore::effective` puts them in place
of the connection's own), before the `[[connections]]`. A connection can be in several
projects, or in none. Older files are read and written back as version 6; a file with a
newer version is refused, so an older app never drops what it cannot read. Ids of
connections that no longer exist are dropped when the file is opened, and deleting a
connection removes it from every project. Version 4 added each connection's
`agent_access` (`off`, `read_only` by default, `read_write`); see [Agents](#agents-mcp).
Version 5 added folders (`connections/folders.rs`): a project's `folders` (`id`, `name`,
`parent`) and `placement` (connection id → folder id), and the rail's `project_folders`
(`id`, `name`, `color`, `projects`, `collapsed`); a project is in one rail folder at most.

The projects' commands are in `commands/projects.rs`: `list_projects`, `save_project`,
`delete_project`, `set_project_membership` and `project_initials`; for folders
`create_connection_folder`, `rename_connection_folder`, `move_connection_folder`,
`delete_connection_folder` and `place_connection`, for the order of a project's connections
and folders (drag and drop, Alt+Up/Alt+Down: `projects/arrange.ts` computes the change)
`arrange_connection`, `arrange_connection_folder` and `group_connections`, for the order of
All connections `reorder_connection`, and for the rail `list_project_folders`,
`group_projects`, `save_project_folder`, `set_project_folder`, `ungroup_projects` and
`reorder_projects`, which return a `Workspace` (connections, projects and rail folders).
`save_connection` takes the projects a connection joins. Imports (`platypus_store::import`)
read each file into `ImportedProject`s (a DBeaver `.dbp` gives one per project), and
`save_imported` saves the ticked part as an `ImportRequest` in one write
(`connections/imports.rs`).

### Agents (MCP)

`crates/platypus-mcp` lets external agents use the saved connections. Its library is the
`Agent` (`agent.rs`), with one method per tool; `stdio::main` serves them over stdio with
`rmcp` (`server.rs`). The app does so when started with `--mcp` (`main.rs` of the desktop
crate, before Tauri starts: no window), and so does the `platypus-mcp` binary, kept for
development and the stdio tests; and the `platypus` CLI calls the same methods for
`--connection` (`crates/platypus-cli/src/agent.rs`). The pieces:
- `Profile` finds the app's folders the way Tauri does (`dirs::config_dir()` and
  `dirs::data_dir()` with the bundle identifier), or `PLATYPUS_DATA_DIR`. It reopens
  `connections.toml` at every call, so a change in the app applies at once; with a master
  password the `VaultSecrets` stay locked, and a connection needing them fails with
  `AgentError::Locked`.
- `connect.rs` opens a saved connection like the app: `ConnectionStore::effective` (network
  profile), the password from `platypus_store::external_password` or the store, the SSH
  secrets, the bootstrap statements, and `platypus_net::connect`. A DuckDB session for
  agents then turns external access off (no `read_text`, `glob`, `COPY` to files or
  `INSTALL`; a data file may read only itself, through `allowed_paths`) and locks its
  configuration. Connections stay open between calls, one per saved connection, and are
  reopened when the fingerprint of their effective settings changes; DuckDB's close after
  each call, since its file lock would keep the app out.
- `access.rs` decides what runs. `SavedConnection::effective_agent_access` caps production
  and read-only connections at read-only. `access::check` takes one statement
  (`platypus_sql::split`) and runs `platypus_sql::analyze`: read-only access runs only
  `StatementKind::Query`, and refuses by name the functions a read-only transaction lets
  act outside it (PostgreSQL's `pg_terminate_backend`, `pg_reload_conf`, `pg_notify`,
  `dblink`, `lo_export`, advisory locks…; MySQL's `LOAD_FILE`, `INTO OUTFILE`); read-write
  refuses transaction control and, unless the `safety.unfiltered` setting is `off`, the
  `unfiltered` changes the app would confirm.
- `query.rs` runs the statement within `Limits` (rows, bytes, timeout; the timeout cancels
  through the connection's `CancelHandle`). On read-only access it runs between
  `Dialect::read_only_begin` and `Dialect::read_only_end`: a `READ ONLY` transaction where
  the database has one (`Dialect::refuses_writes_when_read_only`), else a transaction that
  is always rolled back. DuckDB and SQL Server have no read-only transaction: a write that
  got past the checks runs and is rolled back, holding its locks until then; SQL Server
  waits 5 seconds at most for a lock meanwhile (`LOCK_TIMEOUT`), and effects outside the
  transaction are not undone. A new driver adds its case to those `Dialect` methods; the
  conformance test `agents_read_in_read_only_transactions` checks it.
- `output.rs` renders a result as an aligned text table or the JSON documented in
  [AGENTS.md](AGENTS.md); the whole rendering stays within the byte limit, rows
  dropped from the end until it fits. Change that JSON only by adding fields.
- `redact.rs` masks the connection's password, SSH secrets, password command and the
  password of any URL in every error an agent reads. Tools never return the host, the user
  or SSH settings.
- `link.rs` is the way to the open app: the `AppLink` trait (`sessions`, `confirm`,
  `hello`). `HttpLink` reaches the app's bridge from another process through
  `agent-bridge.json`; the app's own `/mcp` endpoint uses the bridge's `Link`
  (`apps/desktop/src-tauri/src/agent/`) directly. `Agent::with_link` gives one to the agent:
  with `agents.confirm_writes` on (the default), a statement `access::changes` sees as a
  change asks `confirm` before it runs, with the plan's estimate, and is refused without a
  link or a yes. A new client of the app (another process) speaks the bridge's routes,
  documented in [AGENTS.md](AGENTS.md).

Every `run_query` is recorded in the query history with `origin`, the MCP client's name
(`clientInfo.name`, trimmed to 64 characters, `agent` when empty) or `platypus-cli`; `HistoryQuery::origin` filters the app's entries
(`NULL`) or the agents'. A new tool is a method on `Agent`, a `#[tool]` in `server.rs` and,
for the CLI, a subcommand. `commands/agents.rs` gives the Settings' Agents page
(`apps/desktop/src/agents/`) the app's path (the AppImage's on Linux) and the commands
for Claude Code and Codex, which start it with `--mcp`. The installers hold one executable:
the server is in the app, with the drivers it already has.

### Settings storage

`platypus_store::SettingsStore` stores `settings.toml` under dotted keys. Rust code reads a
value with `get::<T>("section.key")`. The keys, types and defaults are declared in the UI,
as described under [Settings](#settings).

## Frontend

### Actions: menus and shortcuts

`apps/desktop/src/actions.ts` is the registry of user commands. An action has:
- an id;
- a scope: where it applies and what argument it receives;
- a title;
- optionally a shortcut, a menu group and order, and a `when` condition;
- optionally `disabled`, which returns a reason (a `msg` text) not to run: context menus and
  the palette still list the action, greyed out with the reason (a menu item's tooltip, a
  palette entry's detail); `runAction` does not run it, and its shortcut is swallowed, so no
  other handler acts on the keys.

Context menus, shortcuts and the command palette read from the registry: global actions
appear in the palette by their title. The user can change any action's shortcut in the
Settings; it is stored under `shortcuts.<action id>`, and `shortcutsOf(action)` gives the
ones in effect.
Actions with the same `submenu` title are listed together under that entry, e.g. `Copy as`.

```ts
registerAction({
  id: "grid.copy_as_insert",
  scope: "grid",
  title: "Copy as INSERT",
  group: "copy",
  order: 20,
  when: ({ grid, source }) => grid.selection() !== null && source?.table != null,
  run: ({ grid, source }) => { /* … */ },
});
```

| Scope | Argument | Shown in |
|---|---|---|
| `global` | — | shortcuts anywhere, including in the editor |
| `navigator.list` | `{ project }` (`null`: All connections) | navigator menu, below the rows |
| `navigator.connection` | `{ connection }` | navigator menu |
| `navigator.database` | `{ connection, database, name }` | navigator menu |
| `navigator.schema` | `{ connection, database, schema }` | navigator menu |
| `navigator.object` | `{ connection, database, schema, object, target }` | navigator menu |
| `navigator.column` | `{ connection, database, schema, object, column }` | navigator menu |

`database` is `null` for the connection's saved database. Otherwise it names another
database of the same server (PostgreSQL), and sessions and tabs opened from there use it.
| `grid` | `{ grid: GridApi, source }` | grid menu, and shortcuts while the grid has focus |
| `editor` | `{ view: EditorView, driver, readOnly, host? }` | editor menu, and shortcuts while the editor has focus (before CodeMirror's own keymaps). `host` is what the tab offers (`EditorHost` in `editor/SqlEditor.tsx`): open an object, run with the CLI, run into a new result tab |
| `project` | `Project` | menu of a project's square in the rail |
| `tab` | `{ tab }` | menu of a tab of the bar (pin, open beside, close others…) |
| `script` | `{ entry, project, root, connections, rename }` | menu of a saved script or folder in the Scripts panel |

To add a scope, augment `ActionScopes` from the feature's own module:
`declare module "../actions" { interface ActionScopes { … } }`.

Put a feature's actions in its own module and import that module once. For example,
`grid/actions.ts` holds the grid's built-in Copy and Select all.

### Tab kinds

`apps/desktop/src/tabs/registry.ts` defines tab kinds. A kind has:
- a component;
- `create`, which builds the tab from what the opener passes;
- `label`;
- `persist` and `restore`, for the workspace saved across restarts.

To add a kind:
1. Augment `TabKinds` with its model, which extends `TabBase`.
2. Call `registerTabKind` in `tabs/<kind>.ts`.
3. Import that file in `tabs/index.ts`.

Open a tab of the new kind with `workbench().openTab("<kind>", connectionId, init)`. The SQL
editor (`kind: "query"`) is the reference implementation, in `components/QueryTab.tsx` and
`tabs/query.ts`.

### Workbench

`workbench()` (`apps/desktop/src/workbench.ts`) is how actions and tabs reach the shell
without props. It provides:
- the connections, and `setConnections` for commands that return the changed list;
- `openTab`;
- `openTemporaryTab`, a tab on a connection that is not saved (the in-memory DuckDB of "Query
  with SQL"): the connection lives while one of its tabs is open, then its `onClosed` runs; its
  tabs are not kept for the next start;
- `sideSession`, the navigator session of a connection and database;
- `capabilities`, what an open session's driver supports;
- `showError`;
- `openSettings`;
- `openDialog`, which shows a dialog from an action. Its `render(close)` returns the dialog;
  for a yes/no question, `ask(message, { confirm, danger })` from `ui/ask.tsx` opens one and
  resolves to the answer (never `window.confirm`, which has no app style and no test hook);
- `projects`, `setProjects`, `activeProject` and `switchProject`, and `editProject` and
  `deleteProject`, which open the project dialog or ask before deleting.

A dialog is a `.modal` element: `ui/modalA11y.ts` gives every one its `dialog` role, a name
from its first heading, the focus inside and back after, and Tab kept within it, so a new
dialog needs none of that itself. Work in the background that the user may want to follow
goes through `track(label, promise)` of `shell/activity.ts`, and shows in the Activity log
(the errors shown with `showError` go there too).

### Interface language

The interface speaks English, Italian, Spanish, German, French, Portuguese (Brazil) and
Chinese (`i18n.ts`, setting `appearance.language`; "System" follows the system's language).
Text is written in English and passed to `t`, which looks it up in the language's dictionary,
`i18n/<code>.ts`, keyed by the English text:

```ts
t("Delete the connection \"{name}\"?", { name: c.name });
plural(n, "{n} row", "{n} rows");
tNodes("{star} matches any text.", { star: <code>*</code> }); // React nodes in the text
```

Action titles and setting labels, sections and descriptions stay in English where they are
declared: the menus, the palette and the Settings translate them when they show them. Text
declared at load time anywhere else (a table of labels, a helper's argument) is marked with
`msg("…")`, which returns it as it is, and translated with `t` where it is shown.
Plurals follow the language's rules (`Intl.PluralRules`) and numbers its locale (`formatCount`,
`locale()`). Italian is in the bundle; the other dictionaries load when their language is
chosen (`loadLanguage`). A new language is a dictionary with every key of `it.ts`, its code in
`Language`, `LOCALES`, `LOADERS` and the setting's options, and its keyboard's key names in
`KEY_LABELS` (`actions.ts`).
`npm run build` runs `scripts/i18n-check.mjs`, which fails on text missing from a dictionary,
or on an entry nothing uses; `node scripts/i18n-check.mjs --untranslated` lists the text left in
JSX without `t`. Messages from the databases and the backend stay as they come.

### Projects

`apps/desktop/src/projects/` holds the rail (`ProjectRail.tsx`), the project dialog, the
dialog that adds saved connections to a project, and the actions (`actions.ts`): the
`Ctrl+1`… switches, next and previous project, the `project` scope of a square's menu, and
"Add to project" and "Remove from this project" in the navigator. `state.ts` filters the
connections of a project and remembers the active one.

Every tab has a `project` (`TabBase.project`, `null` for All connections). All tabs stay
mounted; the tab bar shows those of the active project, and each project remembers its
active tab. The workspace is saved as `platypus.workspace.v2` in `localStorage`, with the
active tab per project; a v1 workspace is read once and its tabs go to All connections. A
new tab joins the active project, so features that open tabs need nothing more.

### SQL editor

`apps/desktop/src/editor/` holds the editor (`SqlEditor.tsx`) and what it is built from:
completion sources (`joinCompletion.ts`, `functions.ts` for the built-in functions of each
dialect, `snippets.ts`), the linter (`lint.ts`), spell checking (`spellcheck.ts`), the
terminal mode (`terminal.tsx`) and the side panels. A completion source is added in
`language()` in `SqlEditor.tsx`.

Statements come split by the backend (`analyze_sql`). A line starting with `@` at a
statement start is a client command: it ends at the line and never reaches the database.
`parseCommand` in `editor/params.ts` reads it and the run loop in `QueryTab.tsx` runs it; a
new command is a case in both. Placeholders (`:name`, `?`, `${name}`) come with each
statement (`StatementSpan.parameters`) and `substitute` replaces them before the statement
runs.

Snippets are stored as JSON in the settings under `editor.snippets`. Saved scripts are
`.sql` files in `scripts/<project id>` (`scripts/all` for All connections) in the app's data
folder, with the connection links in `.links.json`; the commands are in
`commands/scripts.rs` and the panel in `apps/desktop/src/scripts/`.

### Navigator refresh

After changing objects, call `requestNavigatorRefresh({ connectionId, database, schema })`
from `navigatorEvents.ts`. The navigator reloads that part of the tree, and autocompletion
and open object tabs reload as well. Listen with `onNavigatorRefresh` to react to it.

### Settings

Declare a setting next to the code that reads it:

```ts
export const scriptRowLimit = defineSetting({
  key: "editor.script_row_limit",
  section: "SQL editor",
  label: "Rows fetched per statement when running a script",
  default: 1000,
  control: { type: "number", min: 1 },
});

getSetting(scriptRowLimit);  // in handlers
useSetting(scriptRowLimit);  // in components, re-renders on change
```

The Settings dialog (Ctrl+,) lists every declared setting, grouped by section in the order
they are declared. Settings are loaded before the first render; `onSettingsChanged` calls a
function after any change, for code outside React. Controls are `boolean`, `number`, `text`,
`select`, `color` (a `#rrggbb` value, empty for none) and `swatches` (choices shown as cards
of colors).

### Theme

`apps/desktop/src/theme.ts` owns every color: the presets in `PRESETS` and the Appearance
settings over them become CSS variables on `<html>` (`--bg`, `--fg`, `--accent`, the grid's
and the syntax colors, `--radius`, `--grid-row-h`, `--editor-font-size`). Stylesheets use
the variables, never fixed colors. A new preset is one more entry in `PRESETS`. Code that
caches colors, like the grid's canvas, reads them again on `THEME_EVENT`.

A preset gives the base colors (`BasePalette`) and may give the extra ones (`ExtraPalette`):
`--bg-rail`, `--bg-tabbar`, `--fg-strong`, `--border-strong`, `--active`, the object colors
`--obj-db|table|view|routine|trigger` and the connection types
`--type-dev|test|prod` with their `-bg`. What a preset leaves out comes from `derive()`,
which also follows the custom background, sidebar, text and accent colors. "System" stands
for `platypus_dark` or `platypus_light`. The last applied variables are cached in
`platypus.appearance.v2`; a change to the variables' set bumps that key.

### Stylesheets, fonts and icons

`src/styles/` holds one file per area, imported in order by `index.css`: `base` (reset,
fonts, buttons, inputs, `kbd`), `shell` (layout, tabs, status bar), `navigator`, `editor`
(toolbar, connection chip, history, object tabs), `grid` (results, data editor, panels) and
`dialogs` (modals, Settings, palette, menus). A group adds its rules to the file of its area.

Inter and JetBrains Mono (Latin and Latin Extended, OFL) are in `src/assets/fonts/`, exposed
as `--font-ui` and `--font-mono`; the grid's canvas reads `--font-mono` too and redraws when
the fonts finish loading.

Icons come from Lucide through `src/ui/Icon.tsx`: `<Icon name="table" />`, and
`objectIcon(kind)` for a database object. A new icon is one more name in its map; components
never import `lucide-react` themselves.

### Result grid

`DataGrid` (`apps/desktop/src/grid/DataGrid.tsx`) takes these extension props:

| Prop | Purpose |
|---|---|
| `decorate(row, col)` | Background, colour or replacement text for a cell, e.g. edited values or search hits. It runs for every visible cell on each frame, so keep it cheap. |
| `overlay` | Elements positioned over the grid, e.g. an inline editor placed with `GridApi.cellRect`. |
| `onCellDoubleClick` | Called when a cell is double-clicked. |
| `topRows` | Rows drawn above the result's, e.g. new rows; their cells come from `decorate`. |
| `onScroll` | Called when the grid scrolls, e.g. to move an overlay. |
| `onFocusChange` | Called when the focused cell changes, e.g. for the value panel. |
| `onHeaderHover` | Called when the mouse comes onto a column header (its column and box) and with `null` when it leaves, e.g. for the column statistics (`analysis/ColumnProfile.tsx`). |
| `onSelectionChange` | Called when the selection changes, e.g. for its totals. |
| `layout`, `onLayoutChange` | The column layout (`grid/layout.ts`): order, hidden and pinned columns, widths. Without `onLayoutChange` the columns keep the result's order. |
| `ref` | Gives the `GridApi`. |

Cells and selections always use result columns, wherever the layout shows them: a
selection lists its columns in `cols`, left to right as shown, and `visibleColumns()` gives
the display order. `GridApi.dataRow` maps a grid row to the result's row (`null` for the top
rows). `grid/ResultGrid.tsx` hosts a result's features around the grid: the data editor
(`grid/edit/`), search, totals, column, copy, filter and foreign key actions
(`grid/features/`), the value, grouping, metadata, calc, map and references panels, the
record view and the row comparison (`panels/`, `grid/features/references.tsx`). It combines their decorations, so each feature
keeps its own.

A decoration can set `background`, `color`, replacement `text`, `strike` and a muted `hint`
after the value (the data editor's foreign key labels). Features that
need their own object in grid actions add a field to `GridSource` by augmenting
`GridSourceExtensions`, as the data editor does with `edit` (`grid/edit/TableEditor.tsx`).

The `GridApi` reads values and the selection, moves the selection, gives cell boxes,
redraws and gives the keyboard back to the grid. It is also what `grid` actions receive,
together with `source`: the connection, the SQL, the job and the data-viewer table.

## End-to-end tests

`apps/desktop/e2e/` drives the release build through the WebView2 DevTools protocol, on
Windows only. Each scenario runs against a fresh profile: `PLATYPUS_DATA_DIR` and a separate
WebView2 folder. The user's data is never touched. The runner sets `PLATYPUS_DEVTOOLS_PORT`,
which makes the app open the protocol port itself. With it set, `app_info().devtools` is
true and the app exposes test hooks on `window.__platypus`, such as `switchProject(id)`.

```bash
npm run tauri build -- --no-bundle
npm run e2e              # all scenarios
npm run e2e -- postgres  # file names containing "postgres"
npm run e2e -- --skip qa-perf,oracle  # all but file names containing "qa-perf" or "oracle"
```

`PLATYPUS_E2E_SKIP=qa-perf,oracle` does the same as `--skip`, and the two can be combined
with a filter (`npm run e2e -- postgres --skip arrays`).

A scenario in `e2e/scenarios/` exports:
- `default(page, ctx)`, which runs the steps;
- optionally `setup(ctx)`, which seeds `ctx.dir` (for example `connections.toml`) before
  the app starts;
- optionally `skip()`, which returns a reason not to run, for example when a dev database
  is down;
- optionally `env(ctx)`, extra environment variables for the app;
- optionally `ready`, the condition to wait for before the steps (by default the navigator
  showing a connection; a scenario with no saved connections waits for something else).

The runner writes `language = "en"` into the scenario's `settings.toml`, so scenarios read
English text on any machine; a scenario that sets `language` itself keeps its own.

Set `PLATYPUS_E2E_SHOTS` to a folder to get a screenshot of each failure, plus the ones the
scenarios take: `visual-sqlite` saves the main screens in Platypus Dark and Light, to compare
a change to the look by eye. CI runs the
scenarios on Windows. Scenarios that need the Docker databases skip themselves there.
