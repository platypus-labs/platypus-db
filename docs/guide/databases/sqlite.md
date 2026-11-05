[← Databases](README.md)

# SQLite

SQLite runs inside the app (`rusqlite` with SQLite built in): no server, no host, no user, no
password, nothing to download.

## Connect

Choose **SQLite** and give the path of the file, or paste a URL:

| You paste | Meaning |
|---|---|
| `sqlite:///home/ana/app.db` | the file `/home/ana/app.db` |
| `sqlite:///C:/data/app.db` | a file on a Windows drive |
| `sqlite:///:memory:` | an in-memory database |
| `file:app.db?mode=ro` in the path field | a SQLite URI, passed to SQLite as written |

The file is created when it does not exist but its folder does, as an empty database. A missing
folder is an error (more likely a typo or a drive not mounted), and so is a relative path that
does not exist: give the full path. A file that is not a SQLite database is refused when you
connect.

A read-only connection (and a read-only agent) opens the file read-only, so SQLite itself refuses
a write. SSL, SSH and proxy settings do not apply to an embedded database.

## Privileges

The operating system's: the app needs to read the file, and to write it and its folder (for the
journal) to change anything. A busy file is waited for before an error.

## What works

- Data editing in the grid, by primary key (or a unique key on non-null columns, or a virtual key).
  An `INTEGER PRIMARY KEY` column counts as a key though SQLite reports it nullable.
- Manual commit and rollback; cancelling a running statement (`sqlite3_interrupt`).
- Explain shows SQLite's query plan only: there is no Explain analyze.
- Parameters tab: the pragmas that matter, with SQLite's documented defaults.
- No sessions, locks, dashboard, users or statements tabs.
- The table editor, ER diagram and comparison. SQLite alters little in place, so changing a table
  creates it again and copies its rows; the comparison and the rebuild keep expression and partial
  indexes, deferred keys and the `AUTOINCREMENT` counter.
- Metadata search, object DDL, and exports, imports and copies like any SQL database.
- Terminal mode runs `sqlite3`, if it is on the PATH.

## Agents

Read-only agents and read-only connections open the file read-only, and run their queries with
`query_only`. `estimate_change` has no row estimate to give on SQLite. See
[Agents and the CLI](../../AGENTS.md).

## Known limits

- Another process may be holding the file: the app waits up to 5 seconds for a busy file, then SQLite
  reports it.
- An agent's statement that calls `load_extension(…)` is asked about first, like a change.

## Tested on

The conformance suite runs on the SQLite built into the app, including transactions, plans,
table editing, the rebuild that keeps `AUTOINCREMENT`, and files opened read-only.

See also: [Connecting](../connect.md), [Troubleshooting](../troubleshooting.md).
