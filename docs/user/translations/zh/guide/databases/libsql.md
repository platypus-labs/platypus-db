<!-- source: 7b39ba524d58 -->
[← 数据库](README.md)

# libSQL 和 Turso

该驱动通过 HTTP(S) 以 **Hrana 3** 协议与 libSQL 服务器（`sqld`）或 Turso 通信。SQL 是 SQLite 的，因此导航器、类型和计划也都是 SQLite 的。驱动是应用的一部分，无需下载。

## 连接

| 你粘贴的内容 | 解读为 |
|---|---|
| `libsql://db-acme.turso.io?authToken=…` | 对该主机使用 HTTPS，令牌作为密码 |
| `libsql://:token@localhost:8080?sslmode=disable` | 普通 HTTP，令牌位于密码位置 |
| `libsql://localhost:8080?tls=0&authToken=t` | `tls=0` 为普通 HTTP，`tls=1` 为 HTTPS |
| `wss://…`、`ws://…` | 同一服务器的 HTTP(S) 地址：`wss` 为 TLS，`ws` 不是 |
| libSQL 表单中的 `https://host` 或 `http://host` | 服务器的 HTTP 地址 |

没有端口时使用该协议的默认端口：443，TLS 关闭时为 80。

- **令牌**：密码字段，或 URL 中的 `authToken`。
- **TLS**：`libsql://` URL 是 HTTPS，并检查证书和主机名（`verify-full`），因为 URL 中带有令牌。表单中的 Turso 图块同样从 `verify-full` 开始。对于没有 TLS 的本地 `sqld`，请选择 `disable`，或在 URL 中使用 `tls=0`。`sslrootcert` 指定你的 CA 文件；否则使用系统信任的证书。

## 权限

由令牌允许的内容决定：应用会发送你运行的语句，以及供导航器使用的 SQLite 目录查询。

## 可用功能

- 在表格中按主键（或非空列上的唯一键，或虚拟键）编辑数据。
- 手动提交和回滚。语句在服务器于请求之间保持打开的流（`baton`）上运行，因此事务和临时表可以保持。如果服务器在事务之外断开该流，应用会打开另一个流并继续；在事务内部，连接会报告自己已丢失，事务也随之丢失。
- 执行计划显示 SQLite 的查询计划；没有分析执行计划。
- 元数据搜索和对象 DDL，以及与 SQLite 相同的模式工具。
- 没有会话、锁、仪表板、用户或参数标签页。

## 不可用功能

- **取消**：HTTP 无法停止正在运行的语句，所以取消会回答不支持取消 libSQL 语句。
- 结果由服务器整体返回，而不是逐行返回。
- 终端模式不可用：libSQL 服务器没有可用来运行脚本的命令行客户端。

## 代理

只读代理和只读连接只依靠语句检查；不会通知服务器拒绝写入。见[代理和 CLI](../../../../AGENTS.md)。

## 已知限制

- 语句发送之后发生网络故障，可能无法确定它是否已运行；应用不会重新发送它。

## 测试环境

一致性测试套件在 `sqld` v0.24.33（容器 `ghcr.io/tursodatabase/libsql-server`）上运行，不使用 TLS。Turso 的云服务没有测试过。

另见：[连接](../connect.md)、[故障排除](../troubleshooting.md)。
