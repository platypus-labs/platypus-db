<!-- source: 2a42aace96dc -->
[← 数据库](README.md)

# Elasticsearch 和 OpenSearch

一个驱动通过 HTTP API（使用 rustls 的 `reqwest`）与两者通信。连接时服务器会说明它是哪一种，连接中会显示出来（`已连接：OpenSearch 2.17.1`）。驱动是应用的一部分，无需下载任何东西。

## 连接

| 你粘贴的内容 | 解读为 |
|---|---|
| `elasticsearch://user:pass@host:9200/logs-*` | 普通 HTTP；路径是索引模式 |
| `elasticsearchs://user:pass@host:9200` | HTTPS（经过验证：`verify-full`） |
| `opensearch://…` 和 `opensearchs://…` | 相同，记作 OpenSearch，这样 URL 会以那种形式写回 |

默认端口是 9200。**数据库**字段是可选的索引模式（`logs-*`，或用逗号分隔的多个），用来缩小导航器列出的范围；为空时列出所有未隐藏的索引。

- **登录**：用户和密码使用 Basic 认证。用户为空时，密码字段是 **API 密钥**（`encoded` 值），以 `Authorization: ApiKey …` 发送。
- **TLS**：`elasticsearchs://` 和 `opensearchs://` URL 以及从 `require` 起的 SSL 模式使用 HTTPS；`elasticsearch://` 和 `prefer` 是普通 HTTP。`require` 不检查证书；`verify-ca` 和 `verify-full` 会检查。CA 文件和客户端证书填在 TLS 字段中。

## 权限

请求所需要的权限。导航器读取该登录能看到的索引、别名、数据流和映射；没有管理标签页。

## 可用功能

- **按 Kibana 开发者工具控制台的写法书写的请求**：一行如 `GET /orders/_search`（斜杠可选；方法为 `GET`、`POST`、`PUT`、`DELETE`、`HEAD`），然后是它的 JSON 请求体。`_bulk` 和 `_msearch` 接受 NDJSON 请求体，以空行或下一个请求结束。
- **SQL**：其他任何内容都是 SQL（Elasticsearch 上是 `_sql`，OpenSearch 上是 `_plugins/_sql`），带类型的列，长结果使用服务器的游标。错误保留服务器给出的原因，SQL 错误会指出它在语句中的位置。
- 搜索返回的文档与 MongoDB 的结果类似：每个顶层字段一列，聚焦的文档以树的形式显示（`F7`）。聚合以各自的结果返回。
- 导航器把索引列为表（健康状态、分片和副本显示在提示中），把别名和数据流列为视图。打开索引就是搜索它的所有文档，每次读取 1,000 行，使用时间点和 `search_after`（OpenSearch 上是 scroll），所以读取不会在 10,000 条命中处停止。没有映射到列的字段会以 JSON 形式放进最后一个 `_other` 列。
- **表格可以编辑文档**：修改过的单元格是按 `_id` 的 `_update`，新行是 `_create`（已存在的 `_id` 会被服务器拒绝）或生成的文档，删除的行是 `DELETE`。预览显示这些请求，它们依次运行；服务器拒绝其中一个时，其余的会停止，之前的保持不变。*撤销脚本*会恢复旧的 `_source`（带生成的 `_id` 的文档必须手动删除）。
- 不带查询（或 `match_all`）的 `_delete_by_query` 和 `_update_by_query`，以及 `DELETE /_all`，会像不带 `WHERE` 的 `DELETE` 一样先询问。只读连接只运行读取的请求和 SQL。
- 取消会在服务器上停止搜索。索引的对象 DDL 是重新创建它的请求。
- 详细信息见 [FEATURES.md](../../FEATURES.md#elasticsearch-and-opensearch)。

## 不可用功能

- 没有事务（没有手动提交），没有执行计划。
- 没有 SQL 工具：没有表编辑器、ER 图、模式比较、查询构建器、SQL 导入、变更前快照或终端模式。
- 数据查看器没有过滤和排序栏。

## 代理

`run_query` 和 `sample_rows` 可用于控制台请求和 SQL，修改数据的请求会先询问。在 Elasticsearch 和 OpenSearch 上被拒绝的有：`explain`、`get_er_diagram`、`propose_edits` 和 `query_across_databases`。见[代理和 CLI](../../AGENTS.md)。

## 已知限制

- 应用拒绝低于 7.10 的 Elasticsearch 和低于 1.0 的 OpenSearch：与其他数据库不同，连接会失败并提示 "older than … the oldest version supported"。
- 在 Elasticsearch 8 和 OpenSearch 2 上测试过，没有在 7.x 上测试。

## 测试环境

一致性测试套件在 Elasticsearch 8.15.3（登录名 `elastic`）和 OpenSearch 2.17.1 上运行：服务器信息、带类型和分页的 SQL、超过一页的分页搜索、目录和映射、保存并撤销表格的更改、取消缓慢的搜索，以及服务器的错误。

另见：[连接](../connect.md)、[故障排除](../troubleshooting.md)。
