<!-- source: 4ddccaa8076a -->
[← Guia](../README.md)

# Bancos de dados

Uma página para cada banco de dados: como conectar, o que o login exige, o que funciona e o que não
funciona, e em quais versões a suíte de testes roda. [Conectar](../connect.md) explica as partes que
todo banco de dados compartilha (SSH, proxy, o chaveiro, projetos),
[Solução de problemas](../troubleshooting.md) os erros, e [FEATURES.md](../../../../FEATURES.md) é a
referência por recurso.

| Página | Bancos de dados |
|---|---|
| [PostgreSQL](postgresql.md) | PostgreSQL, CockroachDB, YugabyteDB, TimescaleDB, Supabase, Neon, AlloyDB |
| [MySQL](mysql.md) | MySQL, MariaDB |
| [SQLite](sqlite.md) | SQLite |
| [SQL Server](sqlserver.md) | SQL Server, Azure SQL |
| [Oracle](oracle.md) | Oracle Database |
| [ClickHouse](clickhouse.md) | ClickHouse |
| [DuckDB](duckdb.md) | DuckDB, e arquivos CSV, JSON, Parquet e Excel |
| [libSQL](libsql.md) | libSQL (`sqld`), Turso |
| [MongoDB](mongodb.md) | MongoDB |
| [Redis](redis.md) | Redis |
| [Elasticsearch](elasticsearch.md) | Elasticsearch, OpenSearch |
| [Cassandra](cassandra.md) | Apache Cassandra, ScyllaDB |
| [ADBC](adbc.md) | Snowflake, BigQuery, Databricks, Trino, qualquer outro driver ADBC |
| [Db2 e ODBC](db2-odbc.md) | IBM Db2, qualquer driver ou fonte de dados ODBC (Windows) |

Azure SQL, Supabase, Neon, AlloyDB, a nuvem do Turso, Snowflake, BigQuery, Databricks e IBM Db2
estão listados porque o app tem um driver, um preset ou um tile para eles; ninguém executou o app
contra esses serviços (veja "Testado em" abaixo e em cada página).

## O que funciona onde

"Linhas" é a edição na grade. "Admin" são as abas do servidor de *Atividade do servidor…* (sessões,
locks, painel, instruções mais custosas, tabelas e índices, parâmetros, usuários e papéis,
manutenção). Um traço quer dizer "não disponível"; "não testado" quer dizer que o app não bloqueia,
mas nada o executa.

| Banco de dados | Porta padrão | Commit manual | Cancelar | Explain | Linhas | Admin |
|---|---|---|---|---|---|---|
| PostgreSQL | 5432 | sim | sim | plano, analyze | sim | tudo |
| CockroachDB, YugabyteDB | 26257, 5433 | sim | sim | plano, analyze | sim | – |
| MySQL, MariaDB | 3306 | sim | sim | plano; analyze no MySQL 8.0.18+ e no MariaDB | sim | tudo |
| SQL Server | 1433 | sim | sim | plano estimado | sim | tudo; encerra sessões, não cancela a instrução de outra sessão |
| Oracle | 1521 | sim | sim | plano estimado | sim | – |
| ClickHouse | 8123 (8443 com TLS) | – | sim | plano | – | parâmetros, painel, usuários |
| SQLite | um arquivo | sim | sim | plano de consulta | sim | parâmetros |
| DuckDB | um arquivo | sim | sim | plano, analyze | sim | parâmetros |
| libSQL, Turso | 443 (80 sem TLS) | sim | – | plano de consulta | sim | – |
| MongoDB | 27017 | – | sim | – | – | – |
| Redis | 6379 | só `MULTI`/`EXEC` | sim | – | sim | – |
| Elasticsearch, OpenSearch | 9200 | – | sim | – | sim | – |
| Cassandra, ScyllaDB | 9042 | – | sim | – | sim, pela chave primária completa | – |
| Snowflake, BigQuery, Databricks, Trino | veja a página | – | sim | – | – | – |
| Db2, outros ODBC | 50000 para o Db2 | sim | sim | – | não testado | – |

"Admin" no CockroachDB e no YugabyteDB está desativado porque esses servidores não têm partes do que
as abas chamam (o CockroachDB não tem `pg_blocking_pids` nem `pg_terminate_backend`, o YugabyteDB
guarda seus locks em outro lugar). Os bancos de dados sem SQL (MongoDB, Redis, Elasticsearch,
Cassandra) também não têm editor de tabelas, diagrama ER, comparação de schemas, query builder,
importação de SQL, snapshots antes de alterações nem planos de consulta; as páginas deles dizem o
que têm no lugar.

Todo banco de dados recebe o editor, a grade e o navegador. As linhas da grade são encontradas pela
chave primária da tabela, por uma chave única em colunas não nulas, ou por uma *chave virtual* que
você define.

## Drivers baixados no primeiro uso

PostgreSQL, MySQL, SQL Server, SQLite, libSQL, ClickHouse, MongoDB, Redis, Elasticsearch e Cassandra
são embutidos no app. Os outros precisam de uma biblioteca que o app baixa uma vez:

| Banco de dados | O que é baixado | De onde |
|---|---|---|
| DuckDB | A biblioteca do DuckDB, 2.0.0 | A release do DuckDB no GitHub |
| Oracle | Oracle Instant Client, Basic Light 23.26.4 (Windows, Linux; não macOS) | Oracle |
| IBM Db2 | IBM Data Server Driver for ODBC and CLI 12.1.4 (Windows) | IBM |
| Snowflake, BigQuery, Databricks, Trino | o driver ADBC deles | o registry de drivers dbc (Columnar) |

**Ao escolher o banco de dados** de uma nova conexão, se a biblioteca não está neste computador, um
popup se abre sobre o formulário dizendo o que ela é, a versão e o tamanho, a licença e de onde vem,
e que o download é verificado com um SHA-256. **Baixar** a obtém (o Windows pede permissão de
administrador para registrar o driver da IBM); **Cancelar** deixa o formulário utilizável. A oferta
volta em *Testar conexão* e na primeira abertura. A verificação lê só este computador, então aparece
offline, e o download precisa de rede. *Configurações › Drivers* lista, atualiza e remove o que foi
baixado.

## Versões

A versão mais antiga com que cada banco de dados é verificado, e em que a suíte de conformidade roda
(`docker/compose.yml` e o perfil `versions`). Um servidor mais antigo que o mínimo conecta com um
aviso de que alguns recursos podem não funcionar, exceto Cassandra, ScyllaDB, Elasticsearch e
OpenSearch, que o recusam.

| Banco de dados | Mínimo | A suíte roda em |
|---|---|---|
| PostgreSQL | 12 | 12, 14, 16, 17 |
| MySQL | 5.7 | 5.7, 8.0, 8.4 |
| MariaDB | 10.6 | 10.6, 11.4 |
| SQL Server | 2017 | 2017, 2022 |
| MongoDB | 5.0 | 5.0, 8.0 |
| Redis | 6.0 | Redis Stack 7.4 |
| Elasticsearch, OpenSearch | 7.10, 1.0 | 8.15.3, 2.17.1 |
| Cassandra, ScyllaDB | 4.0, 5.4 | 5, 2025.1 |
| ClickHouse | nenhum | 26.3 |
| TimescaleDB, CockroachDB, YugabyteDB | nenhum | 2.30 (no PostgreSQL 17), 26.2, 2025.2 |
| libSQL (`sqld`) | nenhum | 0.24.33 |
| Oracle | nenhum | Free 23.26 |
| Trino | nenhum | 484 |
| SQLite, DuckDB | nenhum | a versão embutida no app, DuckDB 2.0 |

Não testados de forma alguma: Azure SQL, Supabase, Neon, AlloyDB, a nuvem do Turso, Snowflake,
BigQuery, Databricks e IBM Db2.

## Agentes

[Agentes e a CLI](../../../../AGENTS.md) funciona em todos os bancos de dados acima. As diferenças estão em
cada página: como uma sessão somente leitura é feita (o servidor recusa gravações no PostgreSQL,
CockroachDB, YugabyteDB, MySQL, MariaDB, SQLite, DuckDB e ClickHouse; em todos os outros, só a
verificação da instrução), e o que é recusado (`explain` no MongoDB, Redis, Elasticsearch e
Cassandra; `get_er_diagram` no MongoDB, Redis e Elasticsearch; `propose_edits` onde quer que a grade
não edite linhas por chave: Redis, Elasticsearch, Cassandra, ClickHouse e os bancos de dados ADBC;
no MongoDB ele nomeia os documentos por `_id`).
`server_activity` e `top_queries` funcionam no PostgreSQL, MySQL, MariaDB e SQL Server.
