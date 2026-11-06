[← Databases](README.md)

# MongoDB

The driver is MongoDB's official Rust driver (with rustls, so no OpenSSL). It opens MongoDB 5.0 and
later. The editor takes **mongosh syntax**, not SQL. The driver is part of the app; nothing is
downloaded.

## Connect

| You paste | Read as |
|---|---|
| `mongodb://user:pass@host:27017/shop?authSource=admin&replicaSet=rs0` | the URL form; default port 27017 |
| `mongodb://a:27017,b:27018/?authSource=admin` | a seed list, kept whole in the Host field |
| `mongodb+srv://user:pass@cluster0.example.net/shop` | SRV: the hosts come from DNS |

What does not fit the form's fields goes in the **Database field**, after a `?`, as connection
string options: `shop?authSource=admin&replicaSet=rs0`, with `srv=true` for `mongodb+srv`. The
name before the `?` is the database statements run in until `use` changes it (`test` when
empty).

- **Signing in**: a user and password, authenticated against `admin` unless `authSource` says
  otherwise. A pasted URL whose path names a database uses it as the auth source, as the
  connection string spec says. Options that hold a secret (a password in
  `authMechanismProperties`, say) are not kept when you paste a URL; the form lists what it
  left out.
- **TLS**: `tls=true` or `ssl=true` in the URL means `verify-full`; `tls=false` means `disable`.
  MongoDB has no opportunistic TLS, so `prefer` (the default) keeps what the connection string
  implies: TLS on for `mongodb+srv`, off otherwise. `require` turns TLS on and accepts any
  certificate; `verify-ca` and `verify-full` both check the chain and the host name. `sslrootcert`
  (or `tlsCAFile`) names your CA's file; a client certificate can be given in the form, or with
  `tlsCertificateKeyFile` in the Database field. The connection string's TLS options are kept in
  every mode.
- **Through an SSH tunnel** only the one server behind it is reached (`directConnection=true`).

## Privileges

The navigator lists databases and collections the login can list, and samples documents to show
fields. Nothing else is read for administration: there are no administration tabs. The
statements you write need the roles they need.

## What works

- Statements in mongosh syntax, with the shell's values (`ObjectId(...)`, `ISODate(...)`,
  `NumberLong(...)`, `/regex/i`) and Extended JSON:
  - **Reading**: `find` with `sort`, `limit`, `skip`, `projection`, `collation`, `hint`, `min`, `max`,
    `maxTimeMS`, `readConcern` and the like, `findOne`, `aggregate`, `countDocuments`,
    `estimatedDocumentCount`, `distinct`.
  - **Writing**: `insertOne`/`insertMany`, `updateOne`/`updateMany`, `replaceOne`,
    `deleteOne`/`deleteMany`, `bulkWrite`, `findOneAndUpdate`/`findOneAndReplace`/`findOneAndDelete` and
    `findAndModify`, with their options (`arrayFilters`, `collation`, `hint`, `let`, `upsert`,
    `ordered`, `writeConcern`). An option PlatypusDB does not know is refused by name, never dropped.
  - **Indexes and collections**: `createIndex`, `createIndexes`, `dropIndex`, `dropIndexes`, `hideIndex`,
    `unhideIndex`, `getIndexes`, `createCollection`, `createView`, `drop`, `renameCollection`,
    `convertToCapped`, `stats`, `validate`, `latencyStats`, `dataSize`, `storageSize`, `totalIndexSize`,
    `totalSize`, `isCapped`, `exists`.
  - **Plans**: `.explain()` after a statement or `db.c.explain().find(…)` before it, with a verbosity,
    and the **Explain** of the editor. A plan never applies a write.
  - **The server**: `db.runCommand`, `db.adminCommand`, `db.aggregate`, `db.serverStatus`, `db.currentOp`,
    `db.killOp`, the profiler, users and roles (`db.createUser` and the like), `rs.status`, `rs.conf`,
    `sh.status`, `show dbs`, `show collections`, `show users`, `show roles`, `show profile`, `show logs`
    and `use`.
- Results are a grid of the top-level fields, with the focused document as a tree beside it
  (`F7`). The navigator lists databases, collections, fields sampled from the documents, and
  indexes; the editor completes collections, methods and fields.
- Cancel stops a running statement; an abandoned result frees the connection.
- A collection's definition as DDL.
- **Generate data** fills a collection with fake documents.
- The same detail is in [FEATURES.md](../../FEATURES.md#mongodb).

## What does not

- **No data editing in the grid**: results stay read-only. Change documents with `updateOne`,
  `insertOne` and the like.
- **No transactions** (no manual commit).
- **No JavaScript**: the editor runs statements, so `forEach`, `map`, `next`, `mapReduce`, the older
  bulk builders (use `bulkWrite`) and variables are refused, saying so. `watch` is refused too: a change
  stream would take over the connection. A `.comment()` on a read is left out: the app's own finds the
  operation to cancel it.
- `bulkWrite` is not atomic, as in mongosh: what ran before a failure stays, and the error says how far
  it got. Neighbouring inserts travel together, every other write on its own trip.
- No SQL: no table editor, ER diagram, schema comparison, query builder, SQL import, snapshots
  before changes, or terminal mode (`mongosh` takes the password only on its command line).

## Agents

`run_query`, `sample_rows` (`find({}).limit(n)`) and the rest work with mongosh statements; a
statement that is not a plain query is asked about first. Refused on MongoDB: `explain`,
`get_er_diagram`, `propose_edits` and `query_across_databases`; `estimate_change` answers with a
note. Sensitive columns are masked by top-level field only: a sensitive field inside a document is
not told apart. See [Agents and the CLI](../../AGENTS.md).

## Known limits

- Older than 5.0: it connects with a warning that some features may not work.
- The driver is a pool that reconnects by itself, so a lost connection is not reported the way it
  is for the other databases.

## Tested on

The conformance suite runs on MongoDB 5.0 and 8.0: server info, metadata, cancel, abandoned
results, errors with their position, fake documents, and values in later documents that differ
from the first.

See also: [Connecting](../connect.md), [Troubleshooting](../troubleshooting.md).
