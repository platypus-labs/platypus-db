[← Databases](README.md)

# Apache Cassandra and ScyllaDB

One driver, built on the `scylla` crate (with rustls), speaks CQL over the native protocol to both.
The server says which it is, and the connection shows it. The driver is part of the app; nothing is
downloaded. The editor takes CQL.

## Connect

| You paste | Read as |
|---|---|
| `cassandra://cassandra:s%40cret@node1:9043/shop` | plain, port 9043, keyspace `shop` |
| `scylla://node1` | the same driver, for ScyllaDB |
| `cassandras://…` and `scyllas://…` | with TLS (verified: `verify-full`) |

The default port is 9042. The **Host** field is a contact point, or several `host:port` pairs
separated by commas. The **Keyspace** field (optional) is the keyspace the session starts in; the
navigator lists all of them either way.

- **One contact point, or a tunnel**: that node alone serves every statement. A cluster
  advertises its nodes' own addresses, which a container, a NAT or a tunnel hides, so the driver
  does not follow them. With several contact points the driver is given all of them and may use
  any.
- **Signing in**: a user and password, for the server's password authenticator. Without a user
  the connection is anonymous.
- **TLS**: CQL cannot negotiate it, so TLS is on for `cassandras://` and `scyllas://` URLs and
  for SSL modes from `require` up; `prefer` is plain. `require` does not check the certificate;
  `verify-ca` and `verify-full` do. A CA file and a client certificate go in the TLS fields.
- **Statements have no timeout** of the driver's own: one runs until it finishes or you cancel.

## Privileges

The navigator reads `system_schema`, which Cassandra 3 and later and ScyllaDB share; the login
sees what the server lets it read there. There are no administration tabs. Writes need the
permissions CQL asks for.

## What works

- CQL scripts, split by the editor (`$$ … $$` strings, `//` comments and a `BEGIN BATCH … APPLY
  BATCH` are understood), with typed results read page by page as you scroll, using the server's
  paging state.
- The navigator: keyspaces as schemas, tables, materialized views, user-defined types, functions
  and aggregates, with each table's partition and clustering keys and its indexes; the DDL of each
  object.
- **The grid edits rows**, by the full primary key (partition and clustering columns): an edited
  row is an `UPDATE`, a new row an `INSERT` that needs every key column, a deleted row a
  `DELETE`. The preview shows the statements, which run one after the other; one the server
  refuses stops the rest. *Undo script* puts the old values back. Key columns cannot be changed
  (delete the row and insert another), and counters are refused.
- Lightweight transactions (`IF …`) are reported as such with their outcome.
- Cancel stops a result being read. Errors keep the server's message.
- `USE keyspace` changes the session's keyspace, which the driver keeps.

## What does not

- **No transactions**: every statement takes effect on its own, and there is no manual commit.
- **CQL writes are upserts and report no row count**, so a statement that returns no rows reports
  none changed; an update of a row somebody deleted meanwhile writes it again.
- The data viewer has no filter and sort bar, and shows no row count.
- No explain, and no SQL tools: no table editor, ER diagram, schema comparison, query builder,
  SQL import, snapshots before changes, or terminal mode (`cqlsh` takes the password only on its
  command line).

## Agents

`run_query` and `sample_rows` (a CQL `SELECT … LIMIT n`) work; a statement that changes data is
asked about first. Refused on Cassandra: `explain` and `propose_edits`; `estimate_change` answers
with a note. See [Agents and the CLI](../../AGENTS.md).

## Known limits

- Cassandra older than 4.0 and ScyllaDB older than 5.4 are refused when connecting, with "older
  than … the oldest version supported", unlike the other databases, which connect with a warning.
- No joins, no `OFFSET`, and rows are found only by their primary key, as CQL has it.

## Tested on

The conformance suite runs on Apache Cassandra 5 and ScyllaDB 2025.1, without login: server info,
scripts, every type as a typed column, paging, lightweight transactions, errors, cancel, the
catalog and its DDL (also functions and aggregates), saving the grid's changes and undoing them,
a wrong or missing login being refused, and a tunnel address. Cassandra 4.0 and ScyllaDB 5.4 are the
lowest versions the driver accepts and are not run by the suite.

See also: [Connecting](../connect.md), [Troubleshooting](../troubleshooting.md).
