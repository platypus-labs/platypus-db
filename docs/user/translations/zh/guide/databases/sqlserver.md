<!-- source: 3830a0c639a7 -->
[← 数据库](README.md)

# SQL Server 和 Azure SQL

驱动自己实现了 TDS（打过补丁的 `tiberius`），所以无需安装或下载任何东西。它面向 SQL Server 2017（14.0）及更高版本以及 Azure SQL 编写（没有针对该服务测试过：见本页末尾）。

## 连接

| 你粘贴的内容 | 解读为 |
|---|---|
| `sqlserver://sa:pass@host:1433/master?encrypt=true&trustServerCertificate=true`（或 `mssql://`） | URL 形式；默认端口 1433 |
| `jdbc:sqlserver://host:1433;databaseName=shop;encrypt=true` | JDBC URL（微软的驱动、jTDS 或旧的 `jdbc:microsoft:`） |
| `Server=tcp:db.example.com,1433;Database=shop;User Id=sa;Password=…;Encrypt=True` | ADO.NET 或 ODBC 连接字符串 |

在 URL 中，`database` 或 `databaseName` 指定数据库，`encrypt` 和 `trustServerCertificate` 按 JDBC 和 ADO.NET 字符串的方式解读（除非同时给出了 `sslmode`）。

**命名实例**（`host\SQLEXPRESS`）不会通过 SQL Server Browser 查找：请给出实例的端口。粘贴的字符串中如果有实例而没有端口，会提示这一点。

**TLS**：`disable` 关闭加密。`prefer`（默认）加密但不检查证书（服务器经常使用自签名证书），如果服务器没有加密，则回退为明文。`require` 加密而不检查。`verify-ca` 和 `verify-full` 都会检查证书链**和**主机名，因为 TLS 库总是检查名称；请让 `sslrootcert` 指向你的 CA 文件，否则使用系统信任的证书。

**登录**：

- *SQL Server login*：用户和密码。
- *Windows 登录（SSPI、Kerberos）*（粘贴字符串中的 `Integrated Security=SSPI`）：仅当 PlatypusDB 在 Windows 上运行时可用；在其他系统上是错误。
- *Microsoft Entra ID*：由 `az` 工具生成的令牌，该工具必须已安装并登录。Azure SQL 的 Entra 登录就是这样实现的。

**Azure SQL**：网关可能会把客户端转到数据库自己的节点；应用会跟随一次这样的重定向，但通过 SSH 隧道时除外，因为隧道只通向一个地址。

## 权限

浏览只需要该登录能看到的内容。管理标签页读取：

- **会话**和**锁**：`sys.dm_exec_sessions`、`dm_exec_requests`、`dm_exec_connections` 和 `dm_tran_locks`。要看到其他登录在做什么，该登录需要 `VIEW SERVER STATE`。
- **最耗时的语句**：`sys.dm_exec_query_stats`，即计划缓存。
- **表和索引**：`sys.tables`、`dm_db_partition_stats`、`dm_db_index_usage_stats` 和 `sys.stats`。
- **参数**：`sys.configurations`；*在编辑器中打开*会写出一条 `sp_configure` 语句。
- **仪表板**：性能计数器、`sys.databases` 和 `sys.master_files` 的大小，以及（如果有）可用性组；该登录无法读取的数字显示“—”。
- **用户和角色**：服务器的登录名、角色和授权。
- **维护**：`UPDATE STATISTICS`、`ALTER INDEX ALL … REBUILD` 和 `DBCC CHECKTABLE`。

## 可用功能

- 在表格中按主键（或非空列上的唯一键，或虚拟键）编辑数据。
- 手动提交和回滚；取消正在运行的语句（TDS attention）会保留会话、其事务及其临时表。
- 一个批处理返回多个结果集，以及 `PRINT` 消息。
- 每个连接可有多个数据库：指定了数据库的连接只显示那一个；*显示所有数据库*会列出其余的。
- 执行计划显示的是**预估**计划（`SHOWPLAN`）。分析执行计划会被拒绝。
- *结束会话*是 `KILL`。SQL Server 上无法取消另一个会话的语句，所以标签页只提供结束会话。
- 导入使用 SQL Server 的批量复制（对含有 `money`、`xml` 或旧版列的表使用 `OPENJSON` 插入）。
- 模式工具（表编辑器、ER 图、比较），也适用于计算列和带排序规则的列、丰富的索引以及时态表。XML 索引和空间索引不包含在比较中。
- 终端模式运行 `sqlcmd`（如果它在 PATH 中）。

## 代理

在 SQL Server 上，只读代理只靠语句检查来阻止写入（没有服务器端的只读会话）；代理的事务会被回滚。`server_activity` 和 `top_queries` 可用。见[代理和 CLI](../../../../AGENTS.md)。

## 已知限制

- 低于 2017：应用会连接，并警告某些功能可能无法工作。
- 没有分析执行计划，也不查找命名实例。

## 测试环境

一致性测试套件在 SQL Server 2017 和 2022（容器）上运行，通用 ODBC 驱动也在同一个 2022 容器上试过。Azure SQL 没有测试：它的重定向、Entra 登录和加密规则在代码中都有，但没有针对该服务运行过。

另见：[连接](../connect.md)、[故障排除](../troubleshooting.md)。
