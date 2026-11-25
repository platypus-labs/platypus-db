<!-- source: b5b424fa73d7 -->
[← 数据库](README.md)

# ClickHouse

驱动与 ClickHouse 的 **HTTP 接口**通信，并以 Arrow 流格式读取结果。它是应用的一部分，无需下载任何东西。它可以浏览、查询和设计表，但不能在表格中编辑行。

## 连接

| 你粘贴的内容 | 解读为 |
|---|---|
| `clickhouse://default@ch.example.com/logs` | 端口 8123 上的 HTTP，数据库 `logs` |
| `clickhouse://ch.example.com/logs?sslmode=require` | HTTPS；URL 中没有端口时为 8443 |
| `jdbc:clickhouse://host:8123/db` | JDBC URL |
| ClickHouse 表单中的 `https://host:8443` | 服务器的 HTTP(S) URL |

请使用 **HTTP** 端口，而不是原生端口（9000）。用户为空时是 `default`；密码在 `X-ClickHouse-Key` 请求头中发送。“数据库”字段会在连接后运行 `USE <database>`。

**TLS** 由模式决定，因为 HTTP 无法协商它：`disable` 和 `prefer`（默认）使用**普通 HTTP**连接；`require` 和 `verify` 模式使用 HTTPS。使用 HTTPS 时，如果 URL 没有给出端口，默认端口是 8443；如果你填写表单，请自己输入端口。`require` 不检查证书，`verify-ca` 检查证书链，`verify-full` 还检查主机名；`sslrootcert` 指定你的 CA 文件，否则使用系统信任的证书。

一个连接就是一个 HTTP 会话：`SET`、`USE` 和临时表在语句之间保持有效。

## 权限

浏览读取 `system.databases`、`system.tables`、`system.columns` 和 `system.data_skipping_indices`；该登录能看到 ClickHouse 允许它在那里看到的内容。各标签页读取：

- **参数**：`system.settings` 和 `system.server_settings`。
- **仪表板**：`system.metrics`、`system.events`、`system.parts`（数据库大小）和 `system.replicas`；该登录无法读取的内容显示“—”。
- **用户和角色**：`system.users`、`system.roles`、`system.grants` 和 `system.role_grants`。在服务器配置文件中定义的用户也会列出；服务器拒绝更改它们。

## 可用功能

- 查询、结果、导航器、自动补全、对象 DDL、元数据搜索，以及表设计器（包括引擎子句和跳数索引）。
- 执行计划显示 `EXPLAIN indexes = 1` 的计划；没有分析执行计划。
- 取消用 `KILL QUERY` 在服务器上停止语句。
- 参数、仪表板以及用户和角色标签页。没有会话、锁、语句或表和索引标签页。
- 导出，以及以 `INSERT` 语句导入到 ClickHouse。

## 不可用功能

- **不能在表格中编辑数据**：它会显示“ClickHouse 表不能就地编辑”。保存时需要在事务中检查每处更改的行数，而 ClickHouse 既没有事务，更新也没有行数。请用 SQL 修改行。
- **没有事务**：手动提交工具栏被隐藏；每条语句单独生效。
- 没有外键或唯一约束（主键只用来排序数据）。

## 代理

只读代理和只读连接发送的每条语句（`SET` 除外）都带有 `readonly = 1`，所以服务器会拒绝写入，也拒绝访问其他服务器或其文件的表函数（`url`、`s3`、`remote`、`file` 等）。代理的语句还会与一份只读取本地数据的表函数列表对照检查。见[代理和 CLI](../../AGENTS.md)。终端模式不可用：`clickhouse-client` 使用原生端口。

## 已知限制

- 除非模式是 `require` 或更强，否则使用普通 HTTP：默认的 `prefer` 不加密。
- 只使用 HTTP 接口；只暴露原生端口的服务器无法连接。

## 测试环境

一致性测试套件在 ClickHouse 26.3 上运行。更旧的 ClickHouse 版本没有测试，应用也没有为它设定最低版本。

另见：[连接](../connect.md)、[故障排除](../troubleshooting.md)。
