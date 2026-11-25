<!-- source: 4ddccaa8076a -->
[← Guide](../README.md)

# Bases de données

Une page par base de données : comment s'y connecter, ce que demande la connexion, ce qui fonctionne et ce
qui ne fonctionne pas, et sur quelles versions tourne la suite de tests. [Se connecter](../connect.md)
explique ce que toutes les bases de données ont en commun (SSH, proxy, trousseau, projets),
[Dépannage](../troubleshooting.md) les erreurs, et [FEATURES.md](../../../../FEATURES.md) est la référence par
fonctionnalité.

| Page | Bases de données |
|---|---|
| [PostgreSQL](postgresql.md) | PostgreSQL, CockroachDB, YugabyteDB, TimescaleDB, Supabase, Neon, AlloyDB |
| [MySQL](mysql.md) | MySQL, MariaDB |
| [SQLite](sqlite.md) | SQLite |
| [SQL Server](sqlserver.md) | SQL Server, Azure SQL |
| [Oracle](oracle.md) | Oracle Database |
| [ClickHouse](clickhouse.md) | ClickHouse |
| [DuckDB](duckdb.md) | DuckDB, et les fichiers CSV, JSON, Parquet et Excel |
| [libSQL](libsql.md) | libSQL (`sqld`), Turso |
| [MongoDB](mongodb.md) | MongoDB |
| [Redis](redis.md) | Redis |
| [Elasticsearch](elasticsearch.md) | Elasticsearch, OpenSearch |
| [Cassandra](cassandra.md) | Apache Cassandra, ScyllaDB |
| [ADBC](adbc.md) | Snowflake, BigQuery, Databricks, Trino, tout autre pilote ADBC |
| [Db2 et ODBC](db2-odbc.md) | IBM Db2, tout pilote ou source de données ODBC (Windows) |

Azure SQL, Supabase, Neon, AlloyDB, le cloud de Turso, Snowflake, BigQuery, Databricks et IBM Db2 sont
listés parce que l'application a pour eux un pilote, un préréglage ou une tuile ; personne n'a essayé
l'application sur ces services (voir « Testé sur » plus bas et sur chaque page).

## Ce qui fonctionne où

« Lignes » désigne la modification dans la grille. « Admin » désigne les onglets serveur de *Activité du
serveur…* (sessions, verrous, tableau de bord, instructions les plus coûteuses, tables et index, paramètres,
utilisateurs et rôles, maintenance). Un tiret signifie « non disponible » ; « non testé » signifie que
l'application ne le bloque pas mais que rien ne l'exécute.

| Base de données | Port par défaut | Commit manuel | Annulation | Explain | Lignes | Admin |
|---|---|---|---|---|---|---|
| PostgreSQL | 5432 | oui | oui | plan, analyze | oui | tout |
| CockroachDB, YugabyteDB | 26257, 5433 | oui | oui | plan, analyze | oui | – |
| MySQL, MariaDB | 3306 | oui | oui | plan ; analyze sur MySQL 8.0.18+ et MariaDB | oui | tout |
| SQL Server | 1433 | oui | oui | plan estimé | oui | tout ; termine des sessions, ne peut pas annuler l'instruction d'une autre session |
| Oracle | 1521 | oui | oui | plan estimé | oui | – |
| ClickHouse | 8123 (8443 avec TLS) | – | oui | plan | – | paramètres, tableau de bord, utilisateurs |
| SQLite | un fichier | oui | oui | plan de requête | oui | paramètres |
| DuckDB | un fichier | oui | oui | plan, analyze | oui | paramètres |
| libSQL, Turso | 443 (80 sans TLS) | oui | – | plan de requête | oui | – |
| MongoDB | 27017 | – | oui | – | – | – |
| Redis | 6379 | `MULTI`/`EXEC` seulement | oui | – | oui | – |
| Elasticsearch, OpenSearch | 9200 | – | oui | – | oui | – |
| Cassandra, ScyllaDB | 9042 | – | oui | – | oui, par clé primaire complète | – |
| Snowflake, BigQuery, Databricks, Trino | voir la page | – | oui | – | – | – |
| Db2, autres ODBC | 50000 pour Db2 | oui | oui | – | non testé | – |

« Admin » est désactivé sur CockroachDB et YugabyteDB parce que ces serveurs n'ont pas une partie de ce que
les onglets appellent (CockroachDB n'a ni `pg_blocking_pids` ni `pg_terminate_backend`, YugabyteDB garde ses
verrous ailleurs). Les bases de données sans SQL (MongoDB, Redis, Elasticsearch, Cassandra) n'ont pas non plus
d'éditeur de tables, de diagramme ER, de comparaison de schémas, de constructeur de requêtes, d'import SQL,
d'instantanés avant modifications ni de plans de requête ; leurs pages disent ce qu'elles ont à la place.

Toutes les bases de données ont l'éditeur, la grille et le navigateur. Les lignes de la grille sont
retrouvées par la clé primaire de la table, par une clé unique sur des colonnes non nulles, ou par une *clé
virtuelle* que vous définissez.

## Pilotes téléchargés à la première utilisation

PostgreSQL, MySQL, SQL Server, SQLite, libSQL, ClickHouse, MongoDB, Redis, Elasticsearch et Cassandra sont
intégrés à l'application. Les autres demandent une bibliothèque que l'application télécharge une seule fois :

| Base de données | Ce qui est téléchargé | D'où |
|---|---|---|
| DuckDB | La bibliothèque de DuckDB, 2.0.0 | La version de DuckDB sur GitHub |
| Oracle | Oracle Instant Client, Basic Light 23.26.4 (Windows, Linux ; pas macOS) | Oracle |
| IBM Db2 | IBM Data Server Driver for ODBC and CLI 12.1.4 (Windows) | IBM |
| Snowflake, BigQuery, Databricks, Trino | leur pilote ADBC | le registre de pilotes dbc (Columnar) |

**Quand vous choisissez la base de données** pour une nouvelle connexion, et que la bibliothèque n'est pas sur
cette machine, une fenêtre s'ouvre au-dessus du formulaire pour dire de quoi il s'agit, sa version et sa
taille, sa licence et d'où elle vient, et que le téléchargement est vérifié par un SHA-256. **Télécharger**
la récupère (Windows demande la permission d'administrateur pour enregistrer le pilote d'IBM) ; **Annuler**
laisse le formulaire utilisable. La proposition revient à *Tester la connexion* et à la première ouverture.
La vérification ne lit que cette machine, donc elle apparaît même hors ligne, alors que le téléchargement
demande le réseau. *Paramètres › Pilotes* liste, met à jour et supprime ce qui a été téléchargé.

## Versions

La plus ancienne version contre laquelle chaque base de données est vérifiée, et ce sur quoi tourne la suite
de conformité (`docker/compose.yml` et le profil `versions`). Un serveur plus ancien que le minimum se connecte
avec un avertissement disant que certaines fonctionnalités peuvent ne pas marcher, sauf Cassandra, ScyllaDB,
Elasticsearch et OpenSearch, qui le refusent.

| Base de données | Minimum | La suite tourne sur |
|---|---|---|
| PostgreSQL | 12 | 12, 14, 16, 17 |
| MySQL | 5.7 | 5.7, 8.0, 8.4 |
| MariaDB | 10.6 | 10.6, 11.4 |
| SQL Server | 2017 | 2017, 2022 |
| MongoDB | 5.0 | 5.0, 8.0 |
| Redis | 6.0 | Redis Stack 7.4 |
| Elasticsearch, OpenSearch | 7.10, 1.0 | 8.15.3, 2.17.1 |
| Cassandra, ScyllaDB | 4.0, 5.4 | 5, 2025.1 |
| ClickHouse | aucun | 26.3 |
| TimescaleDB, CockroachDB, YugabyteDB | aucun | 2.30 (sur PostgreSQL 17), 26.2, 2025.2 |
| libSQL (`sqld`) | aucun | 0.24.33 |
| Oracle | aucun | Free 23.26 |
| Trino | aucun | 484 |
| SQLite, DuckDB | aucun | la version intégrée à l'application, DuckDB 2.0 |

Pas testés du tout : Azure SQL, Supabase, Neon, AlloyDB, le cloud de Turso, Snowflake, BigQuery,
Databricks et IBM Db2.

## Agents

[Agents et la CLI](../../../../AGENTS.md) fonctionne sur toutes les bases de données ci-dessus. Les différences
sont sur chaque page : comment une session en lecture seule est créée (le serveur refuse les écritures sur
PostgreSQL, CockroachDB, YugabyteDB, MySQL, MariaDB, SQLite, DuckDB et ClickHouse ; partout ailleurs, seule
la vérification de l'instruction s'applique), et ce qui est refusé (`explain` sur MongoDB, Redis,
Elasticsearch et Cassandra ; `get_er_diagram` sur MongoDB, Redis et Elasticsearch ; `propose_edits` partout où
la grille ne modifie pas les lignes par clé : Redis, Elasticsearch, Cassandra, ClickHouse et les bases de
données ADBC ; sur MongoDB, il désigne les documents par `_id`).
`server_activity` et `top_queries` fonctionnent sur PostgreSQL, MySQL,
MariaDB et SQL Server.
