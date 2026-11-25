<!-- source: 6f25d7181e09 -->
[← 数据库](README.md)

# MySQL 和 MariaDB

一个驱动（`mysql_async` 客户端）可以打开 MySQL 5.7 及更高版本和 MariaDB 10.6 及更高版本。两者在数据库网格中各有一个图块；驱动会识别连上的是哪种服务器，并在连接中显示出来（`已连接：MariaDB 11.4`）。无需下载任何东西。

## 连接

| 你粘贴的内容 | 解读为 |
|---|---|
| `mysql://user:pass@host:3306/db?sslMode=REQUIRED`（或 `mariadb://`） | URL 形式；默认端口 3306 |
| `jdbc:mysql://host:3307/db`、`jdbc:mariadb://…` | JDBC URL |
| `Server=db;Port=3306;Database=shop;Uid=app;Pwd=…` | Connector/NET 连接字符串，前提是它还带有 `Driver=`，或你是在 MySQL 表单中粘贴的 |

应用认识的 URL 参数：`sslmode`（也可写成 `ssl-mode`、`sslMode`、`ssl_mode`）、`sslrootcert`、`sslcert`、`sslkey`、`application_name` 和 `connect_timeout`。

**TLS**：`disable` 关闭它。`prefer`（默认）使用 TLS，如果服务器没有 TLS 或只提供客户端拒绝的版本（MySQL 5.7 的镜像就是这样），则回退为明文；用令牌登录时绝不回退。`prefer` 和 `require` 不检查证书。`verify-ca` 根据 `sslrootcert` 检查证书链，不检查主机名；`verify-full` 两者都检查。MySQL 自己的名称（`REQUIRED`、`VERIFY_IDENTITY`）也能识别。客户端证书需要同时提供 `sslcert` 和 `sslkey`。

**登录**使用密码，或使用由 `aws`、`gcloud` 或 `az` 工具生成的 AWS IAM、Google Cloud IAM 或 Microsoft Entra ID 令牌；令牌按服务器的要求通过 TLS 发送。Kerberos（MariaDB 的 `auth_gssapi`、MySQL 的 Kerberos 插件）目前不支持。

连接始终使用 TCP：不使用 Unix 套接字或 Windows 命名管道。

## 权限

浏览只需要该登录能看到的内容。管理标签页读取：

- **会话**：进程列表，在 MySQL 8 上来自 `performance_schema.threads`，在 MariaDB、MySQL 5.7 或关闭了 `performance_schema` 的服务器上来自 `information_schema.PROCESSLIST`。服务器显示其他用户的多少会话，是服务器的规则，应用不会检查。
- **锁**：在 MySQL 8 上是 `performance_schema.data_lock_waits`、`data_locks` 和 `metadata_locks`（查看其他会话的等待需要 `PROCESS`；没有它时列表中没有等待，并会说明），在 MariaDB 和较旧的 MySQL 上是 `information_schema` 中的 `INNODB_LOCK_WAITS`。
- **最耗时的语句**：`performance_schema.events_statements_summary_by_digest`。如果 `performance_schema` 关闭，标签页会提示启动服务器时开启它。
- **表和索引**：`information_schema`，再加上 `performance_schema`（用于了解每个索引被读取了多少）；没有它时，索引的使用情况未知。
- **取消语句**和**结束会话**运行 `KILL QUERY` 和 `KILL CONNECTION`。
- **参数**和**仪表板**：服务器变量和状态计数器。
- **用户和角色**：服务器的账户和授权。
- **维护**：`ANALYZE`、`OPTIMIZE` 和 `CHECK TABLE`。
- **备份…和恢复…**运行 `mysqldump` 和 `mysql`，它们必须在 PATH 中。

## 可用功能

- 在表格中按主键（或非空列上的唯一键，或虚拟键）编辑数据。
- 手动提交和回滚，以及在服务器上取消语句。
- 执行计划：MySQL 8.0.16 及更高版本给出 `EXPLAIN FORMAT=TREE`；较旧的 MySQL 和 MariaDB 给出 `EXPLAIN FORMAT=JSON`。**分析执行计划**在 MySQL 8.0.18 及更高版本上是 `EXPLAIN ANALYZE`，在 MariaDB 上是 `ANALYZE FORMAT=JSON`；MySQL 5.7 以及 8.0.0 到 8.0.17 拒绝它。
- 上面的每个管理标签页，以及模式工具（表编辑器、ER 图、比较）。
- 服务器允许时（`local_infile` 开启），导入使用 `LOAD DATA LOCAL`，否则使用 `INSERT` 语句。
- 错误会指出它们来自哪个服务器，例如 `Unknown column 'x' in 'field list' (MySQL 5.7.44)`。

## 代理

只读代理和只读连接运行 `SET SESSION TRANSACTION READ ONLY`，它们的查询在只读事务中运行。`server_activity` 和 `top_queries` 可用。一次代理调用设置的 MySQL 或 MariaDB `@variable`、临时表或 `SET`，在下一次调用中仍然存在。见[代理和 CLI](../../AGENTS.md)。

## 已知限制

- 低于 MySQL 5.7 或 MariaDB 10.6：它会连接，并警告某些功能可能无法工作。
- 表编辑器使用 MySQL 每条语句即提交的 DDL 保存：失败的脚本可能会留下一部分已应用的更改。
- 终端模式需要 `mysql` 在 PATH 中。

## 测试环境

一致性测试套件在 MySQL 5.7、8.0 和 8.4 以及 MariaDB 10.6 和 11.4 上运行，在 MySQL 8.4 上使用双向 TLS 的客户端证书，并测试了服务器断开的会话。

另见：[连接](../connect.md)、[故障排除](../troubleshooting.md)。
