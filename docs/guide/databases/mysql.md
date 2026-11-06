[← Databases](README.md)

# MySQL and MariaDB

One driver (the `mysql_async` client) opens MySQL 5.7 and later and MariaDB 10.6 and later. Both
have a tile in the grid of databases; the driver tells which server it reached and shows it in the
connection (`Connected: MariaDB 11.4`). Nothing is downloaded.

## Connect

| You paste | Read as |
|---|---|
| `mysql://user:pass@host:3306/db?sslMode=REQUIRED` (or `mariadb://`) | the URL form; default port 3306 |
| `jdbc:mysql://host:3307/db`, `jdbc:mariadb://…` | a JDBC URL |
| `Server=db;Port=3306;Database=shop;Uid=app;Pwd=…` | a Connector/NET connection string, if it also has `Driver=` or you pasted it in a MySQL form |

URL parameters the app knows: `sslmode` (also written `ssl-mode`, `sslMode`, `ssl_mode`),
`sslrootcert`, `sslcert`, `sslkey`, `application_name` and `connect_timeout`.

**TLS**: `disable` turns it off. `prefer` (the default) uses TLS and falls back to plain when the
server has none or only offers versions the client refuses (MySQL 5.7's images do); it never falls
back when signing in with a token. `prefer` and `require` do not check the certificate.
`verify-ca` checks the chain against `sslrootcert` and not the host name; `verify-full` checks
both. MySQL's own names (`REQUIRED`, `VERIFY_IDENTITY`) are understood. A client certificate needs
both `sslcert` and `sslkey`.

**Signing in** is by password, or by an AWS IAM, Google Cloud IAM or Microsoft Entra ID token made
by the `aws`, `gcloud` or `az` tool; the token goes over TLS as the server asks. Kerberos
(MariaDB's `auth_gssapi`, MySQL's Kerberos plugin) is not supported yet.

The connection is always TCP: a Unix socket or a Windows named pipe is not used.

## Privileges

Browsing needs what the login can see. The administration tabs read:

- **Sessions**: the process list, from `performance_schema.threads` on MySQL 8 and from
  `information_schema.PROCESSLIST` on MariaDB, MySQL 5.7 or a server with `performance_schema`
  off. How much of other users' sessions the server shows is the server's rule, which the app
  does not check.
- **Locks**: `performance_schema.data_lock_waits`, `data_locks` and `metadata_locks` on MySQL 8 (seeing other sessions' waits needs
  `PROCESS`; without it the list has no waits and says so), `INNODB_LOCK_WAITS` in `information_schema` on MariaDB and older MySQL.
- **Costliest statements**: `performance_schema.events_statements_summary_by_digest`. If
  `performance_schema` is off, the tab says to start the server with it.
- **Tables and indexes**: `information_schema`, plus `performance_schema` for how much each index
  is read; without it the index use is unknown.
- **Cancel statement** and **End session** run `KILL QUERY` and `KILL CONNECTION`.
- **Parameters** and **Dashboard**: server variables and status counters.
- **Users and roles**: the server's accounts and grants.
- **Maintenance**: `ANALYZE`, `OPTIMIZE` and `CHECK TABLE`.
- **Back up… and Restore…** run `mysqldump` and `mysql`, which must be on the PATH.

## What works

- Data editing in the grid, by primary key (or a unique key on non-null columns, or a virtual key).
- Manual commit and rollback, and cancelling a statement on the server.
- Explain: MySQL 8.0.16 and later give `EXPLAIN FORMAT=TREE`; older MySQL and MariaDB give
  `EXPLAIN FORMAT=JSON`. **Explain analyze** is `EXPLAIN ANALYZE` on MySQL 8.0.18 and later and
  `ANALYZE FORMAT=JSON` on MariaDB; MySQL 5.7 and 8.0.0 to 8.0.17 refuse it.
- Every administration tab above, and schema tools (table editor, ER diagram, comparison).
- Imports use `LOAD DATA LOCAL` when the server allows it (`local_infile` on), otherwise
  `INSERT` statements.
- Errors name the server they come from, e.g. `Unknown column 'x' in 'field list' (MySQL 5.7.44)`.

## Agents

Read-only agents and read-only connections run `SET SESSION TRANSACTION READ ONLY`, and their
queries run in a read-only transaction. `server_activity` and `top_queries` work. A MySQL or
MariaDB `@variable`, temporary table or `SET` made by one agent call is there in the next. See
[Agents and the CLI](../../AGENTS.md).

## Known limits

- Older than MySQL 5.7 or MariaDB 10.6: it connects, with a warning that some features may not
  work.
- The table editor saves with MySQL's commit-per-statement DDL: a failed script can leave part of
  it applied.
- Terminal mode needs `mysql` on the PATH.

## Tested on

The conformance suite runs on MySQL 5.7, 8.0 and 8.4 and MariaDB 10.6 and 11.4, with a client
certificate over mutual TLS on MySQL 8.4, and sessions the server dropped.

See also: [Connecting](../connect.md), [Troubleshooting](../troubleshooting.md).
