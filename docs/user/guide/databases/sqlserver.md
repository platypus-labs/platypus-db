[← Databases](README.md)

# SQL Server and Azure SQL

The driver speaks TDS itself (a patched `tiberius`), so there is nothing to install or download.
It is written for SQL Server 2017 (14.0) and later, and for Azure SQL (not tested against the
service: see the end of this page).

## Connect

| You paste | Read as |
|---|---|
| `sqlserver://sa:pass@host:1433/master?encrypt=true&trustServerCertificate=true` (or `mssql://`) | the URL form; default port 1433 |
| `jdbc:sqlserver://host:1433;databaseName=shop;encrypt=true` | a JDBC URL (Microsoft's driver, jTDS or the old `jdbc:microsoft:`) |
| `Server=tcp:db.example.com,1433;Database=shop;User Id=sa;Password=…;Encrypt=True` | an ADO.NET or ODBC connection string |

In a URL, `database` or `databaseName` names the database, and `encrypt` and
`trustServerCertificate` are read as the JDBC and ADO.NET strings read them (unless `sslmode` is
also given).

**Named instances** (`host\SQLEXPRESS`) are not looked up through the SQL Server Browser: give the
instance's port. A pasted string with an instance and no port says so.

**TLS**: `disable` turns encryption off. `prefer` (the default) encrypts and does not check the
certificate (servers often have a self-signed one), and falls back to plain if the server has no
encryption. `require` encrypts without checking. `verify-ca` and `verify-full` both check the
chain **and** the host name, because the TLS library always checks the name; point `sslrootcert`
at your CA's file, or the system's trusted certificates are used.

**Signing in**:

- *SQL Server login*: user and password.
- *Windows sign-in* (`Integrated Security=SSPI` in a pasted string): only when PlatypusDB runs on
  Windows; elsewhere it is an error.
- *Microsoft Entra ID*: a token made by the `az` tool, which must be installed and signed in.
  This is how Azure SQL's Entra sign-in is reached.

**Azure SQL**: the gateway may send the client to the database's own node; the app follows one
such redirection, except through an SSH tunnel, which leads to one address only.

## Privileges

Browsing needs what the login can see. The administration tabs read:

- **Sessions** and **Locks**: `sys.dm_exec_sessions`, `dm_exec_requests`, `dm_exec_connections` and
  `dm_tran_locks`. To see what other logins do, the login needs `VIEW SERVER STATE`.
- **Costliest statements**: `sys.dm_exec_query_stats`, the plan cache.
- **Tables and indexes**: `sys.tables`, `dm_db_partition_stats`, `dm_db_index_usage_stats` and
  `sys.stats`.
- **Parameters**: `sys.configurations`; *Open in the editor* writes a `sp_configure` statement.
- **Dashboard**: performance counters, `sys.databases` and `sys.master_files` sizes, and
  availability groups where there are any; a figure the login cannot read shows "—".
- **Users and roles**: the server's logins, roles and grants.
- **Maintenance**: `UPDATE STATISTICS`, `ALTER INDEX ALL … REBUILD` and `DBCC CHECKTABLE`.

## What works

- Data editing in the grid, by primary key (or a unique key on non-null columns, or a virtual key).
- Manual commit and rollback; cancelling a running statement (a TDS attention) keeps the session,
  its transaction and its temporary tables.
- Several result sets from one batch, and `PRINT` messages.
- Several databases per connection: a connection that names its database shows only that one;
  *Show all databases* lists the rest.
- Explain shows the **estimated** plan (`SHOWPLAN`). Explain analyze is refused.
- *End session* is `KILL`. Cancelling another session's statement is not possible on SQL Server,
  so the tab offers only to end the session.
- Imports use SQL Server's bulk copy (`OPENJSON` inserts for tables with `money`, `xml` or legacy
  columns).
- Schema tools (table editor, ER diagram, comparison), also for computed and collated columns,
  rich indexes and temporal tables. XML and spatial indexes are left out of a comparison.
- Terminal mode runs `sqlcmd`, if it is on the PATH.

## Agents

On SQL Server the statement check alone stops writes for read-only agents (no server-side
read-only session); an agent's transaction is rolled back. `server_activity` and `top_queries`
work. See [Agents and the CLI](../../AGENTS.md).

## Known limits

- Older than 2017: the app connects and warns that some features may not work.
- No Explain analyze and no named-instance lookup.

## Tested on

The conformance suite runs on SQL Server 2017 and 2022 (containers), and the generic ODBC driver
is tried against the same 2022 container. Azure SQL is not tested: its redirection, Entra sign-in
and encryption rules are in the code but have not been run against the service.

See also: [Connecting](../connect.md), [Troubleshooting](../troubleshooting.md).
