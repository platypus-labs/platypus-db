[← Guide](../README.md)

# Databases

One page for each database: how to connect, what the login needs, what works and what does not,
and which versions the test suite runs on. [Connecting](../connect.md) explains the parts every
database shares (SSH, proxy, the keychain, projects), [Troubleshooting](../troubleshooting.md)
the errors, and [FEATURES.md](../../FEATURES.md) is the reference by feature.

| Page | Databases |
|---|---|
| [PostgreSQL](postgresql.md) | PostgreSQL, CockroachDB, YugabyteDB, TimescaleDB, Supabase, Neon, AlloyDB |
| [MySQL](mysql.md) | MySQL, MariaDB |
| [SQLite](sqlite.md) | SQLite |
| [SQL Server](sqlserver.md) | SQL Server, Azure SQL |
| [Oracle](oracle.md) | Oracle Database |
| [ClickHouse](clickhouse.md) | ClickHouse |
| [DuckDB](duckdb.md) | DuckDB, and CSV, JSON, Parquet and Excel files |
| [libSQL](libsql.md) | libSQL (`sqld`), Turso |
| [MongoDB](mongodb.md) | MongoDB |
| [Redis](redis.md) | Redis |
| [Elasticsearch](elasticsearch.md) | Elasticsearch, OpenSearch |
| [Cassandra](cassandra.md) | Apache Cassandra, ScyllaDB |
| [ADBC](adbc.md) | Snowflake, BigQuery, Databricks, Trino, any other ADBC driver |
| [Db2 and ODBC](db2-odbc.md) | IBM Db2, any ODBC driver or data source (Windows) |

## What works where

"Rows" is editing in the grid. "Admin" is the server tabs of *Server activity…* (sessions, locks,
dashboard, costliest statements, tables and indexes, parameters, users and roles, maintenance).
A dash is "not available"; "untested" means the app does not block it but nothing runs it.

| Database | Default port | Manual commit | Cancel | Explain | Rows | Admin |
|---|---|---|---|---|---|---|
| PostgreSQL | 5432 | yes | yes | plan, analyze | yes | all |
| CockroachDB, YugabyteDB | 26257, 5433 | yes | yes | plan, analyze | yes | – |
| MySQL, MariaDB | 3306 | yes | yes | plan; analyze on MySQL 8.0.18+ and MariaDB | yes | all |
| SQL Server | 1433 | yes | yes | estimated plan | yes | all; ends sessions, cannot cancel another's statement |
| Oracle | 1521 | yes | yes | estimated plan | yes | – |
| ClickHouse | 8123 (8443 with TLS) | – | yes | plan | – | parameters, dashboard, users |
| SQLite | a file | yes | yes | query plan | yes | parameters |
| DuckDB | a file | yes | yes | plan, analyze | yes | parameters |
| libSQL, Turso | 443 (80 without TLS) | yes | – | query plan | yes | – |
| MongoDB | 27017 | – | yes | – | – | – |
| Redis | 6379 | `MULTI`/`EXEC` only | yes | – | yes | – |
| Elasticsearch, OpenSearch | 9200 | – | yes | – | yes | – |
| Cassandra, ScyllaDB | 9042 | – | yes | – | yes, by full primary key | – |
| Snowflake, BigQuery, Databricks, Trino | see the page | – | yes | – | – | – |
| Db2, other ODBC | 50000 for Db2 | yes | yes | – | untested | – |

"Admin" on CockroachDB and YugabyteDB is off because those servers lack parts of what the tabs call
(CockroachDB has no `pg_blocking_pids` or `pg_terminate_backend`, YugabyteDB keeps its locks
elsewhere). The databases with no SQL (MongoDB, Redis, Elasticsearch, Cassandra) also have no
table editor, ER diagram, schema comparison, query builder, SQL import, snapshots before changes
or query plans; their pages say what they have instead.

Every database gets the editor, the grid and the navigator. Rows in the grid are found by
the table's primary key, a unique key on non-null columns, or a *virtual key* you set.

## Drivers downloaded on first use

PostgreSQL, MySQL, SQL Server, SQLite, libSQL, ClickHouse, MongoDB, Redis, Elasticsearch and
Cassandra are built into the app. The others need a library that the app downloads once:

| Database | What is downloaded | From |
|---|---|---|
| DuckDB | DuckDB's library, 2.0.0 | DuckDB's release on GitHub |
| Oracle | Oracle Instant Client, Basic Light 23.26.4 (Windows, Linux; not macOS) | Oracle |
| IBM Db2 | IBM Data Server Driver for ODBC and CLI 12.1.4 (Windows) | IBM |
| Snowflake, BigQuery, Databricks, Trino | their ADBC driver | the dbc driver registry (Columnar) |

**When you pick the database** for a new connection, and the library is not on this machine, a
popup opens over the form that says what it is, its version and size, its license and where it
comes from, and that the download is checked against a SHA-256. **Download** fetches it
(Windows asks for administrator permission to register IBM's driver); **Cancel** leaves the form
usable. The offer returns at *Test connection* and at the first opening. The check reads only this
machine, so it appears offline, and the download needs the network. *Settings › Drivers* lists,
updates and removes what was downloaded.

## Versions

The oldest version each database is checked against, and what the conformance suite runs on
(`docker/compose.yml` and the `versions` profile). A server older than the minimum connects with a
warning that some features may not work, except Cassandra, ScyllaDB, Elasticsearch and OpenSearch,
which refuse it.

| Database | Minimum | The suite runs on |
|---|---|---|
| PostgreSQL | 12 | 12, 14, 16, 17 |
| MySQL | 5.7 | 5.7, 8.0, 8.4 |
| MariaDB | 10.6 | 10.6, 11.4 |
| SQL Server | 2017 | 2017, 2022 |
| MongoDB | 5.0 | 5.0, 8.0 |
| Redis | 6.0 | Redis Stack 7.4 |
| Elasticsearch, OpenSearch | 7.10, 1.0 | 8.15.3, 2.17.1 |
| Cassandra, ScyllaDB | 4.0, 5.4 | 5, 2025.1 |
| ClickHouse | none | 26.3 |
| TimescaleDB, CockroachDB, YugabyteDB | none | 2.30 (on PostgreSQL 17), 26.2, 2025.2 |
| libSQL (`sqld`) | none | 0.24.33 |
| Oracle | none | Free 23.26 |
| Trino | none | 484 |
| SQLite, DuckDB | none | the version built into the app, DuckDB 2.0 |

Not tested at all: Azure SQL, Supabase, Neon, AlloyDB, Turso's cloud, Snowflake, BigQuery,
Databricks and IBM Db2.

## Agents

[Agents and the CLI](../../AGENTS.md) works on every database above. The differences are on each
page: how a read-only session is made (the server refuses writes on PostgreSQL, CockroachDB,
YugabyteDB, MySQL, MariaDB, SQLite, DuckDB and ClickHouse; everywhere else the statement check
alone), and what is refused (`explain` on MongoDB, Redis, Elasticsearch and Cassandra;
`get_er_diagram` on MongoDB, Redis and Elasticsearch; `propose_edits` wherever the grid does not
edit rows by key: MongoDB, Redis, Elasticsearch, Cassandra, ClickHouse and the ADBC databases).
`server_activity` and `top_queries` work on PostgreSQL, MySQL,
MariaDB and SQL Server.
