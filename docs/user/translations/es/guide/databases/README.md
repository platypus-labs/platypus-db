<!-- source: 4ddccaa8076a -->
[← Guía](../README.md)

# Bases de datos

Una página por cada base de datos: cómo conectar, qué necesita el inicio de sesión, qué funciona y
qué no, y en qué versiones se ejecuta la batería de pruebas. [Conectar](../connect.md) explica lo que
todas las bases de datos comparten (SSH, proxy, el llavero, los proyectos),
[Solución de problemas](../troubleshooting.md) los errores, y [FEATURES.md](../../../../FEATURES.md) es la
referencia por funciones.

| Página | Bases de datos |
|---|---|
| [PostgreSQL](postgresql.md) | PostgreSQL, CockroachDB, YugabyteDB, TimescaleDB, Supabase, Neon, AlloyDB |
| [MySQL](mysql.md) | MySQL, MariaDB |
| [SQLite](sqlite.md) | SQLite |
| [SQL Server](sqlserver.md) | SQL Server, Azure SQL |
| [Oracle](oracle.md) | Oracle Database |
| [ClickHouse](clickhouse.md) | ClickHouse |
| [DuckDB](duckdb.md) | DuckDB, y archivos CSV, JSON, Parquet y Excel |
| [libSQL](libsql.md) | libSQL (`sqld`), Turso |
| [MongoDB](mongodb.md) | MongoDB |
| [Redis](redis.md) | Redis |
| [Elasticsearch](elasticsearch.md) | Elasticsearch, OpenSearch |
| [Cassandra](cassandra.md) | Apache Cassandra, ScyllaDB |
| [ADBC](adbc.md) | Snowflake, BigQuery, Databricks, Trino, cualquier otro driver ADBC |
| [Db2 y ODBC](db2-odbc.md) | IBM Db2, cualquier driver u origen de datos ODBC (Windows) |

Azure SQL, Supabase, Neon, AlloyDB, la nube de Turso, Snowflake, BigQuery, Databricks e IBM Db2
figuran porque la app tiene un driver, un preajuste o un icono para ellos; nadie ha ejecutado la app
contra esos servicios (consulta "Probado en" más abajo y en cada página).

## Qué funciona dónde

"Filas" es la edición en la cuadrícula. "Admin" son las pestañas de servidor de *Actividad del
servidor…* (sesiones, bloqueos, panel, sentencias más costosas, tablas e índices, parámetros,
usuarios y roles, mantenimiento). Un guion significa "no disponible"; "sin probar" significa que la
app no lo bloquea pero nadie lo ejecuta.

| Base de datos | Puerto por defecto | Confirmación manual | Cancelar | Explain | Filas | Admin |
|---|---|---|---|---|---|---|
| PostgreSQL | 5432 | sí | sí | plan, analyze | sí | todo |
| CockroachDB, YugabyteDB | 26257, 5433 | sí | sí | plan, analyze | sí | – |
| MySQL, MariaDB | 3306 | sí | sí | plan; analyze en MySQL 8.0.18+ y MariaDB | sí | todo |
| SQL Server | 1433 | sí | sí | plan estimado | sí | todo; termina sesiones, no puede cancelar la sentencia de otra |
| Oracle | 1521 | sí | sí | plan estimado | sí | – |
| ClickHouse | 8123 (8443 con TLS) | – | sí | plan | – | parámetros, panel, usuarios |
| SQLite | un archivo | sí | sí | plan de consulta | sí | parámetros |
| DuckDB | un archivo | sí | sí | plan, analyze | sí | parámetros |
| libSQL, Turso | 443 (80 sin TLS) | sí | – | plan de consulta | sí | – |
| MongoDB | 27017 | – | sí | – | – | – |
| Redis | 6379 | solo `MULTI`/`EXEC` | sí | – | sí | – |
| Elasticsearch, OpenSearch | 9200 | – | sí | – | sí | – |
| Cassandra, ScyllaDB | 9042 | – | sí | – | sí, por clave primaria completa | – |
| Snowflake, BigQuery, Databricks, Trino | consulta la página | – | sí | – | – | – |
| Db2, otros ODBC | 50000 para Db2 | sí | sí | – | sin probar | – |

"Admin" está desactivado en CockroachDB y YugabyteDB porque esos servidores carecen de parte de lo
que usan las pestañas (CockroachDB no tiene `pg_blocking_pids` ni `pg_terminate_backend`, YugabyteDB
guarda sus bloqueos en otro sitio). Las bases de datos sin SQL (MongoDB, Redis, Elasticsearch,
Cassandra) tampoco tienen editor de tablas, diagrama ER, comparación de esquemas, constructor de
consultas, importación SQL, instantáneas antes de cambios ni planes de consulta; sus páginas dicen
qué tienen en su lugar.

Todas las bases de datos tienen el editor, la cuadrícula y el navegador. Las filas de la cuadrícula
se localizan por la clave primaria de la tabla, una clave única en columnas no nulas, o una *clave
virtual* que tú defines.

## Drivers que se descargan en el primer uso

PostgreSQL, MySQL, SQL Server, SQLite, libSQL, ClickHouse, MongoDB, Redis, Elasticsearch y
Cassandra vienen integrados en la app. Las demás necesitan una biblioteca que la app descarga una vez:

| Base de datos | Qué se descarga | De dónde |
|---|---|---|
| DuckDB | La biblioteca de DuckDB, 2.0.0 | La versión de DuckDB en GitHub |
| Oracle | Oracle Instant Client, Basic Light 23.26.4 (Windows, Linux; no macOS) | Oracle |
| IBM Db2 | IBM Data Server Driver for ODBC and CLI 12.1.4 (Windows) | IBM |
| Snowflake, BigQuery, Databricks, Trino | su driver ADBC | el registro de drivers dbc (Columnar) |

**Cuando eliges la base de datos** para una conexión nueva, y la biblioteca no está en este equipo,
se abre sobre el formulario una ventana que dice qué es, su versión y tamaño, su licencia y de dónde
viene, y que la descarga se verifica con un SHA-256. **Descargar** la obtiene (Windows pide permiso de
administrador para registrar el driver de IBM); **Cancelar** deja el formulario utilizable. La oferta
vuelve en *Probar la conexión* y en la primera apertura. La comprobación solo mira este equipo, así
que aparece sin conexión, y la descarga necesita red. *Ajustes › Drivers* lista, actualiza y elimina
lo descargado.

## Versiones

La versión más antigua con la que se comprueba cada base de datos, y en qué se ejecuta la batería de
pruebas de conformidad (`docker/compose.yml` y el perfil `versions`). Un servidor anterior al mínimo
conecta con un aviso de que algunas funciones pueden no funcionar, salvo Cassandra, ScyllaDB,
Elasticsearch y OpenSearch, que lo rechazan.

| Base de datos | Mínimo | La batería se ejecuta en |
|---|---|---|
| PostgreSQL | 12 | 12, 14, 16, 17 |
| MySQL | 5.7 | 5.7, 8.0, 8.4 |
| MariaDB | 10.6 | 10.6, 11.4 |
| SQL Server | 2017 | 2017, 2022 |
| MongoDB | 5.0 | 5.0, 8.0 |
| Redis | 6.0 | Redis Stack 7.4 |
| Elasticsearch, OpenSearch | 7.10, 1.0 | 8.15.3, 2.17.1 |
| Cassandra, ScyllaDB | 4.0, 5.4 | 5, 2025.1 |
| ClickHouse | ninguno | 26.3 |
| TimescaleDB, CockroachDB, YugabyteDB | ninguno | 2.30 (en PostgreSQL 17), 26.2, 2025.2 |
| libSQL (`sqld`) | ninguno | 0.24.33 |
| Oracle | ninguno | Free 23.26 |
| Trino | ninguno | 484 |
| SQLite, DuckDB | ninguno | la versión integrada en la app, DuckDB 2.0 |

Sin probar en absoluto: Azure SQL, Supabase, Neon, AlloyDB, la nube de Turso, Snowflake, BigQuery,
Databricks e IBM Db2.

## Agentes

[Agentes y la CLI](../../../../AGENTS.md) funciona con todas las bases de datos de arriba. Las diferencias
están en cada página: cómo se crea una sesión de solo lectura (el servidor rechaza las escrituras en
PostgreSQL, CockroachDB, YugabyteDB, MySQL, MariaDB, SQLite, DuckDB y ClickHouse; en todas las demás
solo cuenta la comprobación de la sentencia), y qué se rechaza (`explain` en MongoDB, Redis,
Elasticsearch y Cassandra; `get_er_diagram` en MongoDB, Redis y Elasticsearch; `propose_edits` donde
la cuadrícula no edita filas por clave: Redis, Elasticsearch, Cassandra, ClickHouse y las bases de
datos ADBC; en MongoDB nombra los documentos por `_id`).
`server_activity` y `top_queries` funcionan en PostgreSQL, MySQL, MariaDB y SQL Server.
