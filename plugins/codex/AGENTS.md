# PlatypusDB

The `platypus` MCP server is the PlatypusDB app started with `--mcp`. It reaches the database
connections the user saved in the app, with the access the user gave each one. Passwords,
hosts and users never reach you.

## Find your way first

1. `list_connections`: the connections you may use, with driver, type (dev, test, prod) and
   `agent_access` (`read_only` or `read_write`). Use the names it shows.
2. `list_schemas`, then `list_objects` for a schema (500 objects a page; follow
   `next_offset`), or `search_objects` to find a table or column by name.
3. `describe_table` before writing a query on a table: columns, types, keys, indexes and the
   foreign keys to join on. `get_ddl` gives the CREATE statement of any object, and `get_er_diagram` a Mermaid ER
   diagram of a schema (or of some tables and those linked to them by foreign keys).
4. `sample_rows` shows the first rows of a table, to see what the values look like.

Clients that read MCP resources can also read `platypus://<connection>`,
`platypus://<connection>/<schema>` and `platypus://<connection>/<schema>/<table>`.

## Query

- `run_query` runs **one** statement in the connection's own SQL dialect. It returns 100 rows,
  32 KB and waits 30 s by default; raise `max_rows`, `max_bytes` or `timeout_seconds` only
  when the user needs more, and prefer `COUNT(*)`, `GROUP BY` and `LIMIT` to reading many rows.
- The answer says when rows or long values were left out: tell the user, do not present a
  partial result as complete.
- `explain` shows a statement's plan without running it; `analyze=true` runs it and rolls
  back what it changes. Use it before suggesting an index or rewriting a slow query.

## Changes

- On `read_only` connections only statements that change nothing run, inside a read-only
  transaction. Production connections are always read-only.
- On `read_write` connections a change may wait for the user to allow it in the open app,
  and is refused when the app is closed. `UPDATE` or `DELETE` without `WHERE` is refused
  unless the user's safety settings allow it.
- For a change the access does not allow, or one the user should check first, use
  `propose_sql`: it opens the SQL in a new editor tab of the app, for the user to review and
  run by hand. Never try to get around the access.

## The user's scripts and history

To learn how the user queries the data, read what they already wrote. All three tools are
read-only and follow the connections' agent access: what belongs to a connection you may not
use is left out.

- `list_scripts` lists the saved scripts (`project` names a project; left out, All
  connections) with the connection each one opens on; `read_script` returns one script's text
  (cut at 256 KiB, and it says so).
- `search_history` returns the statements run before, newest first (50 by default, up to 500),
  filtered by `connection`, `text` and `origin` (`app` for the user, `agent` for agents).

## The open app

While the app is open, `list_open_sessions` shows what the user has open (connection,
database, manual commit, open transaction). Your statements run on sessions of your own,
never on these.

When the user points at something on their screen ("this query", "what I selected", "the
result I see"), call `get_user_context`: PlatypusDB asks them to share the active tab's
editor text, selection and result grid (`parts` limits what you ask for). It returns only
what they allow, each time or for your whole session; a refusal comes with their reason.
The editor and selection are cut at 64 KiB, the result at 100 rows and 32 KiB (`truncated`
says so), and a tab on a connection that is not open to agents is not shared.
