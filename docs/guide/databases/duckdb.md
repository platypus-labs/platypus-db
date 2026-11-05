[← Databases](README.md)

# DuckDB

DuckDB runs inside the app, like SQLite: no server, host, user or password. Unlike SQLite, the app
does not carry DuckDB's library; it **downloads it the first time** a DuckDB database or a data
file is opened.

## First use: DuckDB's library

When you choose **DuckDB** for a new connection and the library is not on this machine, a popup
opens over the form: *Download DuckDB?* It shows the package (DuckDB 2.0.0, from DuckDB's release
on GitHub, MIT license), its size and its source, and checks the download against a SHA-256.
**Download** puts it in the app's data folder, once for every DuckDB database and file; **Cancel**
leaves the form usable, and the offer returns at *Test connection* and at the first opening.
*Settings › Drivers* lists the library and removes it.

The app downloads it for Windows (x64, ARM64), Linux (x64, ARM64) and macOS (one library for both
processors). The app uses exactly the version it was built with (2.0.x) and offers that one if
another is installed. Offline, with the library missing, the connection cannot open.

## Connect

Choose **DuckDB** and give the path of the database file, or paste a URL:

| You paste | Meaning |
|---|---|
| `duckdb:///home/ana/analytics.duckdb` | that file, created when missing (its folder must exist) |
| `duckdb:///C:/data/analytics.duckdb` | a file on a Windows drive |
| `duckdb:///:memory:` | the in-memory database the app's sessions share |
| `:memory:<name>` as the path | an in-memory database of its own, by name |

**Data files as databases.** A path ending in `.csv`, `.tsv`, `.json`, `.ndjson`, `.jsonl`,
`.parquet`, `.xlsx`, `.xlsm`, `.xls` or `.ods` opens as an in-memory database on that file (you can
also drop a file on the window). CSV, TSV, JSON, NDJSON and Parquet become a view named after the
file, which DuckDB reads again at each query; each sheet of a workbook becomes a table, loaded
once. A CSV or TSV is checked first (every line must have as many values as the header), its text
encoding is detected (or you give it), and the import options let you skip bad lines.

Extensions are neither downloaded nor loaded on their own; only the ones built into the library
(Parquet, JSON) are there.

## Privileges

The operating system's: read access to the file, write access to it and its folder to change
anything. One process at a time may open a DuckDB file for writing, so while the app has a file
open another program (the `duckdb` command line, for one) cannot use it; the app's sessions on one
file share one instance. Terminal mode is therefore not offered.

## What works

- Data editing in the grid, by primary key (or a unique key on non-null columns, or a virtual key).
  A data file is read into memory: a CSV, JSON or Parquet view is not a table and cannot be
  edited, and the edits to a workbook's sheets are not written back to the file.
- Manual commit and rollback; cancelling a statement (`duckdb_interrupt`).
- Explain shows the plan (`EXPLAIN (FORMAT JSON)`); **Explain analyze** runs the statement in a
  transaction it rolls back, and is refused while you have a transaction open.
- Parameters tab: DuckDB's settings. No sessions, locks, dashboard, users or statements tabs.
- Metadata search, object DDL, schema tools. DuckDB alters little in place, so changing a table
  creates it again and copies its rows.
- Imports go through DuckDB's appender, in the session's transaction.

## Agents

A read-only agent, and a read-only connection, open the database file read-only, when no other
session of the app already holds it open; an in-memory database or a data file is never
read-only, and the statement check stops writes there. Agents' DuckDB sessions cannot read or write
other files nor load extensions, and close the file after each call so that the app can open it.
`query_across_databases` runs in a DuckDB of its own that the app downloads like this one. See
[Agents and the CLI](../../AGENTS.md).

## Known limits

- Excel files are read with the app's own reader, not DuckDB's Excel extension.
- A DuckDB version other than 2.0.x is not loaded.

## Tested on

The conformance suite runs on a DuckDB file, with the library that matches the app's bindings:
transactions, plans, table editing, special values, 128-bit and unsigned numbers and zoned times.

See also: [Connecting](../connect.md), [Troubleshooting](../troubleshooting.md).
