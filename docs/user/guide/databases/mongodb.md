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
- **Manual commit** on a replica set or a sharded cluster (see [Transactions](#transactions)).
- **Editing documents in the grid** (see [Edit documents in the grid](#edit-documents-in-the-grid)).
- A collection's definition as DDL.
- **Generate data** fills a collection with fake documents.
- The same detail is in [FEATURES.md](../../FEATURES.md#mongodb).

## What does not

- **Only some results are editable in the grid**: the documents of one collection (see
  [Edit documents in the grid](#edit-documents-in-the-grid)). Every other result, such as an
  aggregation, a view or a `findOne`, stays read-only. Change those documents with `updateOne`,
  `insertOne` and the like.
- **No transactions on a standalone server**: MongoDB runs them only on a replica set or a sharded
  cluster. Manual commit is refused there, saying so.
- **No JavaScript**: the editor runs statements, so `forEach`, `map`, `next`, `mapReduce`, the older
  bulk builders (use `bulkWrite`) and variables are refused, saying so. `watch` is refused too: a change
  stream would take over the connection. A `.comment()` on a read is left out: the app's own finds the
  operation to cancel it.
- `bulkWrite` is not atomic, as in mongosh: what ran before a failure stays, and the error says how far
  it got. Neighbouring inserts travel together, every other write on its own trip.
- No SQL: no table editor, ER diagram, schema comparison, query builder, SQL import, snapshots
  before changes, or terminal mode (`mongosh` takes the password only on its command line).

## Edit documents in the grid

The grid edits the result of a plain `find` on one collection: the data viewer, or a statement you
type, such as `db.orders.find({ status: 'paid' }).sort({ _id: 1 }).limit(20)`. The collection can be
named with `getCollection("my orders")` or `db.getSiblingDB("shop").orders`. After `use`, the
collection is the one of the database you switched to. A projection is fine when it only lists
fields to include and keeps `_id`. Views, aggregations, `findOne`, `distinct`, counts, a projection
without `_id` and a projection that leaves fields out stay read-only.

A document is found by its `_id`. A result without `_id` is read-only. **Preview SQL** shows the
mongosh that will run, one statement per document:

- an edited document is `updateOne({ _id: … }, { $set: { … } })`. A NULL cell writes `null`. When
  a number changes, the update is a pipeline instead, `[{ $set: { … } }]`: the number is written
  with a `$switch` on the type the document stores (see below), and every other value as a
  `$literal`, so that a string such as `$total` or a document with `$` keys is saved as it is;
- a new document is `insertOne({ … })`. Cells left NULL are not sent. Without an `_id`, the app
  makes an `ObjectId`;
- a deleted document is `deleteOne({ _id: … })`.

Values are written with the BSON type of their column: `bool`, `string`, `objectId`, `date`, and
`document` and `array` from the JSON the grid shows. A date is written like `2024-01-02 10:30:00`,
with or without the time zone (UTC without one). Numbers inside a document or an array keep the
type JSON gives them: a whole number is an int (a long beyond 32 bits), a number with a fraction or
an exponent a double.

A number keeps the type the document stores, `int`, `long`, `double` or `decimal`, whenever that
type holds the new value exactly: a column's type is only what its documents share (ints and longs
make a `long` column, any of them with doubles a `double` one), so it does not say what one
document holds. An int edited to `7` stays an int, a long a long, a double a double. A value the
stored type cannot hold takes the column's type, or the first numeric type that holds it: `3.5` in
an int field becomes a double, `3000000000` a long. A long beyond 2^53 is never written as a
double, which would lose digits. Nothing is converted on the server: the app only writes the
number in the types it knows hold it exactly.

These are refused before anything runs, with the reason:

- a change to `_id`: MongoDB never changes it. Delete the document and insert it again;
- a column of mixed types (`mixed`): the text of a value does not tell a string from a number;
- binary data and the other types the grid cannot write back exactly (regular expressions,
  timestamps, JavaScript);
- a field with a `.` in its name or a name that starts with `$`: `$set` would read it as a path or
  an operator;
- a value that does not fit its type, such as `many` in an `int` column or an array in a
  `document` column.

Each statement must change exactly one document. If one finds none, because the document was
deleted or its `_id` changed since the grid read it, the save stops and says which command.

**All or nothing needs a replica set.** On a replica set or a sharded cluster, a save runs in a
transaction of its own and commits only if every command worked. If you have a transaction open
(manual commit), the save runs in it and waits for Commit. If a command fails, MongoDB has no
savepoints: the whole transaction is rolled back, with the changes you had not committed yet, and
the error says so. On a standalone server there are no transactions: the commands run one after the
other, and a failure stops the rest. The commands before it stay saved, and the error says how many.

The undo script that follows a save puts the documents back: `$set` of the old values for edits,
`deleteOne` for new documents, `insertOne` of the loaded fields for deleted ones. Lines starting
with `//` say what it cannot restore exactly: fields the result did not show, columns of types the
grid cannot write, fields that were NULL (they come back as `null`, or are left out), and numbers
whose type the edit may have changed (they come back as numbers of the type they have then, by the
same `$switch`).

## Transactions

Manual commit works on a replica set or a sharded cluster (a `mongos`); a single-node replica set
is enough. On a standalone server it is refused: the **Auto-commit** button is disabled and says
why. A production connection to a replica set opens in manual commit, with Commit and Rollback. The connection keeps one session: the first
change after you switch to manual commit, or after a Commit or Rollback, opens a transaction in it
(an insert, update or delete, `createCollection`, `createIndex`, a save of the grid), and Commit or
Rollback ends it. Reads before it open none: they see what is committed, as in autocommit, so
browsing data never leaves a transaction open for the server to end after its lifetime. Once a
transaction is open, your reads run in it and see its writes; other sessions see them only after
Commit.

- **In the transaction**, once one is open (the reads) or opening it (the rest): `find`, `findOne`,
  `aggregate`, `countDocuments`, `distinct`, the inserts,
  updates, replaces and deletes, `bulkWrite`, the `findOneAnd…` methods, `createCollection` and
  `createIndex`. MongoDB takes an index in a transaction only on a collection created in the same
  transaction: on one that exists, the server refuses it and ends the transaction.
- **Outside it**, as in autocommit mode, without touching it: what reads metadata or talks to the
  server. `show …`, `use`, `db.stats()`, `db.getCollectionNames()`, `getIndexes`, the collection's
  statistics, `estimatedDocumentCount`, `.explain()`, `db.runCommand`, `db.adminCommand`, `rs.*`,
  `sh.*`, users and roles, the profiler, the navigator and completion. Reads of the `admin`,
  `config` and `local` databases and of `system.*` collections run outside too.
- **Refused in manual commit**: `drop`, `renameCollection`, `dropIndex`, `dropIndexes`, `hideIndex`,
  `convertToCapped`, `createView`, a time-series `createCollection`, an `aggregate` with `$out` or
  `$merge`, and writes to `admin`, `config`, `local` or `system.*`. MongoDB does not run them in a
  transaction, and run outside it they would apply at once, which Rollback could not undo. The
  error says so; nothing is sent and the open transaction stays as it was. Switch to autocommit to
  run them.
- A result read in a transaction keeps loading as you scroll, a batch at a time, and other
  statements can run in between. A result read before the transaction opened keeps loading outside
  it. Its columns come from its first documents. After Commit or
  Rollback the rest of it cannot be read: run the statement again.

**When the server ends a transaction.** MongoDB aborts a transaction that stays open longer than
`transactionLifetimeLimitSeconds`, 60 seconds by default. It also aborts one on some errors: a
duplicate key, a write conflict with another session, a statement it refuses in a transaction,
a time limit (`maxTimeMS`) reached. Other errors, such as an unknown pipeline stage, leave it
open. After an error PlatypusDB asks the server whether the transaction still exists. When it does
not, the statement's or Commit's error says that the server ended the transaction and that its
changes were discarded (with the time limit when that was the reason), the transaction shows as
closed, and the next statement opens a new one. Cancelling a statement in a transaction rolls the
transaction back: the server ends a transaction whose operation is killed, and what the statement
did is unknown.

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
from the first. Transactions are tested on a single-node replica set of MongoDB 8.0: commit,
rollback, reads inside and outside, a result of several batches, metadata in manual commit, and a
transaction the server ended for its time limit.

See also: [Connecting](../connect.md), [Troubleshooting](../troubleshooting.md).
