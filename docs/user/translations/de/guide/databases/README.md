<!-- source: 4ddccaa8076a -->
[← Handbuch](../README.md)

# Datenbanken

Eine Seite pro Datenbank: wie du dich verbindest, was die Anmeldung braucht, was funktioniert und was
nicht, und auf welchen Versionen die Testsuite läuft. [Verbinden](../connect.md) erklärt, was alle
Datenbanken gemeinsam haben (SSH, Proxy, der Schlüsselbund, Projekte),
[Fehlerbehebung](../troubleshooting.md) die Fehler, und [FEATURES.md](../../../../FEATURES.md) ist die
Referenz nach Funktionen.

| Seite | Datenbanken |
|---|---|
| [PostgreSQL](postgresql.md) | PostgreSQL, CockroachDB, YugabyteDB, TimescaleDB, Supabase, Neon, AlloyDB |
| [MySQL](mysql.md) | MySQL, MariaDB |
| [SQLite](sqlite.md) | SQLite |
| [SQL Server](sqlserver.md) | SQL Server, Azure SQL |
| [Oracle](oracle.md) | Oracle Database |
| [ClickHouse](clickhouse.md) | ClickHouse |
| [DuckDB](duckdb.md) | DuckDB und CSV-, JSON-, Parquet- und Excel-Dateien |
| [libSQL](libsql.md) | libSQL (`sqld`), Turso |
| [MongoDB](mongodb.md) | MongoDB |
| [Redis](redis.md) | Redis |
| [Elasticsearch](elasticsearch.md) | Elasticsearch, OpenSearch |
| [Cassandra](cassandra.md) | Apache Cassandra, ScyllaDB |
| [ADBC](adbc.md) | Snowflake, BigQuery, Databricks, Trino, jeder andere ADBC-Treiber |
| [Db2 und ODBC](db2-odbc.md) | IBM Db2, jeder ODBC-Treiber oder jede Datenquelle (Windows) |

Azure SQL, Supabase, Neon, AlloyDB, die Cloud von Turso, Snowflake, BigQuery, Databricks und IBM Db2
sind aufgeführt, weil die App einen Treiber, eine Voreinstellung oder eine Kachel dafür hat; niemand hat
die App gegen diese Dienste laufen lassen (siehe „Getestet auf“ unten und auf jeder Seite).

## Was wo funktioniert

„Zeilen“ ist das Bearbeiten im Raster. „Admin“ sind die Server-Tabs von *Serveraktivität…* (Sitzungen,
Sperren, Dashboard, teuerste Anweisungen, Tabellen und Indizes, Parameter, Benutzer und Rollen,
Wartung). Ein Strich heißt „nicht verfügbar“; „ungetestet“ heißt, dass die App es nicht blockiert, aber
nichts es ausführt.

| Datenbank | Standardport | Manueller Commit | Abbrechen | Explain | Zeilen | Admin |
|---|---|---|---|---|---|---|
| PostgreSQL | 5432 | ja | ja | Plan, Analyze | ja | alle |
| CockroachDB, YugabyteDB | 26257, 5433 | ja | ja | Plan, Analyze | ja | – |
| MySQL, MariaDB | 3306 | ja | ja | Plan; Analyze ab MySQL 8.0.18 und bei MariaDB | ja | alle |
| SQL Server | 1433 | ja | ja | geschätzter Plan | ja | alle; beendet Sitzungen, kann die Anweisung eines anderen nicht abbrechen |
| Oracle | 1521 | ja | ja | geschätzter Plan | ja | – |
| ClickHouse | 8123 (8443 mit TLS) | – | ja | Plan | – | Parameter, Dashboard, Benutzer |
| SQLite | eine Datei | ja | ja | Abfrageplan | ja | Parameter |
| DuckDB | eine Datei | ja | ja | Plan, Analyze | ja | Parameter |
| libSQL, Turso | 443 (80 ohne TLS) | ja | – | Abfrageplan | ja | – |
| MongoDB | 27017 | – | ja | – | – | – |
| Redis | 6379 | nur `MULTI`/`EXEC` | ja | – | ja | – |
| Elasticsearch, OpenSearch | 9200 | – | ja | – | ja | – |
| Cassandra, ScyllaDB | 9042 | – | ja | – | ja, über den vollständigen Primärschlüssel | – |
| Snowflake, BigQuery, Databricks, Trino | siehe Seite | – | ja | – | – | – |
| Db2, andere ODBC | 50000 bei Db2 | ja | ja | – | ungetestet | – |

„Admin“ ist bei CockroachDB und YugabyteDB aus, weil diesen Servern Teile dessen fehlen, was die Tabs
aufrufen (CockroachDB hat kein `pg_blocking_pids` und kein `pg_terminate_backend`, YugabyteDB hält
seine Sperren woanders). Die Datenbanken ohne SQL (MongoDB, Redis, Elasticsearch, Cassandra) haben auch
keinen Tabelleneditor, kein ER-Diagramm, keinen Schemavergleich, keinen Query Builder, keinen SQL-Import,
keine Snapshots vor Änderungen und keine Abfragepläne; ihre Seiten sagen, was sie stattdessen haben.

Jede Datenbank bekommt den Editor, das Raster und den Navigator. Zeilen im Raster werden über den
Primärschlüssel der Tabelle gefunden, über einen eindeutigen Schlüssel auf Spalten ohne NULL oder über
einen *virtuellen Schlüssel*, den du festlegst.

## Treiber, die bei der ersten Verwendung heruntergeladen werden

PostgreSQL, MySQL, SQL Server, SQLite, libSQL, ClickHouse, MongoDB, Redis, Elasticsearch und Cassandra
sind in die App eingebaut. Die anderen brauchen eine Bibliothek, die die App einmal herunterlädt:

| Datenbank | Was heruntergeladen wird | Von |
|---|---|---|
| DuckDB | die Bibliothek von DuckDB, 2.0.0 | dem Release von DuckDB auf GitHub |
| Oracle | Oracle Instant Client, Basic Light 23.26.4 (Windows, Linux; nicht macOS) | Oracle |
| IBM Db2 | IBM Data Server Driver for ODBC and CLI 12.1.4 (Windows) | IBM |
| Snowflake, BigQuery, Databricks, Trino | ihr ADBC-Treiber | der dbc-Treiber-Registry (Columnar) |

**Wenn du die Datenbank wählst** für eine neue Verbindung und die Bibliothek nicht auf diesem Rechner
liegt, öffnet sich über dem Formular ein Popup, das sagt, was es ist, seine Version und Größe, seine
Lizenz und woher es kommt, und dass der Download gegen eine SHA-256 geprüft wird. **Herunterladen**
holt sie (Windows verlangt Administratorrechte, um den Treiber von IBM zu registrieren); **Abbrechen**
lässt das Formular benutzbar. Das Angebot kommt bei *Verbindung testen* und beim ersten Öffnen wieder.
Die Prüfung liest nur diesen Rechner, daher erscheint sie auch offline, und der Download braucht das
Netzwerk. *Einstellungen › Treiber* listet auf, aktualisiert und entfernt, was heruntergeladen wurde.

## Versionen

Die älteste Version, gegen die jede Datenbank geprüft wird, und worauf die Konformitätssuite läuft
(`docker/compose.yml` und das Profil `versions`). Ein Server, der älter als das Minimum ist, verbindet
sich mit einer Warnung, dass manche Funktionen vielleicht nicht gehen, außer Cassandra, ScyllaDB,
Elasticsearch und OpenSearch, die ihn ablehnen.

| Datenbank | Minimum | Die Suite läuft auf |
|---|---|---|
| PostgreSQL | 12 | 12, 14, 16, 17 |
| MySQL | 5.7 | 5.7, 8.0, 8.4 |
| MariaDB | 10.6 | 10.6, 11.4 |
| SQL Server | 2017 | 2017, 2022 |
| MongoDB | 5.0 | 5.0, 8.0 |
| Redis | 6.0 | Redis Stack 7.4 |
| Elasticsearch, OpenSearch | 7.10, 1.0 | 8.15.3, 2.17.1 |
| Cassandra, ScyllaDB | 4.0, 5.4 | 5, 2025.1 |
| ClickHouse | keines | 26.3 |
| TimescaleDB, CockroachDB, YugabyteDB | keines | 2.30 (auf PostgreSQL 17), 26.2, 2025.2 |
| libSQL (`sqld`) | keines | 0.24.33 |
| Oracle | keines | Free 23.26 |
| Trino | keines | 484 |
| SQLite, DuckDB | keines | die in die App eingebaute Version, DuckDB 2.0 |

Gar nicht getestet: Azure SQL, Supabase, Neon, AlloyDB, die Cloud von Turso, Snowflake, BigQuery,
Databricks und IBM Db2.

## Agenten

[Agenten und die CLI](../../../../AGENTS.md) funktioniert mit jeder der obigen Datenbanken. Die Unterschiede
stehen auf jeder Seite: wie eine schreibgeschützte Sitzung entsteht (der Server lehnt Schreibzugriffe
bei PostgreSQL, CockroachDB, YugabyteDB, MySQL, MariaDB, SQLite, DuckDB und ClickHouse ab; überall sonst
nur die Prüfung der Anweisung) und was abgelehnt wird (`explain` bei MongoDB, Redis, Elasticsearch und
Cassandra; `get_er_diagram` bei MongoDB, Redis und Elasticsearch; `propose_edits` überall, wo das Raster
keine Zeilen über einen Schlüssel bearbeitet: Redis, Elasticsearch, Cassandra, ClickHouse und die
ADBC-Datenbanken; bei MongoDB benennt es Dokumente über `_id`).
`server_activity` und `top_queries` funktionieren bei PostgreSQL, MySQL, MariaDB und SQL Server.
