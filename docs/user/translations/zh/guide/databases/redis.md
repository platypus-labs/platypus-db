<!-- source: 8b43e38d9f7d -->
[← 数据库](README.md)

# Redis

驱动基于 `redis` crate（服务器支持时使用 RESP3，TLS 使用 rustls）。它可以打开 Redis 6 及更高版本，以及用于 JSON 键的 Redis Stack。编辑器接受 **Redis 命令**，每行一条。驱动是应用的一部分，无需下载任何东西。

## 连接

| 你粘贴的内容 | 解读为 |
|---|---|
| `redis://:s%40cret@cache:6380/3` | 普通 TCP，只有密码，数据库 3 |
| `redis://app:pw@cache/0` | ACL 用户及其密码 |
| `rediss://app:pw@cache` | TLS（经过验证：`verify-full`） |

默认端口是 6379。路径（或“数据库”字段）是**数据库编号**，为空时是 `0`；其他任何值都是错误。用户和密码是 ACL 的：只有密码时，以默认用户登录。

**TLS** 对 `rediss://` URL 以及从 `require` 起的 SSL 模式开启（`redis://` 表示不使用 TLS，`prefer` 也不会开启它，因为 Redis 在单独的端口上提供 TLS）。`require` 不检查证书；`verify-ca` 和 `verify-full` 会检查。CA 文件和客户端证书填在 TLS 字段中。会话为它用到的每个数据库打开一个连接，因为在共享连接上执行 `SELECT` 会影响使用该连接的所有其他人。

## 权限

该登录的 ACL 规则决定导航器和命令能做什么。导航器读取 `INFO`，并对每种键类型运行 `SCAN … TYPE`；浏览某个类型运行 `BROWSE`，绝不运行 `KEYS`。没有管理标签页。

## 可用功能

- 编辑器中的命令，引号规则与 `redis-cli` 相同；以 `#` 和 `--` 开头的行是注释。回复是行，嵌套的回复在文档树中以 JSON 显示（`F7`）。
- `SELECT n` 把会话移到另一个数据库。`MULTI` 会把命令排队，直到 `EXEC`，后者把它们作为一个事务发送，并显示每个回复。
- 导航器列出有键的数据库，每个数据库下按键类型各有一张表：`string`、`hash`、`list`、`set`、`zset`、`stream`，以及配合 RedisJSON 的 `json`。打开一个类型会运行 `BROWSE db0.hash`，可选带 `MATCH` 和 `LIMIT`，大的键一次读取一块。
- **表格可以编辑行**：值、TTL、键名、哈希字段、成员和分数；新行添加键和条目，删除的行移除它们。预览显示这些命令，它们作为一个 `MULTI`/`EXEC` 运行；服务器拒绝某条命令时，不会撤销其他命令。*撤销脚本*会恢复这些值。已存在的键、字段或成员会在任何命令运行之前就终止保存。
- `FLUSHDB` 和 `FLUSHALL` 会先询问，与不带 `WHERE` 的 `UPDATE` 相同；在生产连接上，更改服务器的命令（`CONFIG SET`、`SHUTDOWN`）会像 DDL 一样询问。只读连接只运行读取的命令。
- 取消会停止阻塞命令（`BLPOP`、`XREAD BLOCK`）。
- 终端模式运行 `redis-cli`（如果它在 PATH 中），密码放在它的环境中。
- 所有命令和键类型在 [FEATURES.md](../../FEATURES.md#redis) 中说明。

## 不可用功能

- 订阅（`SUBSCRIBE` 等）和 `MONITOR` 会被拒绝：它们会占用整个连接。
- 没有手动提交：`MULTI`/`EXEC` 是唯一的事务。
- 没有执行计划、没有对象 DDL、没有 SQL 工具（表编辑器、ER 图、模式比较、查询构建器、SQL 导入），没有变更前快照。

## 代理

`run_query` 和 `sample_rows`（`BROWSE … LIMIT n`）可用；任何不是读取的命令都会先询问。显示服务器凭据的命令（`ACL GETUSER`、`ACL LIST`，以及模式匹配密码设置的 `CONFIG GET`）对只读代理会被拒绝。在 Redis 上被拒绝的有：`explain`、`get_er_diagram`、`propose_edits` 和 `query_across_databases`。见[代理和 CLI](../../AGENTS.md)。

## 已知限制

- 低于 6.0：它会连接，并警告某些功能可能无法工作。
- 只有数据库的键不超过 10,000 个时，才会显示表的键数。
- 不是 UTF-8 的值会带引号并以转义形式显示。

## 测试环境

一致性测试套件在 Redis Stack 7.4（带 RedisJSON）上运行：元数据、每种类型的 `BROWSE`、命令及其错误、保存并撤销表格的编辑、取消阻塞命令、ACL 用户和数据库选择，以及丢失后重新打开的连接。Redis 6 是驱动接受的最低版本，但测试套件没有运行它。

另见：[连接](../connect.md)、[故障排除](../troubleshooting.md)。
