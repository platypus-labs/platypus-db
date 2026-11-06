[← Databases](README.md)

# IBM Db2 and ODBC (Windows)

Db2, and any database with an ODBC driver or a data source (DSN), are reached through the system's
ODBC driver manager. This works **on Windows only** for now: on macOS and Linux a Db2 or ODBC
connection says that ODBC drivers are reached on Windows only.

## First use: IBM's ODBC driver (Db2)

When you choose **IBM Db2** for a new connection and no IBM ODBC driver is registered on this
machine, a popup opens over the form: *IBM Db2 needs IBM Data Server Driver for ODBC and CLI*. It
shows the package (version 12.1.4), its size, IBM's terms and where it comes from, and checks the
download against a SHA-256. **Download and install** fetches it into the app's data folder and
registers it with `db2cli install -setup`; **Windows asks for administrator permission (User
Account Control)** to do that. **Cancel** leaves the form usable, and the offer returns at *Test
connection* and at the first opening.

A driver already in the system counts: the app looks for one named `IBM DB2 ODBC DRIVER…` or
`IBM DATA SERVER…`, whether its own registration or a full IBM client made it, and takes the
newest by name. The connection's `driver` option replaces that choice. *Settings › Drivers* lists
and removes the downloaded driver, and shows the ODBC drivers and data sources of the system.

## Connect

**IBM Db2**: host, port (50000 by default), **database** (both host and database are required),
user and password. The app builds the connection string itself (`DATABASE`, `HOSTNAME`, `PORT`,
`PROTOCOL=TCPIP`, `UID`, `PWD`). TLS: the modes `disable` and `prefer` connect without it;
`require` and above add `Security=SSL`, and `sslrootcert` becomes `SSLServerCertificate`. Db2
servers listen for TLS on another port than the plain one, so set the port. Through an SSH tunnel
the tunnel's local end is used.

**Other ODBC driver** (the generic choice): give one of

- a **data source (DSN)** the system knows, typed or taken from the suggestions;
- a **driver** name (suggested from the system's ODBC drivers) with a **connection string**
  holding the rest, such as `Server=host;Database=sales`;
- a **connection string** on its own, used as written (it may name its own `DRIVER=`).

The user and password fields are added as `UID` and `PWD` unless the string already has them.
A pasted ADO.NET or ODBC string with `Driver={…}` is read into the matching form when the driver
is SQL Server, PostgreSQL, MySQL/MariaDB or ClickHouse; any other driver or a `DSN=` becomes a
generic ODBC connection with the string as it is.

## Privileges

Whatever the database login has. The navigator uses ODBC's catalog calls (`SQLTables`,
`SQLColumns`, `SQLPrimaryKeys`, `SQLForeignKeys`). There are no administration tabs.

## What works

- Queries and results, with the types the driver reports.
- Manual commit and rollback.
- Cancel: ODBC's cancel call is not reachable, so a statement that has not started yet is given up
  with its session (the app opens a new one); one that is returning rows stops when its stream is
  dropped.
- The navigator, from the catalog calls. SQL for Db2 is written with Db2's quoting (bare names
  fold to upper case, rows are capped with `FETCH FIRST`); the generic driver writes standard SQL.

## What does not

- No explain, no object DDL, no administration tabs, no bulk loading (imports use `INSERT`).
- **Text and binary values longer than 64 KiB are an error**, rather than a huge buffer.
- **Data editing in the grid** is not tested on these databases. The grid does not block it, but
  without savepoints a failed save in manual-commit mode rolls the session's whole transaction
  back and says so.
- Terminal mode is not available.

## Agents

Read-only agents rely on the statement check alone. See [Agents and the CLI](../../AGENTS.md).

## Known limits

- Windows only.
- Db2 has no container in the test setup, so the Db2 connection string and driver detection are
  checked by unit tests only.

## Tested on

The generic ODBC driver is run against SQL Server 2022 through Windows' own `SQL Server` driver:
types, long results, errors with their SQLSTATE, metadata, commit and rollback, cancel of a waiting
statement, SQL Server's own literals and types, and imports. IBM Db2 is not tested against a
server.

See also: [Connecting](../connect.md), [Troubleshooting](../troubleshooting.md).
