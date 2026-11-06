[← Databases](README.md)

# Redis

The driver is built on the `redis` crate (RESP3 when the server speaks it, with rustls for TLS).
It opens Redis 6 and later, and Redis Stack for JSON keys. The editor takes **Redis commands**, one
per line. The driver is part of the app; nothing is downloaded.

## Connect

| You paste | Read as |
|---|---|
| `redis://:s%40cret@cache:6380/3` | plain TCP, password only, database 3 |
| `redis://app:pw@cache/0` | an ACL user and its password |
| `rediss://app:pw@cache` | TLS (verified: `verify-full`) |

The default port is 6379. The path (or the Database field) is the **database number**, `0` when
empty; anything else is an error. The user and password are the ACL's: a password alone signs in
as the default user.

**TLS** is on for `rediss://` URLs and for SSL modes from `require` up (`redis://` means no TLS and
`prefer` does not turn it on, because Redis takes TLS on a port of its own). `require` does not
check the certificate; `verify-ca` and `verify-full` do. A CA file and a client certificate go in
the TLS fields. The session opens one connection per database it uses, because `SELECT` on a
shared connection would move every other user of it.

## Privileges

The ACL rules of the login decide what the navigator and the commands can do. The navigator
reads `INFO` and, for each key type, runs `SCAN … TYPE`; browsing a type runs `BROWSE`, never
`KEYS`. There are no administration tabs.

## What works

- Commands in the editor, quoted as `redis-cli` quotes them; `#` and `--` lines are comments.
  Replies are rows, with nested replies as JSON in the document tree (`F7`).
- `SELECT n` moves the session to another database. `MULTI` queues commands until `EXEC`, which
  sends them as one transaction and shows each reply.
- The navigator lists the databases that hold keys, each with a table per key type: `string`,
  `hash`, `list`, `set`, `zset`, `stream`, and `json` with RedisJSON. Opening one runs
  `BROWSE db0.hash`, optionally with `MATCH` and `LIMIT`, reading big keys a chunk at a time.
- **The grid edits rows**: values, TTLs, key names, hash fields, members and scores; new rows add
  keys and items, deleted rows remove them. The preview shows the commands, which run as one
  `MULTI`/`EXEC`; a command the server refuses does not undo the others. An *Undo script* puts the
  values back. A key, field or member that already exists stops the save before anything runs.
- `FLUSHDB` and `FLUSHALL` ask first, like an `UPDATE` without `WHERE`; on production connections
  commands that change the server (`CONFIG SET`, `SHUTDOWN`) ask as DDL does. Read-only
  connections run only commands that read.
- Cancel stops a blocking command (`BLPOP`, `XREAD BLOCK`).
- Terminal mode runs `redis-cli` if it is on the PATH, with the password in its environment.
- All the commands and key types are described in [FEATURES.md](../../FEATURES.md#redis).

## What does not

- Subscriptions (`SUBSCRIBE` and the like) and `MONITOR` are refused: they would take over the
  connection.
- No manual commit: `MULTI`/`EXEC` is the only transaction.
- No explain, no object DDL, no SQL tools (table editor, ER diagram, schema comparison, query
  builder, SQL import), no snapshots before changes.

## Agents

`run_query` and `sample_rows` (`BROWSE … LIMIT n`) work; any command that is not a read is asked
about first. The commands that show the server's credentials (`ACL GETUSER`, `ACL LIST`, and a
`CONFIG GET` whose pattern matches a password setting) are refused to read-only agents. Refused on
Redis: `explain`, `get_er_diagram`, `propose_edits` and `query_across_databases`. See
[Agents and the CLI](../../AGENTS.md).

## Known limits

- Older than 6.0: it connects with a warning that some features may not work.
- A table's key count shows only when the database has up to 10,000 keys.
- Values that are not UTF-8 show quoted with escapes.

## Tested on

The conformance suite runs on Redis Stack 7.4 (with RedisJSON): metadata, `BROWSE` on every type,
commands and their errors, saving and undoing grid edits, cancel of blocking commands, ACL users
and database selection, and a lost connection that reopens. Redis 6 is the lowest version the
driver accepts but is not run by the suite.

See also: [Connecting](../connect.md), [Troubleshooting](../troubleshooting.md).
