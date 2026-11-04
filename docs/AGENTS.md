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
- An agent can propose SQL with `propose_sql`: it opens in a new editor tab, for you to
  review and run.
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
`get_er_diagram`, `search_objects`, `run_query`, `sample_rows`, `explain`, `propose_sql`,
`list_open_sessions`, `list_scripts`, `read_script` and `search_history`. `run_query` runs one statement and returns 100 rows,
32 KB and 30 s at most by default (`max_rows` up to 10,000, `max_bytes` up to 1 MB,
`timeout_seconds` up to 300), as an aligned text table or JSON, saying when rows or values
longer than 1,000 characters were left out. In the text table SQL `NULL` reads `NULL` and a
text `NULL` reads `'NULL'`. `list_objects` returns 500 objects at most (`limit` up to 2,000);
a larger schema comes in pages (`truncated`, `total`, `next_offset`, then `offset`). The
client's `notifications/cancelled` cancels the running statement.

`get_er_diagram` (`connection`, `schema`, optional `tables` and `depth`) returns a Mermaid
`erDiagram` as text, built from the catalog, so it works with the app closed: the tables of the
schema with their columns and primary keys, and the foreign keys between them. Without `tables`
it draws the whole schema (the first 300 tables of a larger one); with `tables` it draws those
and the tables linked to them by foreign keys, `depth` hops away in either direction (default
1, at most 3; 0 draws only the named tables). A diagram over 32 KB is cut at whole tables, with
a `%%` comment saying how many were drawn and suggesting `tables`. A table that does not exist,
a name that is a view, and a schema that does not exist are errors; MongoDB, Redis and
Elasticsearch have no tables to draw and answer with an error.

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

Masking prevents accidents; it is not a wall. A subquery that renames the column, a second
branch of a `UNION`, a `WHERE` that tests the value, or a document field below the top level of
MongoDB still let a value through. For a real boundary, give the connection a database user
without access to those columns.

The history and the agents' call log never keep a password a statement sets
(`CREATE ROLE … PASSWORD '…'`, `IDENTIFIED BY '…'`, `SET PASSWORD`): it is stored as
`'********'`, in the statement, in a call's arguments and in the error, whether the app or an
agent ran it.

Clients that read MCP resources find the schema there too: `resources/list` has one resource
per connection agents may use (`platypus://<connection>`, its schemas), and two templates,
`platypus://<connection>/<schema>` (the schema's objects, up to 2,000) and
`platypus://<connection>/<schema>/<table>` (the table described as `describe_table` does).
Names are percent-encoded (`platypus://App%20db/main/my%20table%2Fx`). Each read is in the
call log as `read_resource`, and follows the connection's access as the tools do.

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
