[← Databases](README.md)

# Oracle

The driver uses Oracle's client library (ODPI-C, through the `oracle` crate), loaded when you
connect. The library is Oracle Instant Client, which the app downloads on first use.

## First use: Instant Client

When you choose **Oracle** for a new connection and the client is not on this machine, a popup
opens over the form: *Oracle needs Oracle Instant Client*. It shows the package (Basic Light
23.26.4), its size, its license (Oracle Free Use Terms and Conditions) and where it comes from,
and checks the download against a SHA-256. **Download** fetches it into the app's data folder;
**Cancel** leaves the form usable, and the offer comes back at *Test connection* and at the first
opening. *Settings › Drivers* lists the client and removes it.

- The app downloads it on Windows (x64) and Linux (x64 and ARM64). On macOS it downloads
  nothing: install Instant Client from Oracle and put its folder on the library path.
- If the app has not downloaded one, it uses an Instant Client or full client that the system
  can load.
- Windows needs the Microsoft Visual C++ Redistributable, and Linux the `libaio` library
  (`libaio1`, `libaio1t64` or `libaio` depending on the distribution); when the client cannot
  load, the error names the missing piece.

## Connect

There is no `oracle://` URL; paste a JDBC URL instead, such as
`jdbc:oracle:thin:@host:1521/service` (thin and OCI forms are read), or fill in the form:

- **Host** and **port** (default 1521), and **Service name** in the Database field
  (for example `FREEPDB1`). They are written as an Easy Connect string, `host:port/service`.
- **Without a host**, the Database field is a net service name from `tnsnames.ora`, or a whole
  connect descriptor, written as it is.
- **User and password.** With no user, the operating system's account or an Oracle wallet signs
  in. The user may be written the SQL*Plus way, `sys as sysdba`, or you can choose the
  **Role** (`SYSDBA`, `SYSOPER`, `SYSBACKUP`, `SYSDG`, `SYSKM`, `SYSASM`).
- **TLS**: modes `disable` and `prefer` use plain TCP; `require` and above use `tcps://`
  with the system's trusted certificates. `require` skips the server name match; `verify-ca`
  and `verify-full` check it. The connect timeout applies. Through an SSH tunnel the local end
  of the tunnel is used.

## Privileges

Browsing reads the `ALL_*` views, so the login sees the objects it has rights on. Cancel sends a
break; if the statement is still running after a couple of seconds, the app cancels it from a
second session with `ALTER SYSTEM CANCEL SQL`, which needs that privilege, and failing that gives
up the session (its transaction is rolled back and a new connection is opened).

## What works

- Data editing in the grid, by primary key (or a unique key on non-null columns, or a virtual key).
- Manual commit and rollback.
- PL/SQL blocks: a script ends a block with a `/` line. `DBMS_OUTPUT` text comes back as messages.
- Navigator (tables, views, materialized views, sequences, synonyms, functions, procedures,
  triggers, types and packages), object DDL, and schema tools.
- Explain shows the plan from `EXPLAIN PLAN` and `DBMS_XPLAN`, estimated only: there is no
  Explain analyze.
- Compile errors, numbers and nanosecond times, JSON, vectors, cursors, long LOBs and file columns
  are read back as the database holds them.
- No sessions, locks, dashboard, statements, users or parameters tabs.
- Terminal mode is not available.

## Agents

Read-only agents rely on the statement check alone: Oracle has no server-side read-only session
here. A statement that calls an Oracle package, such as `UTL_HTTP`, is asked about before it
runs, as a change. See [Agents and the CLI](../../AGENTS.md).

## Known limits

- The app does not compare the server's version with a minimum; the client it downloads is 23.26.4.
- Instant Client is not downloaded on macOS.

## Tested on

The conformance suite runs on Oracle Free 23.26 (container `gvenzl/oracle-free`), with Instant
Client downloaded or the system's: types, long results, errors that point at the mistake, PL/SQL,
navigator, commit and rollback, saving many edited rows, cancel (also of a PL/SQL sleep),
plans, DDL that runs back, connecting as `SYSDBA`, and schema comparison. Other versions are not
tested.

See also: [Connecting](../connect.md), [Troubleshooting](../troubleshooting.md).
