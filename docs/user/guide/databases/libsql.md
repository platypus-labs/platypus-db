[← Databases](README.md)

# libSQL and Turso

The driver speaks the **Hrana 3** protocol over HTTP(S) to a libSQL server (`sqld`) or to Turso.
The SQL is SQLite's, so the navigator, the types and the plans are SQLite's too. The driver is part
of the app; nothing is downloaded.

## Connect

| You paste | Read as |
|---|---|
| `libsql://db-acme.turso.io?authToken=…` | HTTPS to that host, the token as the password |
| `libsql://:token@localhost:8080?sslmode=disable` | plain HTTP, the token in the password position |
| `libsql://localhost:8080?tls=0&authToken=t` | `tls=0` is plain HTTP, `tls=1` is HTTPS |
| `wss://…`, `ws://…` | the same server over HTTP(S): `wss` is TLS, `ws` is not |
| `https://host` or `http://host` in a libSQL form | the server's HTTP address |

Without a port the scheme's default is used: 443, or 80 when TLS is off.

- **Token**: the password field, or `authToken` in the URL.
- **TLS**: a `libsql://` URL is HTTPS with the certificate and host name checked (`verify-full`),
  because the URL carries the token. The Turso tile in the form starts at `verify-full` too. For a
  local `sqld` without TLS choose `disable`, or use `tls=0` in the URL. `sslrootcert` names your
  CA's file; otherwise the system's trusted certificates are used.

## Privileges

Whatever the token allows: the app sends the statements you run, and the SQLite catalog queries
for the navigator.

## What works

- Data editing in the grid, by primary key (or a unique key on non-null columns, or a virtual key).
- Manual commit and rollback. A statement runs on a stream the server keeps open between requests
  (a `baton`), so a transaction and temporary tables last. If the server drops the stream outside
  a transaction, the app opens another and goes on; inside a transaction, the connection reports
  itself lost and the transaction with it.
- Explain shows SQLite's query plan; there is no Explain analyze.
- Metadata search and object DDL, and schema tools as for SQLite.
- No sessions, locks, dashboard, users or parameters tabs.

## What does not

- **Cancel**: HTTP has no way to stop a running statement, so Cancel answers that cancelling
  libSQL statements is not supported.
- Results arrive whole from the server, not row by row.
- Terminal mode is not available: libSQL servers have no command-line client to run scripts with.

## Agents

Read-only agents and read-only connections rely on the statement check alone; the server is not
told to refuse writes. See [Agents and the CLI](../../AGENTS.md).

## Known limits

- A network failure after a statement was sent can leave it unknown whether it ran; the app does
  not send it again.

## Tested on

The conformance suite runs on `sqld` v0.24.33 (container `ghcr.io/tursodatabase/libsql-server`),
without TLS. Turso's cloud is not tested.

See also: [Connecting](../connect.md), [Troubleshooting](../troubleshooting.md).
