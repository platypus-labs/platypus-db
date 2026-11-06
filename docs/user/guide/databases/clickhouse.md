[← Databases](README.md)

# ClickHouse

The driver talks to ClickHouse's **HTTP interface** and reads results in the Arrow stream
format. It is part of the app; nothing is downloaded. It browses, queries and designs tables, but
does not edit rows in the grid.

## Connect

| You paste | Read as |
|---|---|
| `clickhouse://default@ch.example.com/logs` | HTTP on port 8123, database `logs` |
| `clickhouse://ch.example.com/logs?sslmode=require` | HTTPS; with no port in the URL, 8443 |
| `jdbc:clickhouse://host:8123/db` | a JDBC URL |
| `https://host:8443` in a ClickHouse form | the HTTP(S) URL of the server |

Use the **HTTP** port, not the native one (9000). The user is `default` when empty; the password is
sent in the `X-ClickHouse-Key` header. The Database field runs `USE <database>` after connecting.

**TLS** is chosen by the mode, since HTTP cannot negotiate it: `disable` and `prefer` (the
default) connect in **plain HTTP**; `require` and the `verify` modes use HTTPS. With HTTPS the
default port is 8443 when the URL gives none; if you fill in the form, type the port yourself.
`require` does not check the certificate, `verify-ca` checks the chain, `verify-full` also the
host name; `sslrootcert` names your CA's file, otherwise the system's trusted certificates are
used.

A connection is one HTTP session: `SET`, `USE` and temporary tables last between statements.

## Privileges

Browsing reads `system.databases`, `system.tables`, `system.columns` and
`system.data_skipping_indices`; the login sees what ClickHouse lets it see there. The tabs
read:

- **Parameters**: `system.settings` and `system.server_settings`.
- **Dashboard**: `system.metrics`, `system.events`, `system.parts` (database sizes) and
  `system.replicas`; what the login cannot read shows "—".
- **Users and roles**: `system.users`, `system.roles`, `system.grants` and `system.role_grants`.
  Users defined in the server's configuration files are listed too; the server refuses to change
  them.

## What works

- Queries, results, the navigator, autocompletion, object DDL, metadata search, and the table
  designer (including the engine clause and skipping indices).
- Explain shows the plan of `EXPLAIN indexes = 1`; there is no Explain analyze.
- Cancel stops the statement on the server with `KILL QUERY`.
- Parameters, Dashboard and Users and roles tabs. No sessions, locks, statements or tables and
  indexes tabs.
- Exports, and imports into ClickHouse as `INSERT` statements.

## What does not

- **No data editing in the grid**: it says "ClickHouse tables are not edited in place". Saving
  would check each change's row count inside a transaction, and ClickHouse has neither
  transactions nor row counts for updates. Change rows with SQL.
- **No transactions**: the manual-commit toolbar is hidden; every statement takes effect alone.
- No foreign keys or unique constraints (the primary key only orders the data).

## Agents

A read-only agent, and a read-only connection, send every statement except `SET` with
`readonly = 1`, so the server refuses writes and the table functions that reach other servers or
its files (`url`, `s3`, `remote`, `file` and the like). An agent's statement is also checked
against a list of table functions that read local data only. See
[Agents and the CLI](../../AGENTS.md). Terminal mode is not available: `clickhouse-client` uses
the native port.

## Known limits

- Plain HTTP unless the mode is `require` or stronger: the default `prefer` does not encrypt.
- Only the HTTP interface is used; a server that exposes just the native port cannot be reached.

## Tested on

The conformance suite runs on ClickHouse 26.3. Older ClickHouse versions are not tested, and the
app has no minimum version for it.

See also: [Connecting](../connect.md), [Troubleshooting](../troubleshooting.md).
