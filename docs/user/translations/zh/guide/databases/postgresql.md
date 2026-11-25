<!-- source: a2b0bfe9784d -->
[← 数据库](README.md)

# PostgreSQL、CockroachDB、YugabyteDB、TimescaleDB 及其他

一个基于 `tokio-postgres` 的驱动，使用 PostgreSQL 协议。它可以打开 PostgreSQL 12 及更高版本，以及使用同一协议的服务器和服务。无需下载任何东西：驱动是应用的一部分。

## 连接

在数据库网格中选择 **PostgreSQL**，或粘贴 URL：表单会自动填好。

| 你粘贴的内容 | 解读为 |
|---|---|
| `postgres://user:pass@host:5432/db?sslmode=require`（或 `postgresql://`） | URL 形式；默认端口是 5432 |
| `jdbc:postgresql://host/db?sslmode=require` | JDBC URL，DBeaver 和 DataGrip 就是这样保存的 |
| `host=db port=5432 dbname=shop user=app sslmode=require` | libpq 连接字符串 |
| `Host=db;Database=shop;Username=app;Password=…` | Npgsql 连接字符串 |

应用认识的 URL 参数：`sslmode`、`sslrootcert`、`sslcert`、`sslkey`、`application_name`、`connect_timeout` 和 `pgbouncer=true`。其他任何参数都会被忽略，表单会说明是哪些。故障转移主机列表（`h1:5432,h2:5432`）只保留第一个主机。

**TLS** 遵循 libpq 的 `sslmode`：

| 模式 | 效果 |
|---|---|
| `disable` | 不使用 TLS |
| `prefer`（默认） | 服务器提供时使用 TLS，否则使用明文；不检查证书 |
| `require` | 使用 TLS，不检查证书 |
| `verify-ca` | 使用 TLS，检查证书链，不检查主机名 |
| `verify-full` | 使用 TLS，检查证书链和主机名 |

如果有 `sslrootcert` 中的文件，就用它来检查证书链，否则用系统信任的证书。客户端证书（`sslcert` 和 `sslkey`）在每种使用 TLS 的模式下都会发送。

**登录**使用密码，或使用云命令行工具在每次连接时生成的令牌：AWS IAM（`aws`）、Google Cloud IAM（`gcloud`）和 Microsoft Entra ID（`az`）在连接的*认证方式*选项中。该工具必须已在你的机器上安装并登录。或者使用下面的 **Kerberos**。

**Kerberos（GSSAPI）。** 在 *Password from* 下选择 *Kerberos 票据（GSSAPI）*。密码字段会消失：不会询问、保存或发送密码。如果服务器要求密码，或不要求 Kerberos 就让你登录，连接会被拒绝并说明原因。

- **服务名称**默认为 `postgres`，除非你填写它（服务器的 `krb_srvname`）。所请求的票据是默认域中的 `service/host`：主机必须是服务主体中的名称，而不是 IP 地址或别名，因为应用不会去解析它。通过 SSH 隧道或端口转发时，它仍然是服务器的名称。
- **用户**是 PostgreSQL 角色，由服务器根据你的主体映射得出（`include_realm` 和 `pg_ident.conf`）。留空时，应用使用你的系统用户名。
- Linux 上的**票据**来自 `kinit`，位于 `KRB5CCNAME` 指定的缓存（或默认缓存）中，或来自 keytab：设置了 `KRB5_CLIENT_KTNAME` 时，Kerberos 会自己获取票据。这些变量必须在应用启动的地方设置。在 Windows 上，它是你的 Windows 会话的票据。
- **没有票据时**，测试会说明该运行什么：`kinit`、这两个变量和 `klist`，后面是 Kerberos 自己的文本。票据过期、服务未知和 KDC 无法访问时也是如此。
- **`psql`、`pg_dump` 和 `pg_restore`** 以同样的方式登录（`PGKRBSRVNAME`、`PGREQUIREAUTH=gss`，且没有 `PGPASSWORD`）。它们的 libpq 必须是带 GSSAPI 构建的。
- **哪些构建支持。** Windows（SSPI）和 Linux（GSSAPI，MIT Kerberos）的软件包。macOS 的不支持：还没有 Mac 构建过它，表单会这样说明。`platypus-driver-pg` 的 Cargo 特性 `kerberos` 会启用它；没有它，不会链接或运行任何与 Kerberos 有关的内容。
- **GSS 加密**（`hostgssenc`、`gssencmode`）不使用：请用 TLS 加密，配合 `hostssl` 或 `host` 行。粘贴的 URL 中的 `krbsrvname` 会被忽略，与应用不认识的任何其他参数一样。

**连接池。** 在事务模式的 PgBouncer、Supavisor 或 Neon 连接池之后，请在表单中选择*事务模式*（或在 URL 中加上 `pgbouncer=true`）。此时语句以匿名方式运行，因为下一个事务可能在另一个服务器会话上。对于以 `.pooler.supabase.com` 结尾的主机和名称中带有 `-pooler` 的 Neon 主机，表单会建议这样做。在该模式下，PL/pgSQL 调试器关闭，代理的只读会话只依靠语句检查。

其他服务器有各自的图块，它们只设置默认值：

| 图块 | 它设置的默认值 |
|---|---|
| CockroachDB | 端口 26257 |
| YugabyteDB | 端口 5433，用户和数据库为 `yugabyte` |
| TimescaleDB | 无 |
| Supabase | 用户和数据库为 `postgres`，TLS 为 `require` |
| Neon、AlloyDB | TLS 为 `require` |

## 权限

浏览只需要服务器向该登录展示的内容。管理标签页读取以下内容：

- **会话**和**锁**：`pg_stat_activity` 和 `pg_locks`。应用列出服务器返回的客户端会话；服务器向该登录显示其他用户的多少会话和语句，是服务器的规则，应用不会检查。*取消语句*和*结束会话*调用 `pg_cancel_backend` 和 `pg_terminate_backend`；该登录是否被允许，由服务器决定。
- **最耗时的语句**：`pg_stat_statements` 扩展。没有它时，标签页会说明如何开启（把它加入 `shared_preload_libraries`，重启，`CREATE EXTENSION`）。
- **表和索引**：`pg_stat_user_tables` 和 `pg_stat_user_indexes`。
- **参数**和**仪表板**：`pg_settings`、`pg_stat_database`、`pg_stat_activity` 和复制视图；备用服务器只会列给 `pg_monitor` 的成员。该登录无法读取的数字显示“—”并说明原因。
- **用户和角色**：`pg_roles` 和 `pg_auth_members`；更改它们需要这些语句所需的权限。
- **维护**：对表执行 `VACUUM`、`ANALYZE` 和 `REINDEX`。
- **备份…和恢复…**运行 `pg_dump`、`pg_restore` 和 `psql`，它们必须在 PATH 中。
- **调试…**需要服务器上有 `pldebugger` 扩展。

## 可用功能

- 在表格中按主键（或非空列上的唯一键，或你设置的虚拟键）编辑数据。数组和复合值每行编辑一个元素或字段。
- 带手动提交的事务，以及在服务器上取消的语句。
- 执行计划和分析执行计划；计划从 PostgreSQL 的 JSON 或文本中读取。
- 每个连接可有多个数据库、分区、PostGIS 几何、数组和复合类型。
- 导入和复制到 PostgreSQL 使用 `COPY`。
- 上面的每个管理标签页、PL/pgSQL 调试器，以及模式工具（表编辑器、ER 图、比较）。

**CockroachDB 和 YugabyteDB** 使用同一个驱动打开，但应用会识别出是哪种服务器，并关闭依赖 PostgreSQL 自身视图的服务器工具：没有会话、锁、仪表板、语句、表和索引、参数、用户或维护标签页，也没有调试器。CockroachDB 有自己的计划格式（执行计划和分析执行计划可用），导入以 `INSERT` 语句而不是 `COPY` 进行。没有键的 CockroachDB 表通过其隐藏的 `rowid` 编辑。TimescaleDB（已测试）、Supabase、Neon 和 AlloyDB（没有针对这些服务测试过）在应用看来就是 PostgreSQL，具备上面的一切；托管服务对你的角色有哪些限制，是它自己的规则。

## 代理

只读代理和只读连接用 `default_transaction_read_only` 打开会话（PostgreSQL、CockroachDB 和 YugabyteDB），所以由服务器本身拒绝写入；在事务模式连接池之后，则只有语句检查。`server_activity` 和 `top_queries` 适用于真正的 PostgreSQL，不适用于 CockroachDB 或 YugabyteDB。见[代理和 CLI](../../AGENTS.md)。

## 已知限制

- 低于 12：应用会连接，并警告某些功能可能无法工作。
- 在事务模式连接池之后，调试器关闭。
- 终端模式需要 `psql` 在 PATH 中。

## 测试环境

一致性测试套件在 PostgreSQL 12、14、16 和 17、PostgreSQL 17 上的 TimescaleDB 2.30、CockroachDB 26.2 和 YugabyteDB 2025.2 上运行，另外还有事务模式的 PgBouncer、双向 TLS 下的客户端证书，以及装有 `pldebugger` 的服务器上的调试器。Supabase、Neon 和 AlloyDB 没有测试：它们就是带默认值的 PostgreSQL 驱动。

Kerberos 使用来自真实 KDC（MIT Kerberos，位于 `docker/compose.yml` 的 `kerberos` 配置中）的真实票据，在 PostgreSQL 17 上、从带 GSSAPI 的 Linux 客户端进行了测试：来自 `kinit` 的票据、keytab、TLS、`psql`、没有票据、票据过期和服务名称错误。**通过 Windows 的 SSPI 登录尚未用真实票据试过：**不在域中的 Windows 机器，不更改系统设置（`ksetup`）就无法访问测试 KDC。在 Windows 上，只测试了没有票据时的情形。服务器的 `sspi` 行（Windows 上的 PostgreSQL）同样没有试过。

另见：[连接](../connect.md)、[故障排除](../troubleshooting.md)。
