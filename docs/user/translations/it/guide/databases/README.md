<!-- source: 4ddccaa8076a -->
[← Guida](../README.md)

# Database

Una pagina per ogni database: come connettersi, cosa serve al login, cosa funziona e cosa no, e su
quali versioni gira la suite di test. [Connettersi](../connect.md) spiega le parti comuni a tutti i
database (SSH, proxy, il portachiavi, i progetti), [Risoluzione dei problemi](../troubleshooting.md)
gli errori, e [FEATURES.md](../../../../FEATURES.md) è il riferimento per funzionalità.

| Pagina | Database |
|---|---|
| [PostgreSQL](postgresql.md) | PostgreSQL, CockroachDB, YugabyteDB, TimescaleDB, Supabase, Neon, AlloyDB |
| [MySQL](mysql.md) | MySQL, MariaDB |
| [SQLite](sqlite.md) | SQLite |
| [SQL Server](sqlserver.md) | SQL Server, Azure SQL |
| [Oracle](oracle.md) | Oracle Database |
| [ClickHouse](clickhouse.md) | ClickHouse |
| [DuckDB](duckdb.md) | DuckDB, e file CSV, JSON, Parquet ed Excel |
| [libSQL](libsql.md) | libSQL (`sqld`), Turso |
| [MongoDB](mongodb.md) | MongoDB |
| [Redis](redis.md) | Redis |
| [Elasticsearch](elasticsearch.md) | Elasticsearch, OpenSearch |
| [Cassandra](cassandra.md) | Apache Cassandra, ScyllaDB |
| [ADBC](adbc.md) | Snowflake, BigQuery, Databricks, Trino, qualsiasi altro driver ADBC |
| [Db2 e ODBC](db2-odbc.md) | IBM Db2, qualsiasi driver o origine dati ODBC (Windows) |

Azure SQL, Supabase, Neon, AlloyDB, il cloud di Turso, Snowflake, BigQuery, Databricks e IBM Db2
sono elencati perché l'app ha per loro un driver, un preset o un riquadro; nessuno ha provato l'app
su quei servizi (vedi "Testato su" qui sotto e in ogni pagina).

## Cosa funziona dove

"Righe" è la modifica nella griglia. "Admin" sono le schede del server di *Attività del server…*
(sessioni, lock, dashboard, istruzioni più costose, tabelle e indici, parametri, utenti e ruoli,
manutenzione). Un trattino vuol dire "non disponibile"; "non provato" vuol dire che l'app non lo
blocca ma nessuno lo esegue.

| Database | Porta predefinita | Commit manuale | Annulla | Explain | Righe | Admin |
|---|---|---|---|---|---|---|
| PostgreSQL | 5432 | sì | sì | piano, analyze | sì | tutto |
| CockroachDB, YugabyteDB | 26257, 5433 | sì | sì | piano, analyze | sì | – |
| MySQL, MariaDB | 3306 | sì | sì | piano; analyze da MySQL 8.0.18 e su MariaDB | sì | tutto |
| SQL Server | 1433 | sì | sì | piano stimato | sì | tutto; chiude le sessioni, non può annullare l'istruzione di un altro |
| Oracle | 1521 | sì | sì | piano stimato | sì | – |
| ClickHouse | 8123 (8443 con TLS) | – | sì | piano | – | parametri, dashboard, utenti |
| SQLite | un file | sì | sì | piano della query | sì | parametri |
| DuckDB | un file | sì | sì | piano, analyze | sì | parametri |
| libSQL, Turso | 443 (80 senza TLS) | sì | – | piano della query | sì | – |
| MongoDB | 27017 | – | sì | – | – | – |
| Redis | 6379 | solo `MULTI`/`EXEC` | sì | – | sì | – |
| Elasticsearch, OpenSearch | 9200 | – | sì | – | sì | – |
| Cassandra, ScyllaDB | 9042 | – | sì | – | sì, per chiave primaria completa | – |
| Snowflake, BigQuery, Databricks, Trino | vedi la pagina | – | sì | – | – | – |
| Db2, altri ODBC | 50000 per Db2 | sì | sì | – | non provato | – |

"Admin" è disattivato su CockroachDB e YugabyteDB perché quei server non hanno alcune parti che le
schede richiamano (CockroachDB non ha `pg_blocking_pids` né `pg_terminate_backend`, YugabyteDB tiene
i suoi lock altrove). I database senza SQL (MongoDB, Redis, Elasticsearch, Cassandra) non hanno
nemmeno l'editor di tabelle, il diagramma ER, il confronto di schemi, il query builder,
l'importazione SQL, gli snapshot prima delle modifiche o i piani delle query; le loro pagine dicono
cosa hanno al loro posto.

Ogni database ha l'editor, la griglia e il navigatore. Le righe nella griglia si trovano con la
chiave primaria della tabella, con una chiave univoca su colonne non nulle, o con una *chiave
virtuale* che imposti tu.

## Driver scaricati al primo utilizzo

PostgreSQL, MySQL, SQL Server, SQLite, libSQL, ClickHouse, MongoDB, Redis, Elasticsearch e
Cassandra sono integrati nell'app. Gli altri hanno bisogno di una libreria che l'app scarica una
volta sola:

| Database | Cosa viene scaricato | Da dove |
|---|---|---|
| DuckDB | la libreria di DuckDB, 2.0.0 | la release di DuckDB su GitHub |
| Oracle | Oracle Instant Client, Basic Light 23.26.4 (Windows, Linux; non macOS) | Oracle |
| IBM Db2 | IBM Data Server Driver for ODBC and CLI 12.1.4 (Windows) | IBM |
| Snowflake, BigQuery, Databricks, Trino | il loro driver ADBC | il registro dei driver dbc (Columnar) |

**Quando scegli il database** per una nuova connessione, e la libreria non è su questo computer, si
apre sopra il modulo un popup che dice cos'è, la sua versione e dimensione, la sua licenza e da dove
viene, e che il download viene verificato con uno SHA-256. **Scarica** la scarica (Windows chiede il
permesso di amministratore per registrare il driver di IBM); **Annulla** lascia il modulo
utilizzabile. L'offerta ritorna a *Prova la connessione* e alla prima apertura. Il controllo legge
solo questo computer, quindi compare anche offline, mentre il download ha bisogno della rete.
*Impostazioni › Driver* elenca, aggiorna e rimuove ciò che è stato scaricato.

## Versioni

La versione più vecchia con cui si controlla ogni database, e su cosa gira la suite di conformance
(`docker/compose.yml` e il profilo `versions`). Un server più vecchio del minimo si connette con un
avviso che alcune funzioni potrebbero non funzionare, tranne Cassandra, ScyllaDB, Elasticsearch e
OpenSearch, che lo rifiutano.

| Database | Minimo | La suite gira su |
|---|---|---|
| PostgreSQL | 12 | 12, 14, 16, 17 |
| MySQL | 5.7 | 5.7, 8.0, 8.4 |
| MariaDB | 10.6 | 10.6, 11.4 |
| SQL Server | 2017 | 2017, 2022 |
| MongoDB | 5.0 | 5.0, 8.0 |
| Redis | 6.0 | Redis Stack 7.4 |
| Elasticsearch, OpenSearch | 7.10, 1.0 | 8.15.3, 2.17.1 |
| Cassandra, ScyllaDB | 4.0, 5.4 | 5, 2025.1 |
| ClickHouse | nessuno | 26.3 |
| TimescaleDB, CockroachDB, YugabyteDB | nessuno | 2.30 (su PostgreSQL 17), 26.2, 2025.2 |
| libSQL (`sqld`) | nessuno | 0.24.33 |
| Oracle | nessuno | Free 23.26 |
| Trino | nessuno | 484 |
| SQLite, DuckDB | nessuno | la versione integrata nell'app, DuckDB 2.0 |

Per niente testati: Azure SQL, Supabase, Neon, AlloyDB, il cloud di Turso, Snowflake, BigQuery,
Databricks e IBM Db2.

## Agenti

[Agenti e la CLI](../../../../AGENTS.md) funziona su tutti i database qui sopra. Le differenze sono in
ogni pagina: come si crea una sessione di sola lettura (il server rifiuta le scritture su
PostgreSQL, CockroachDB, YugabyteDB, MySQL, MariaDB, SQLite, DuckDB e ClickHouse; ovunque altrove
c'è solo il controllo dell'istruzione), e cosa viene rifiutato (`explain` su MongoDB, Redis,
Elasticsearch e Cassandra; `get_er_diagram` su MongoDB, Redis ed Elasticsearch; `propose_edits`
dovunque la griglia non modifichi le righe per chiave: Redis, Elasticsearch, Cassandra, ClickHouse e
i database ADBC; su MongoDB nomina i documenti per `_id`).
`server_activity` e `top_queries` funzionano su PostgreSQL, MySQL, MariaDB e SQL Server.
