[← README](../README.md)

# Agents (MCP) and the CLI

Claude Code, Codex, Cursor and other MCP clients use your saved connections through
PlatypusDB's MCP server: the app itself, started with `--mcp`, which they start themselves
(no window opens, and the app need not be open for reading).

- Each connection gives agents **no access**, **read only** (the default) or **read and
  write**; production and read-only connections are never more than read only.
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
  Settings → Safety → "Ask before agents change data" turns the questions off.
- While the app is open, agents reach it through a **bridge** on a local port with a token:
  the top bar shows the agents connected, and the `list_open_sessions` tool shows them the
  sessions open in the app (connection, database, manual commit, open transaction).
  Settings → Agents → Setup turns the bridge off, changes its port (47337 by default), shows
  its address and token for MCP clients over HTTP and makes a new token.

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

<details>
<summary>How the MCP server works</summary>

`platypus-db --mcp` talks MCP over stdin and stdout. It reads the connections of the app's
profile, or of `PLATYPUS_DATA_DIR` when set. With a master password the saved passwords stay
locked to agents: such connections answer with an error saying so, and work with a password
command or `.pgpass` instead. A connection that asks for its password every time is not
available to agents.

Tools: `list_connections`, `list_schemas`, `list_objects`, `describe_table`, `get_ddl`,
`search_objects`, `run_query`, `sample_rows`, `explain`, `propose_sql` and
`list_open_sessions`. `run_query` runs one statement and returns 100 rows,
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
"truncated", "truncated_by": "max_rows" | "max_bytes" | null, "values_cut", "elapsed_ms"}`,
with numbers and booleans as JSON values and everything else as text; a statement without
rows prints `{"affected_rows", "elapsed_ms"}`. `-f table` prints the text table instead. A
failure prints `{"error": {"kind", "message"}}` and exits with 1; `kind` is `not_found`,
`denied`, `locked`, `invalid`, `connect`, `database`, `timeout` or `store`.
