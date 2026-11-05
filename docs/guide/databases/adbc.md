[← Databases](README.md)

# Snowflake, BigQuery, Databricks, Trino and other ADBC drivers

These databases are reached through **ADBC** (Arrow Database Connectivity) drivers, libraries that
the app loads when you connect. Snowflake, BigQuery, Databricks and Trino have a tile each; any
other database with an ADBC driver connects through **Other ADBC driver**.

## First use: the driver download

The four named databases need their driver on this machine. When you choose one for a new
connection and the driver is not installed, a popup opens over the form: *Snowflake needs a driver*
(and so on). It says it is an ADBC driver, downloaded once for all connections of that database,
and shows the package, its version, its license and where it comes from: the
[dbc driver registry](https://docs.columnar.tech/dbc/) (Columnar). **Download** fetches it,
checks its SHA-256 (a package without one is refused) and keeps it with the app's data;
**Cancel** leaves the form usable, and the offer returns at *Test connection* and at the first
opening.

The offer reads only what is on this machine, so it appears offline too; the download needs the
network. *Settings › Drivers* lists the installed drivers and removes them, checks for newer
versions once a day (or on *Check for updates*), installs or goes back to another version with
*Versions…*, and can install updates by itself. After a change, the saved connections that use
the driver are tried (connect, version, schemas, `SELECT 1`), with *Go back* if one fails.
The other-driver choice downloads nothing: you point it at a library file.

## Connect

Each database has its own fields:

| Database | Fields |
|---|---|
| Snowflake | **Account** (for example `xy12345.eu-west-1`), user, password, warehouse, role, database, schema |
| BigQuery | **Project**, dataset, and a service account key file (JSON); empty means Google's Application Default Credentials. No password |
| Databricks | **Server hostname**, **HTTP path**, catalog, schema; the password field is the **access token** |
| Trino | **Host**, port (8080 by default), **user**, catalog, schema, password (optional), and TLS |
| Other ADBC driver | the **driver library** file, an optional **entrypoint** symbol, a URI, user and password; every other option you add is passed to the driver |

**Trino's TLS** choices are *Automatic* (HTTPS on port 443, HTTP otherwise), *Off* (HTTP), *On,
verify certificate* and *On, unverified*.

**Other ADBC driver**: the library is the driver file you downloaded or built. The entrypoint is
needed only for a library that does not export `AdbcDriverInit` (for DuckDB's ADBC library it is
`duckdb_adbc_init`). The URI and the options are whatever that driver documents. A missing file,
or one that is not a driver, is reported when you connect.

## Privileges

Whatever the account or token has on the service. The navigator reads the catalogs, schemas, tables
and columns with ADBC's metadata call.

## What works

- Queries, results, the navigator and autocompletion. The result streams as Arrow batches.
- Cancel: the app asks the driver to cancel; Go-based drivers only cancel their own calls, so a
  statement that does not stop is given up with its session, and the app opens a new one.
- Object DDL on **Trino** only (`SHOW CREATE`). Snowflake, BigQuery, Databricks and other drivers
  show no DDL.
- Foreign keys that point at a table are not listed (it would mean reading every table's keys).
- Exports, and imports of files (typed imports are tested on Trino).

## What does not

- **No manual commit**: every statement commits by itself, and turning autocommit off says that
  manual commit with an ADBC driver is not supported.
- **No explain**, and no administration tabs.
- **No data editing in the grid**: a save checks each change in a transaction, and these
  connections have none. Results stay read-only; use SQL for changes.
- Terminal mode is not available.

## Agents

`run_query`, `sample_rows` and the other tools work, within the checks. With no server-side
read-only session, read-only agents rely on the statement check alone. Statements that act
outside it are asked about before they run: Trino's `query()` and Snowflake's `SYSTEM$` functions.
See [Agents and the CLI](../../AGENTS.md).

## Known limits

- A cloud warehouse sign-in other than the fields above (key pair, single sign-on) is not in the
  form; the generic driver takes the driver's own options.
- Snowflake, BigQuery and Databricks are in the code and share the driver layer with Trino, but
  have no test against the real services.

## Tested on

The conformance suite runs **Trino 484** through its downloaded driver (types, long results,
cancel, metadata, errors, statements that end with a terminator, typed imports, and rows and
UUIDs) and the **generic ADBC driver** on the Apache SQLite ADBC driver and on DuckDB's. Snowflake,
BigQuery and Databricks are not tested.

See also: [Connecting](../connect.md), [Troubleshooting](../troubleshooting.md).
