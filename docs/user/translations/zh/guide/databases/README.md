<!-- source: 4ddccaa8076a -->
[← 指南](../README.md)

# 数据库

每种数据库一页：如何连接、登录需要什么、哪些功能可用、哪些不可用，以及测试套件在哪些版本上运行。[连接](../connect.md)说明所有数据库共有的部分（SSH、代理、钥匙串、项目），[故障排除](../troubleshooting.md)说明各种错误，[FEATURES.md](../../FEATURES.md) 是按功能编排的参考文档。

| 页面 | 数据库 |
|---|---|
| [PostgreSQL](postgresql.md) | PostgreSQL、CockroachDB、YugabyteDB、TimescaleDB、Supabase、Neon、AlloyDB |
| [MySQL](mysql.md) | MySQL、MariaDB |
| [SQLite](sqlite.md) | SQLite |
| [SQL Server](sqlserver.md) | SQL Server、Azure SQL |
| [Oracle](oracle.md) | Oracle Database |
| [ClickHouse](clickhouse.md) | ClickHouse |
| [DuckDB](duckdb.md) | DuckDB，以及 CSV、JSON、Parquet 和 Excel 文件 |
| [libSQL](libsql.md) | libSQL（`sqld`）、Turso |
| [MongoDB](mongodb.md) | MongoDB |
| [Redis](redis.md) | Redis |
| [Elasticsearch](elasticsearch.md) | Elasticsearch、OpenSearch |
| [Cassandra](cassandra.md) | Apache Cassandra、ScyllaDB |
| [ADBC](adbc.md) | Snowflake、BigQuery、Databricks、Trino，以及任何其他 ADBC 驱动 |
| [Db2 和 ODBC](db2-odbc.md) | IBM Db2，以及任何 ODBC 驱动或数据源（Windows） |

之所以列出 Azure SQL、Supabase、Neon、AlloyDB、Turso 的云服务、Snowflake、BigQuery、Databricks 和 IBM Db2，是因为应用为它们提供了驱动、预设或图块；没有人用这些服务运行过应用（见下文和各页面中的“测试环境”）。

## 各数据库的支持情况

“行”指在表格中编辑。“管理”指*服务器活动…*的服务器标签页（会话、锁、仪表板、成本最高的语句、表和索引、参数、用户和角色、维护）。短横线表示“不可用”；“未测试”表示应用不阻止它，但没有任何测试运行过它。

| 数据库 | 默认端口 | 手动提交 | 取消 | 执行计划 | 行 | 管理 |
|---|---|---|---|---|---|---|
| PostgreSQL | 5432 | 是 | 是 | 计划、分析 | 是 | 全部 |
| CockroachDB、YugabyteDB | 26257、5433 | 是 | 是 | 计划、分析 | 是 | – |
| MySQL、MariaDB | 3306 | 是 | 是 | 计划；MySQL 8.0.18+ 和 MariaDB 支持分析 | 是 | 全部 |
| SQL Server | 1433 | 是 | 是 | 预估计划 | 是 | 全部；可结束会话，不能取消他人的语句 |
| Oracle | 1521 | 是 | 是 | 预估计划 | 是 | – |
| ClickHouse | 8123（使用 TLS 时 8443） | – | 是 | 计划 | – | 参数、仪表板、用户 |
| SQLite | 文件 | 是 | 是 | 查询计划 | 是 | 参数 |
| DuckDB | 文件 | 是 | 是 | 计划、分析 | 是 | 参数 |
| libSQL、Turso | 443（不使用 TLS 时 80） | 是 | – | 查询计划 | 是 | – |
| MongoDB | 27017 | – | 是 | – | – | – |
| Redis | 6379 | 仅 `MULTI`/`EXEC` | 是 | – | 是 | – |
| Elasticsearch、OpenSearch | 9200 | – | 是 | – | 是 | – |
| Cassandra、ScyllaDB | 9042 | – | 是 | – | 是，按完整主键 | – |
| Snowflake、BigQuery、Databricks、Trino | 见该页面 | – | 是 | – | – | – |
| Db2、其他 ODBC | Db2 为 50000 | 是 | 是 | – | 未测试 | – |

CockroachDB 和 YugabyteDB 的“管理”是关闭的，因为这些服务器缺少这些标签页所调用的部分功能（CockroachDB 没有 `pg_blocking_pids` 或 `pg_terminate_backend`，YugabyteDB 把锁放在别处）。没有 SQL 的数据库（MongoDB、Redis、Elasticsearch、Cassandra）也没有表编辑器、ER 图、模式比较、查询构建器、SQL 导入、变更前快照或查询计划；它们的页面会说明它们有什么来代替。

每种数据库都有编辑器、表格和导航器。表格中的行通过表的主键、非空列上的唯一键，或你设置的*虚拟键*来定位。

## 首次使用时下载的驱动

PostgreSQL、MySQL、SQL Server、SQLite、libSQL、ClickHouse、MongoDB、Redis、Elasticsearch 和 Cassandra 内置在应用中。其他的需要一个库，应用会下载一次：

| 数据库 | 下载的内容 | 来源 |
|---|---|---|
| DuckDB | DuckDB 的库，2.0.0 | DuckDB 在 GitHub 上的发布版 |
| Oracle | Oracle Instant Client，Basic Light 23.26.4（Windows、Linux；不含 macOS） | Oracle |
| IBM Db2 | IBM Data Server Driver for ODBC and CLI 12.1.4（Windows） | IBM |
| Snowflake、BigQuery、Databricks、Trino | 它们的 ADBC 驱动 | dbc 驱动注册表（Columnar） |

**选择数据库**创建新连接时，如果这台机器上没有该库，表单上方会弹出一个窗口，说明它是什么、它的版本和大小、它的许可证和来源，以及下载会用 SHA-256 校验。**下载**会获取它（Windows 会要求管理员权限来注册 IBM 的驱动）；**取消**则表单仍可使用。在*测试连接*时和第一次打开时会再次提示。检查只读取这台机器，所以离线时也会出现，而下载需要网络。*设置 › 驱动*会列出、更新和移除已下载的内容。

## 版本

每种数据库所检查的最低版本，以及一致性测试套件所运行的版本（`docker/compose.yml` 和 `versions` 配置）。低于最低版本的服务器在连接时会给出警告，提示某些功能可能无法工作，Cassandra、ScyllaDB、Elasticsearch 和 OpenSearch 除外，它们会拒绝连接。

| 数据库 | 最低版本 | 测试套件运行的版本 |
|---|---|---|
| PostgreSQL | 12 | 12、14、16、17 |
| MySQL | 5.7 | 5.7、8.0、8.4 |
| MariaDB | 10.6 | 10.6、11.4 |
| SQL Server | 2017 | 2017、2022 |
| MongoDB | 5.0 | 5.0、8.0 |
| Redis | 6.0 | Redis Stack 7.4 |
| Elasticsearch、OpenSearch | 7.10、1.0 | 8.15.3、2.17.1 |
| Cassandra、ScyllaDB | 4.0、5.4 | 5、2025.1 |
| ClickHouse | 无 | 26.3 |
| TimescaleDB、CockroachDB、YugabyteDB | 无 | 2.30（在 PostgreSQL 17 上）、26.2、2025.2 |
| libSQL（`sqld`） | 无 | 0.24.33 |
| Oracle | 无 | Free 23.26 |
| Trino | 无 | 484 |
| SQLite、DuckDB | 无 | 应用内置的版本，DuckDB 2.0 |

完全没有测试过的：Azure SQL、Supabase、Neon、AlloyDB、Turso 的云服务、Snowflake、BigQuery、Databricks 和 IBM Db2。

## 代理

[代理和 CLI](../../AGENTS.md)适用于上面的每一种数据库。差异见各页面：只读会话是如何建立的（在 PostgreSQL、CockroachDB、YugabyteDB、MySQL、MariaDB、SQLite、DuckDB 和 ClickHouse 上由服务器拒绝写入；其他地方只靠语句检查），以及哪些会被拒绝（MongoDB、Redis、Elasticsearch 和 Cassandra 上的 `explain`；MongoDB、Redis 和 Elasticsearch 上的 `get_er_diagram`；凡是表格不按键编辑行的地方的 `propose_edits`：Redis、Elasticsearch、Cassandra、ClickHouse 和 ADBC 数据库；在 MongoDB 上它按 `_id` 指明文档）。`server_activity` 和 `top_queries` 适用于 PostgreSQL、MySQL、MariaDB 和 SQL Server。
