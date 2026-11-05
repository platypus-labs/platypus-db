[← Databases](README.md)

# PostgreSQL, CockroachDB, YugabyteDB, TimescaleDB and the rest

One driver, built on `tokio-postgres`, speaks the PostgreSQL protocol. It opens PostgreSQL 12 and
later, and the servers and services that speak the same protocol. Nothing is downloaded: the
driver is part of the app.

## Connect

Pick **PostgreSQL** in the grid of databases, or paste a URL: the form fills itself in.

| You paste | Read as |
|---|---|
| `postgres://user:pass@host:5432/db?sslmode=require` (or `postgresql://`) | the URL form; the default port is 5432 |
| `jdbc:postgresql://host/db?sslmode=require` | a JDBC URL, as DBeaver and DataGrip keep them |
| `host=db port=5432 dbname=shop user=app sslmode=require` | a libpq connection string |
| `Host=db;Database=shop;Username=app;Password=…` | an Npgsql connection string |

URL parameters the app knows: `sslmode`, `sslrootcert`, `sslcert`, `sslkey`, `application_name`,
`connect_timeout` and `pgbouncer=true`. Any other parameter is left out, and the form says which.
A failover list of hosts (`h1:5432,h2:5432`) keeps the first host.

**TLS** follows libpq's `sslmode`:

| Mode | What happens |
|---|---|
| `disable` | no TLS |
| `prefer` (the default) | TLS if the server offers it, otherwise plain; the certificate is not checked |
| `require` | TLS, certificate not checked |
| `verify-ca` | TLS, the certificate chain is checked, the host name is not |
| `verify-full` | TLS, chain and host name are checked |

The chain is checked against the file in `sslrootcert` if there is one, otherwise against the
system's trusted certificates. A client certificate (`sslcert` and `sslkey`) is sent in every
mode that uses TLS.

**Signing in** is by password, or by a token that a cloud's command-line tool makes at each
connection: AWS IAM (`aws`), Google Cloud IAM (`gcloud`) and Microsoft Entra ID (`az`) are in the
connection's *Authentication* choice. The tool must be installed and signed in on your machine.

**Poolers.** Behind PgBouncer, Supavisor or Neon's pooler in transaction mode, choose *Transaction
mode* in the form (or add `pgbouncer=true` to the URL). Statements then run unnamed, because the
next transaction may be on another server session. The form suggests it for hosts ending in
`.pooler.supabase.com` and for Neon hosts with `-pooler` in the name. In that mode the
PL/pgSQL debugger is off, and agents' read-only sessions rely on the statement check alone.

The other servers have buttons of their own, which only set defaults:

| Button | Defaults it sets |
|---|---|
| CockroachDB | port 26257 |
| YugabyteDB | port 5433, user and database `yugabyte` |
| TimescaleDB | none |
| Supabase | user and database `postgres`, TLS `require` |
| Neon, AlloyDB | TLS `require` |

## Privileges

Browsing needs only what the server shows the login. The administration tabs read these:

- **Sessions and locks**: `pg_stat_activity` and `pg_locks`. Other users' sessions and statements
  show for a superuser or a member of `pg_read_all_stats`. *Cancel statement* and *End session*
  call `pg_cancel_backend` and `pg_terminate_backend`.
- **Costliest statements**: the `pg_stat_statements` extension. Without it the tab says how to
  turn it on (add it to `shared_preload_libraries`, restart, `CREATE EXTENSION`).
- **Tables and indexes**: `pg_stat_user_tables` and `pg_stat_user_indexes`.
- **Parameters** and **Dashboard**: `pg_settings`, `pg_stat_database`, `pg_stat_activity` and the
  replication views; the standbys are listed only to a member of `pg_monitor`. A figure the login
  cannot read shows "—" and why.
- **Users and roles**: `pg_roles` and `pg_auth_members`; changing them needs the rights the
  statements need.
- **Maintenance**: `VACUUM`, `ANALYZE` and `REINDEX` on the table.
- **Back up… and Restore…** run `pg_dump`, `pg_restore` and `psql`, which must be on the PATH.
- **Debug…** needs the `pldebugger` extension on the server.

## What works

- Data editing in the grid, by primary key (or a unique key on non-null columns, or a virtual key
  you set). Arrays and composite values edit one element or field per line.
- Transactions with manual commit, and statements cancelled on the server.
- Explain and Explain analyze; the plan is read from PostgreSQL's JSON or text.
- Several databases per connection, partitions, PostGIS geometries, arrays and composite types.
- Imports and copies into PostgreSQL use `COPY`.
- Every administration tab above, the PL/pgSQL debugger, and schema tools (table editor, ER
  diagram, comparison).

**CockroachDB and YugabyteDB** open with the same driver, but the app finds out which server it
is, and the server tools that rely on PostgreSQL's own views are off: no sessions, locks,
dashboard, statements, tables and indexes, parameters, users or maintenance tabs, and no
debugger. CockroachDB has its own plan format (Explain and Explain analyze work) and imports go
as `INSERT` statements instead of `COPY`. A CockroachDB table without a key is edited by its
hidden `rowid`. TimescaleDB, Supabase, Neon and AlloyDB are PostgreSQL to the app, with
everything above.

## Agents

Read-only agents and read-only connections open the session with `default_transaction_read_only`
(PostgreSQL, CockroachDB and YugabyteDB), so the server itself refuses writes; behind a
transaction-mode pooler only the statement check does. `server_activity` and `top_queries` work on
PostgreSQL proper, not on CockroachDB or YugabyteDB. See [Agents and the CLI](../../AGENTS.md).

## Known limits

- Older than 12: the app connects and warns that some features may not work.
- The debugger is off behind a transaction-mode pooler.
- Terminal mode needs `psql` on the PATH.

## Tested on

The conformance suite runs on PostgreSQL 12, 14, 16 and 17, TimescaleDB 2.30 on PostgreSQL 17,
CockroachDB 26.2 and YugabyteDB 2025.2, plus PgBouncer in transaction mode, a client certificate
over mutual TLS, and the debugger on a server with `pldebugger`. Supabase, Neon and AlloyDB are
not tested: they are the PostgreSQL driver with defaults.

See also: [Connecting](../connect.md), [Troubleshooting](../troubleshooting.md).
