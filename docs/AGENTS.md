[← README](../README.md)

# Agents (MCP) and the CLI

Claude Code, Codex, Cursor and other MCP clients use your saved connections through
PlatypusDB's MCP server: the app itself, started with `--mcp`, which they start themselves
(no window opens, and the app need not be open for reading).

- Each connection gives agents **no access**, **read only** (the default) or **read and
  write**; production and read-only connections are never more than read only. A connection
  that comes new from a shared project starts with **no access** on your computer, whatever
  the project file says, and the file never raises it later; what you choose stays on your
  computer and is not written back to the team's file.
- **Edits outside the app wait for you**: `connections.toml` and `settings.toml` are files a
  coding agent with file tools can write, so the app keeps what you allowed agents (agent
  access, "Ask before agents change data", the Safety setting for `UPDATE` or `DELETE` without
  `WHERE`) in `agent-policy.json`, sealed by its SHA-256 in the system keychain. Agents get
  what the files say, never more than what is sealed; an edit that gives them more is asked
  about when the app starts or its window gets focus (accept it, or undo it), and an edit that
  gives them less applies at once. Without a keychain there is no seal, and the files are
  taken as they are.
- Passwords stay in the keychain and no answer shows a host, a user or a secret.
- Agents' queries are in the history with the agent's name, and every tool call is in the
  agents' call log (Settings → Agents → Activity), by session, with a filter and a CSV export.
  Above the list, a summary per agent and per session (calls, tables read, rows returned,
  changes allowed and refused, errors) follows the same filters and exports as Markdown or HTML.
  See [The agent report](#the-agent-report).
- An agent can propose SQL with `propose_sql`: it opens in a new editor tab, for you to
  review and run. With `propose_edits` it proposes rows to insert, update or delete in a table: they
  open in the table's data viewer as pending edits, for you to save or discard.
- **Changes wait for you**: on read-write connections each change an agent wants to run opens
  a dialog in the app with the statement, the rows the database expects it to change and its
  plan. Refuse it (with a reason the agent reads), allow it once, or allow that agent's changes
  on the connection for the rest of its session; after five minutes without an answer it is
  refused. With the app closed, agents' changes are refused. A query that writes through a
  function counts too: where the database refuses writes in a read-only transaction
  (PostgreSQL, MySQL, MariaDB, SQLite) queries run in one first, and the write is asked about.
  A query that calls what acts outside the transaction (`dblink_exec`, `pg_terminate_backend`,
  Trino's `query()`, Snowflake's `SYSTEM$` functions, Oracle's packages: the names read-only
  agents are refused) is asked about before it runs, as a change.
  Settings → Safety → "Ask before agents change data" turns the questions off.
- While the app is open, agents reach it through a **bridge** on a local port with a token:
  the top bar shows the agents connected, and the `list_open_sessions` tool shows them the
  sessions open in the app (connection, database, manual commit, open transaction).
  Settings → Agents → Setup turns the bridge off, changes its port (47337 by default), shows
  its address and token for MCP clients over HTTP and makes a new token.
- An agent's statements run on **one database session per connection**, kept open between
  calls: a MySQL or MariaDB `@variable`, a temporary table or a `SET` made by one call is there
  in the next (PostgreSQL and SQL Server keep theirs the same way; DuckDB closes its file after
  each call, so it keeps none). The session is shared by the calls of every MCP client of the
  running server, and starts again when the server restarts, the connection's settings change,
  the server drops it, a call times out or is cancelled, or a transaction is left open. Each
  `platypus` CLI command is a process of its own, so it never keeps session state from one
  command to the next.

Settings → Agents → Setup lists Claude Desktop, Claude Code, Codex, Cursor, Windsurf,
Antigravity and Gemini CLI with their configuration file and whether PlatypusDB is in it:
**Install** writes the server there, after asking and after a copy of the file
(`<file>.platypus-backup-<date>`), keeping the rest of the file as it was. The same entry is
shown to copy by hand, and so are the commands that register the server. Security lists the
access of each connection. Agents
registered with `platypus-mcp.exe` by version 0.2 need registering again with these commands:
the installers no longer ship that second executable.

```bash
claude mcp add -s user platypus -- "C:\Program Files\PlatypusDB\platypus-db.exe" --mcp
```

or, with the app open, over HTTP (address and token from Settings → Agents):

```bash
claude mcp add --transport http platypus http://127.0.0.1:47337/mcp --header "Authorization: Bearer <token>"
```

and for Codex, in `~/.codex/config.toml`:

```toml
[mcp_servers.platypus]
args = ["--mcp"]
command = 'C:\Program Files\PlatypusDB\platypus-db.exe'
```

### Ready-made plugin

The repository is also a plugin marketplace for Claude Code: the `platypus-db` plugin adds the
MCP server and a skill that tells the agent how to use the tools (explore first, the limits of
`run_query`, read-only access, `propose_sql` for changes):

```bash
claude plugin marketplace add PaoloCisly/platypus-db
claude plugin install platypus-db@platypus-db
```

The plugin starts `platypus-db --mcp`. Where the app is not on `PATH` (Windows and macOS; the
Linux packages put it there), set `PLATYPUS_DB` to its full path, e.g.
`C:\Program Files\PlatypusDB\platypus-db.exe`, before starting Claude Code. For Codex,
[`plugins/codex`](../plugins/codex) has the `config.toml` entry and the same instructions as an
`AGENTS.md` to copy into `~/.codex/AGENTS.md` or a project's `AGENTS.md`.

<details>
<summary>How the MCP server works</summary>

`platypus-db --mcp` talks MCP over stdin and stdout. It reads the connections of the app's
profile, or of `PLATYPUS_DATA_DIR` when set. With a master password the saved passwords stay
locked to agents: such connections answer with an error saying so, and work with a password
command or `.pgpass` instead. A connection that asks for its password every time is not
available to agents. The bootstrap statements of a connection in a shared project run for
agents and the CLI only once you approved that exact text in the app (which asks at the first
connect): until then they are left out, and the server's log (stderr) says so.

Tools: `list_connections`, `list_schemas`, `list_objects`, `describe_table`, `get_ddl`,
`get_er_diagram`, `compare_schemas`, `search_objects`, `run_query`, `sample_rows`,
`profile_table`, `explain`, `propose_sql`, `propose_edits`, `list_open_sessions`, `list_scripts`, `read_script`,
`search_history`, `estimate_change`, `server_activity`, `top_queries`, `next_page` and
`query_across_databases`. `run_query` runs one statement and returns 100 rows,
32 KB and 30 s at most by default (`max_rows` up to 10,000, `max_bytes` up to 1 MB,
`timeout_seconds` up to 300), as an aligned text table or JSON, saying when rows or values
longer than 1,000 characters were left out. In the text table SQL `NULL` reads `NULL` and a
text `NULL` reads `'NULL'`. `list_objects` returns 500 objects at most (`limit` up to 2,000);
a larger schema comes in pages (`truncated`, `total`, `next_offset`, then `offset`). The
client's `notifications/cancelled` cancels the running statement.

A result that `max_rows` or `max_bytes` cut returns a `cursor` (the `cursor` field in JSON, a
`More rows` line in text; a connection with its own `max_rows` limit gives none, since that
limit bounds the result). `next_page` (`cursor`, optional `max_rows`, `max_bytes`, `format`)
reads the next rows of the same result instead of running the statement again. The first read
keeps the rows after its page, up to 10,000 more rows or 8 MB, inside the statement's usual
read-only transaction, which ends as always: the cursor serves its pages from memory and holds
nothing on the connection, so its pages do not change if the data does. When that cap ended the
result, the answers say so (narrow the query to read on). A page goes through the same masking
(the sensitive columns are decided once, with the first page), the connection's `max_rows` and
`max_bytes`, and counts as a call for `calls_per_minute`. A cursor belongs to the MCP session and
the connection it came from; an unknown, expired (5 minutes after its last use), closed (with its
session; a session keeps 8 and all together 64 MB, the oldest go first), guessed or another
session's cursor is refused, and so is one whose connection agents may no longer use. A result
cut by a timeout or an error has no cursor.

`query_across_databases` (`sources`, `sql`, optional `source_rows`, `max_rows`, `max_bytes`,
`timeout_seconds`, `format`) runs one DuckDB statement over tables of several connections, the join
of the app's "Query across databases". Each source is `connection`, `schema`, `table` and an
optional `name` (how the statement calls it; default the table's name): up to 8, each from a
connection open to agents (access off reads as an unknown connection), read within that
connection's own limits (`source_rows` up to 100,000 rows, 32 MB, its timeout, its calls per
minute and cost guard) and copied into an in-memory DuckDB that exists for the call. The
statement is checked as a read-only statement (no `INSERT`, `COPY`, `ATTACH`, `INSTALL`, no
`read_csv` or any function or `FROM 'file'` that reads files or URLs), and the DuckDB session has
external access off and its settings locked, so it cannot write to a source, the disk or the
network. A source with more rows than were copied is said so in the answer: a join may then miss
rows. Sensitive columns are copied into DuckDB as NULL unless that connection shows them, because
after a join their origin is lost: a join or a filter on one matches nothing (dots, all equal,
would match every row with every other), and `masked_columns` names them. Join on a column that
is not sensitive.
The result keeps to the strictest limits of the sources. If the DuckDB library is not installed
the answer says the app downloads it (on first use of a DuckDB connection, or in Settings >
Drivers); agents cannot.

`get_er_diagram` (`connection`, `schema`, optional `tables`, `depth` and `format`) returns a Mermaid
`erDiagram` as text, built from the catalog, so it works with the app closed: the tables of the
schema with their columns and primary keys, and the foreign keys between them. Without `tables`
it draws the whole schema (the first 300 tables of a larger one); with `tables` it draws those
and the tables linked to them by foreign keys, `depth` hops away in either direction (default
1, at most 3; 0 draws only the named tables). A diagram over 32 KB, or over a lower byte limit of
the connection, is cut at whole tables, with a `%%` comment saying how many were drawn and
suggesting `tables`.

With `format: "svg"` (the default is `mermaid`) the answer is an SVG image, laid out and drawn
with the code of the diagram's own SVG export. The MCP server reads the catalog as for Mermaid,
with the same access, limits and checks, and sends only the model (table, column and type names,
keys and foreign keys) to the open PlatypusDB, which draws it and answers. Unlike
`get_user_context` the app asks the user nothing, opens no connection, and shows no tab or
dialog: the agent could read the same schema anyway. It needs the app open (and answering within
20 seconds): otherwise the call is an error that says so and that `format: "mermaid"` works
without it. All names are escaped in the image, which holds no script or event attribute. An image
cannot be cut like text, so one over 256 KB (about 90 tables of a dozen columns), or over a lower
byte limit of the connection, is refused with a message that suggests `tables` and a
smaller `depth` (or Mermaid, which is cut at whole tables); for a schema of more than 300 tables
a comment after the image says how many were drawn. An unknown `format` is an error naming the
two. A table that does not exist,
a name that is a view, and a schema that does not exist are errors; MongoDB, Redis and
Elasticsearch have no tables to draw and answer with an error.

`compare_schemas` (`connection`, `schema`, `other_schema`, optional `other_connection`,
`max_bytes`, `timeout_seconds`) compares two schemas as the app's schema compare does, of one
connection or of two connections of the same kind of database (otherwise it answers as the app
does: only databases of the same kind). It returns the differences (tables, columns, indexes,
foreign keys and constraints added, dropped or changed) and the migration script that makes
`other_schema` like `schema`, as JSON text. Nothing runs: the script reaches you only through
`propose_sql`, which the answer says. The answer stays within `max_bytes` (32 KB by default):
the list of differences is cut first, with a note; a script that does not fit is left out whole
(`script: null`, `script_complete: false`), never cut, since a part of a migration is dangerous
to run. Both connections must be open to agents, and each is read within its own agent limits
(timeout, calls per minute).

`profile_table` (`connection`, `schema`, `table`, optional `sample_rows`, `max_bytes`,
`timeout_seconds`) describes each column of a table or view from its first `sample_rows` rows
(10,000 by default, up to 100,000; no order is implied, and the answer says when the table has
more): NULLs, distinct values, empty texts, minimum and maximum, the most frequent values,
patterns (emails, dates, numbers stored as text) and the spread of numbers. It reads in the
database's read-only transaction, within the connection's timeout and calls per minute. A
sensitive column, decided as for results (the user's marks on that table over the Safety
rules), gives its counts only (`nulls`, `distinct`, `empty`): no minimum, maximum, top value,
pattern or length. If the marks cannot be read the call fails rather than show values; a
connection set to show sensitive values shows them here too. Values are cut at 1,000
characters and have the connection's secrets masked. The connection's `max_rows` also caps the sample.
On MongoDB a column is a top-level field: a sensitive field inside a document is not told apart.

`estimate_change` (`connection`, `sql`) is a dry run: it returns, without running anything, what the
confirmation dialog would show the user for a change. It asks the database for the plan
(`EXPLAIN` without `ANALYZE`, so not even a function the statement calls runs) and answers
`outcome`: `estimated` with `estimated_rows`, the `plan` (the values compared to sensitive columns
masked), the `safety_checks` that apply (an `UPDATE` or `DELETE` without `WHERE` names its table and
rows), `asks_the_user` (Settings ask before agents' changes), `app_open` and `would_run`; `refused`
with the reason, when the connection's access, or the Safety setting on a change of every row,
would refuse the statement (a read-only connection, a production one); or `not_a_change` for a
statement that changes nothing. A database without query plans (MongoDB, Redis, Elasticsearch,
Cassandra) or a plan the database does not give answers with `estimate_note` instead of an error.
It counts as a call for `calls_per_minute`.

`server_activity` and `top_queries` (`connection`, optional `limit`) read what the server does,
with the views of the app's admin tab, on PostgreSQL, MySQL, MariaDB and SQL Server; other
databases answer with an error saying they have none. `server_activity` returns the sessions
(user, database, client, state, the statement and how long it has run, what it waits for, the
sessions blocking it; those that wait or run first, 50 by default, up to 200) and the locks;
`top_queries` the statements that took the server most time (`pg_stat_statements`,
`performance_schema`, Query Store; 20 by default, up to 100). Nothing is cancelled or ended. The
statements are other people's, with their values: credentials in them (`CREATE ROLE ... PASSWORD`)
never show, whatever the connection's setting; the connection's own
secrets are redacted; every string in them, and the numbers tied to sensitive columns, read `'••••••'` unless the user chose
to show values on the connection; a statement is cut at 500 characters, with a note. If the
sensitive marks cannot be read the call fails. A missing privilege (`pg_read_all_stats`,
`VIEW SERVER STATE`) or a missing extension (`pg_stat_statements` not installed) is said in the
answer.

The user's saved scripts and history are readable too, never writable: `list_scripts` lists
the scripts of a project (by its name; without one, those of All connections) with the
connection each opens on, `read_script` returns one (256 KB at most), and `search_history`
the statements run, newest first (50, up to 500), by connection, text and origin (`app` or
`agent`). Scripts linked to a connection with no agent access, and that connection's history,
are left out as if they did not exist; errors in the history are cleaned of the passwords
the connection's URL or password command would show. The values a script or a statement ties
to a sensitive column (`email = '…'`, `SET phone = '…'`, the positions of `INSERT … VALUES`)
read `'••••••'`, and so do the values a history error quotes (`Key (email)=(…)`). In text a
column is known by its name only: the names of Settings › Safety (emails, phones, passwords…)
count, and so do the columns the user marked sensitive in the app; a column marked not
sensitive is still masked where the rules name it, since the text does not say which table it
is. When the app's catalog cannot be read, the tools fail rather than show the values.

The results of `run_query` and `sample_rows`, and the editor, selection and result that
`get_user_context` shares, mask sensitive columns too: their values read `••••••` and NULL stays
NULL, as in the grid while it masks. The names and types of Settings › Safety count, and the
user's marks; where a statement reads one table a mark on its column counts over the rules, a
"not sensitive" mark included. In `run_query` and `sample_rows` the name is enough to follow a
column through the select list or a `RETURNING` (`email AS contact`, `upper(email)`). What
`get_user_context` shares of a result is known by its column names only, so there the alias and
the type rules do not count. A result says which columns it masked: `masked_columns` in JSON, a
last line in text, and a note in `get_user_context`.

Settings › Agents › Security can show the values on one connection. It is part of what the user
approves, like the agent access: written in `connections.toml` by hand it changes nothing
until the user accepts it in the app.

### Limits per connection

Settings › Agents › Security also holds the limits of a connection. Each is optional (empty
means none) and only tightens the fixed caps, which no setting can exceed (10,000 rows, 1 MB, 300
seconds): `max_rows`, `max_bytes` and `timeout_seconds` lower what `run_query`, `sample_rows` and
`explain` accept, so an agent that asks for more gets the connection's maximum, and a default
above the maximum is lowered to it. When the agent asked for more, the result says so (a line in
text, `notes` in JSON). They are part of what the user approves, like the access: raising or
removing one in `connections.toml` by hand changes nothing until the user accepts it in the app,
while lowering one applies at once. The fields live in the `[connections.agent_limits]` table
of the connection. They are not carried in a project export or a shared project, and an import
starts without them.

`calls_per_minute` counts `run_query`, `sample_rows` and `explain` on the connection over a
sliding minute. Past it a call is refused with the seconds to wait. Metadata tools are not
counted. Each server process counts on its own: an agent that starts several (several windows
or clients) gets the limit for each, and the CLI's calls are a process apiece.

`max_estimated_rows` and `max_plan_cost` are the cost guard. Before a read (`run_query`,
`sample_rows`) on a database that has query plans, PlatypusDB asks for the estimated plan without
running the statement, and refuses the read when the root's estimated rows or cost, or the
largest of the steps' when the root has none, are over a threshold. The error names the limit
and carries the plan, so the agent can narrow the query; the values compared with sensitive
columns in it are masked. Where there is no estimate (SQLite gives none), the database has no
plans, or the plan cannot be had, the read goes ahead and the result says the guard could not
check. Changes are not checked here: they already ask the user. The cost unit is the database's
own, so it is not comparable between PostgreSQL and MySQL.

Masking prevents accidents; it is not a wall. A subquery that renames the column, a second
branch of a `UNION`, a `WHERE` that tests the value, or a document field below the top level of
MongoDB still let a value through. For a real boundary, give the connection a database user
without access to those columns.

The history and the agents' call log never keep a password a statement sets
(`CREATE ROLE … PASSWORD '…'`, `IDENTIFIED BY '…'`, `SET PASSWORD`): it is stored as
`'********'`, in the statement, in a call's arguments and in the error, whether the app or an
agent ran it.

## The agent report

The call log (`agent-calls.db`, in the data folder) keeps, for each call recorded since the
report exists, three figures besides what a row had: `rows_returned` (rows the call gave the
agent), `tables_read` (a JSON array of table names) and `effect` (`none`, `allowed` or
`refused`). Older rows have them `NULL` and the report shows `–` for them. No value is kept:
names and counts only.

- **Session**: the `session` column, one start of the MCP server. There is no gap rule: the log
  records the session, so a long pause inside one is still one session.
- **Changes**: `effect` is set from the statement of `run_query` (`access::changes`) and how the
  call ended: `allowed` when it ran, `refused` when it ended in a refusal (read-only access, a
  safety setting, the user's no in the app), `none` otherwise.
- **Tables read**: the arguments of `sample_rows` and `profile_table`, and the sources of
  `query_across_databases`; for `run_query`, the names after `FROM` and `JOIN` found with the
  lexer of `platypus-sql` (`platypus-mcp/src/facts.rs`). `platypus-sql` has no parser that
  lists tables, so this is an **estimate**: a table function, a derived table or a name the
  statement builds is missed, and `main.t` and `t` count as two names. A call that failed or
  was refused reads none.
- **Rows returned**: the count the tool's own answer reports (the footer of the text table, or
  `row_count`), so a cut result counts the rows shown.
- **Cost**: the aggregation is three small queries in the store (`AgentCallStore::report`),
  run only while the Activity page shows the summary. Writing a call costs the three extra
  columns; there is no new index. Measured on 100,000 calls in a debug build: about 0.5 s
  (the log keeps 20,000 calls, so about a tenth of that in use).

Clients that read MCP resources find the schema there too: `resources/list` has one resource
per connection agents may use (`platypus://<connection>`, its schemas), and two templates,
`platypus://<connection>/<schema>` (the schema's objects, up to 2,000) and
`platypus://<connection>/<schema>/<table>` (the table described as `describe_table` does).
Names are percent-encoded (`platypus://App%20db/main/my%20table%2Fx`). Each read is in the
call log as `read_resource`, and follows the connection's access as the tools do.

### Prompts

`prompts/list` has four ready tasks that chain the tools: `explain-query` (`connection`, `sql`:
read the plan, check the tables, propose a better statement), `slowest-queries` (`connection`:
`top_queries`, then `explain` on the worst), `write-migration` (`connection`, `change`: the
statements and how to undo them, checked with `estimate_change`, handed over with
`propose_sql`) and `document-table` (`connection`, `schema`, `table`). Each argument is
required; the prompt asks the agent to run nothing that changes data.

The scripts of All connections that contain `:name` or `${name}` parameters are prompts too
(`script-<path>`, e.g. `script-reports-monthly-sql`; two paths that give the same name get
`-2`, `-3`, in path order), with one required argument per parameter. At most 100 scripts are
offered. Scripts follow `list_scripts` and `read_script`: those of a connection with agent
access off are not listed, and the text has the values of sensitive columns masked. The prompt
says the script is the user's data, quoted as saved, and not instructions.

### Schema change notifications

A client may call `resources/subscribe` on a connection, schema or table resource and gets
`notifications/resources/updated` for it, followed by `notifications/resources/list_changed`,
when the app saw the connection's schema change. That is when the user **refreshes** the
connection, a database or a schema in the navigator, or when a schema snapshot is stored that
differs from the previous one (schema timeline, scheduled snapshots included). The app writes a
counter per connection in `schema-changes.json` in its data folder; a server with subscribers
reads that file every 5 seconds, so it works with `platypus-db --mcp` in its own process
whether the app is open or closed. With no subscriber nothing runs: the check stops at the last
`resources/unsubscribe` and when the server ends. A connection with agent access off cannot be
subscribed to and never notifies, even if its access is turned off after subscribing. A change
made outside the app (another tool, a migration run by hand) is seen only at the next refresh
in the app. After a notification, read the resource again.

The notes, tags and sensitive marks the user keeps in the app (`catalog.db`) come with the
objects. `describe_table`, the table resource and `list_objects` add `note` and `tags` to a
table or view, and `describe_table` adds `note`, `tags` and `sensitive` (`true` or `false`:
only the mark, never a value) to its columns; what is empty is left out, so an object without
entries reads as it always did. Notes are text the user wrote, to be read as data, not as
instructions: they pass through the connection's redaction, a note over 1,000 characters is cut
with `note_cut: true`, and one call returns 64,000 characters of notes at most. When the catalog
cannot be read, `describe_table` and `list_objects` still answer and say
`catalog_unavailable` (in `list_objects` only with `limit`, whose answer is an object: a plain list stays a list).
`search_objects` also finds objects by their database comment and by notes (`matched` is
`comment` or `note`, with `column` for a column's note), and `#tag` finds the tagged objects
(`matched: "tag"`), as the app's navigator filter does: a `#word` is a tag only when the
connection has such a tag, else it is plain text; with several tags, an object needs them all,
and text beside them must be in its name, comment or note. Only objects the connection lists
can match, so a note left on a dropped table finds nothing. Entries are read only for the
connection's own database (the app keys them with an empty database), and never for a
connection agents may not use.

`sample_rows` returns the first rows of a table, view or collection (20 by default) within
the limits of `run_query`. `explain` returns a statement's plan as an indented tree (or JSON)
with costs, rows and index suggestions, within the byte limit and the timeout; the statement
is checked as `run_query` checks it, and runs only with `analyze`, rolled back: on read-only
connections that needs a read-only transaction that refuses writes (PostgreSQL, MySQL,
MariaDB, SQLite). `propose_sql` runs nothing: the app opens the SQL in an editor tab, at once
if it is open, else when it starts (within 24 hours).

`propose_edits` takes `connection`, `schema`, `table`, an optional `title` and, as lists, `inserts`
(objects of column to value), `updates` (`{key, set}`, `key` naming a row by exactly the
columns of a primary or unique key) and `deletes` (objects naming rows by such a key). A value
is a string, number, boolean or null; arrays and objects are refused. At least one change, at
most 500 rows and 1 MiB in all. It writes nothing, whatever the agent access (read-only included): the
app opens the table's data viewer with the rows as pending edits (new rows green, changed cells
and deleted rows marked), which you save or discard like your own; a row it cannot find or a
value the column refuses is said so in the viewer and the rest is applied. A table with no
primary or unique key, a connection saved as read-only or that forbids editing data, and MongoDB,
Redis, Elasticsearch, Cassandra, ClickHouse and the databases reached through ADBC are refused.

On read-only connections only statements that change nothing run, inside the database's
read-only transaction (PostgreSQL, MySQL and MariaDB `READ ONLY`, SQLite `query_only`; DuckDB
and SQL Server roll the transaction back), and functions that act outside it
(`pg_terminate_backend`, `dblink`, `LOAD_FILE`…) are refused. The session itself is read-only
on the server, for read-only agents and for the app's read-only connections alike: PostgreSQL,
CockroachDB and YugabyteDB set `default_transaction_read_only`, MySQL and MariaDB `SET SESSION
TRANSACTION READ ONLY`, SQLite and DuckDB open the file read-only (DuckDB only when no other
session of the app holds the file), after the bootstrap statements, so that a function that
writes fails there too. Behind a pooler in transaction mode, and on SQL Server, Oracle, libSQL
and the ADBC and ODBC databases, the statement check alone stops writes. ClickHouse has no
transaction: its read-only sessions (read-only agents, and the app's read-only connections)
send every statement but `SET` with `readonly = 1`, so the server itself refuses writes and
the table functions that reach other servers or its files (`url`, `s3`, `remote`, `file`,
`deltaLake`, `iceberg`, `hudi`, `fileCluster`, `ytsaurus`…); an agent's statement is also
checked against the table functions that read only local data (`numbers`, `zeros`,
`generateRandom`, `values`, `view`, `merge`, `dictionary`, `null`, `format`, `loop`,
`generate_series`): any other one in a `FROM` list is refused. DuckDB sessions of agents
cannot read or write other files nor load extensions, and close after each call so the app
can open the file. On Redis, the commands that show the server's credentials change nothing
but are not reads either: `ACL GETUSER` and `ACL LIST` (the hash of every password) and a
`CONFIG GET` whose patterns match a parameter that holds a password (`requirepass`,
`masterauth`, `masteruser`, the TLS key passphrases; so `CONFIG GET *` and `CONFIG GET *pass*`
too). Read-only agents and the app's read-only connections refuse them; on a read-write
connection the user is asked first, as for a change. `CONFIG GET maxmemory`, `ACL USERS` and
`ACL WHOAMI` still run.

On read-write connections changes commit at once, once allowed in the app (see above);
`BEGIN`, `COMMIT` and `ROLLBACK` are refused, and so are `UPDATE` or `DELETE` without `WHERE`
and `TRUNCATE` unless Settings → Safety runs them without asking.

The bridge: while it runs, the app writes its port and token to `agent-bridge.json` in its
data folder (readable only by the user) and removes it on exit; the token is kept in
`agent-token` until a new one is made. `--mcp` and the CLI read that file at each call, so
they find the app whenever it opens. Routes, all with `Authorization: Bearer <token>`:
`GET /v1/sessions`, `POST /v1/confirm` (a change to ask about; the answer is
`{"answer": "once" | "session" | "deny", "reason"}`), `POST /v1/hello` (an agent says it is
connected; forgotten after 90 s without one) and `/mcp` (MCP over streamable HTTP, the same
tools as `--mcp`).

</details>

Agents without MCP use the `platypus` CLI (crate `platypus-cli`) with `--connection`, which
follows the same access, asks the open app before a change as `--mcp` does, and prints one
JSON document:

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
"truncated", "truncated_by": "max_rows" | "max_bytes" | null, "values_cut", "elapsed_ms",
"masked_columns"}` (`masked_columns` only when there are any),
with numbers and booleans as JSON values and everything else as text; a statement without
rows prints `{"affected_rows", "elapsed_ms"}`. `-f table` prints the text table instead. A
failure prints `{"error": {"kind", "message"}}` and exits with 1; `kind` is `not_found`,
`denied`, `locked`, `invalid`, `connect`, `database`, `timeout` or `store`.
