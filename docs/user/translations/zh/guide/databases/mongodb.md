<!-- source: 81c6c1d2c8d1 -->
[← 数据库](README.md)

# MongoDB

驱动是 MongoDB 官方的 Rust 驱动（使用 rustls，因此不需要 OpenSSL）。它可以打开 MongoDB 5.0 及更高版本。编辑器接受 **mongosh 语法**，而不是 SQL。驱动是应用的一部分，无需下载任何东西。

## 连接

| 你粘贴的内容 | 解读为 |
|---|---|
| `mongodb://user:pass@host:27017/shop?authSource=admin&replicaSet=rs0` | URL 形式；默认端口 27017 |
| `mongodb://a:27017,b:27018/?authSource=admin` | 种子列表，完整保留在“主机”字段中 |
| `mongodb+srv://user:pass@cluster0.example.net/shop` | SRV：主机来自 DNS |

表单字段容纳不下的内容，作为连接字符串选项写在**“数据库”字段**中的 `?` 之后：`shop?authSource=admin&replicaSet=rs0`，对 `mongodb+srv` 则加上 `srv=true`。`?` 之前的名称是语句运行所在的数据库，直到 `use` 更改它（为空时是 `test`）。

- **登录**：用户和密码，除非 `authSource` 另有说明，否则对 `admin` 进行认证。粘贴的 URL 如果路径中指定了数据库，就按连接字符串规范把它用作认证源。带有机密的选项（例如 `authMechanismProperties` 中的密码）在你粘贴 URL 时不会保留；表单会列出它省略了什么。
- **TLS**：URL 中的 `tls=true` 或 `ssl=true` 表示 `verify-full`；`tls=false` 表示 `disable`。MongoDB 没有机会性 TLS，所以 `prefer`（默认）保持连接字符串所暗示的：`mongodb+srv` 开启 TLS，其他情况关闭。`require` 开启 TLS 并接受任何证书；`verify-ca` 和 `verify-full` 都会检查证书链和主机名。`sslrootcert`（或 `tlsCAFile`）指定你的 CA 文件；客户端证书可以在表单中给出，或在“数据库”字段中用 `tlsCertificateKeyFile` 给出。连接字符串中的 TLS 选项在每种模式下都会保留。
- **通过 SSH 隧道**只能访问其后的那一台服务器（`directConnection=true`）。

## 权限

导航器列出该登录能列出的数据库和集合，并抽样文档以显示字段。不会为管理而读取其他任何内容：没有管理标签页。你写的语句需要它们所需的角色。

## 可用功能

- mongosh 语法的语句，支持 shell 的值（`ObjectId(...)`、`ISODate(...)`、`NumberLong(...)`、`/regex/i`）和 Extended JSON：
  - **读取**：带 `sort`、`limit`、`skip`、`projection`、`collation`、`hint`、`min`、`max`、`maxTimeMS`、`readConcern` 等的 `find`，以及 `findOne`、`aggregate`、`countDocuments`、`estimatedDocumentCount`、`distinct`。
  - **写入**：`insertOne`/`insertMany`、`updateOne`/`updateMany`、`replaceOne`、`deleteOne`/`deleteMany`、`bulkWrite`、`findOneAndUpdate`/`findOneAndReplace`/`findOneAndDelete` 和 `findAndModify`，以及它们的选项（`arrayFilters`、`collation`、`hint`、`let`、`upsert`、`ordered`、`writeConcern`）。PlatypusDB 不认识的选项会按名称被拒绝，绝不会被丢弃。
  - **索引和集合**：`createIndex`、`createIndexes`、`dropIndex`、`dropIndexes`、`hideIndex`、`unhideIndex`、`getIndexes`、`createCollection`、`createView`、`drop`、`renameCollection`、`convertToCapped`、`stats`、`validate`、`latencyStats`、`dataSize`、`storageSize`、`totalIndexSize`、`totalSize`、`isCapped`、`exists`。
  - **计划**：语句之后的 `.explain()` 或之前的 `db.c.explain().find(…)`，可带详细级别，以及编辑器的**执行计划**。计划绝不会执行写入。
  - **服务器**：`db.runCommand`、`db.adminCommand`、`db.aggregate`、`db.serverStatus`、`db.currentOp`、`db.killOp`、分析器、用户和角色（`db.createUser` 等）、`rs.status`、`rs.conf`、`sh.status`、`show dbs`、`show collections`、`show users`、`show roles`、`show profile`、`show logs` 和 `use`。
- 结果是顶层字段的表格，聚焦的文档以树的形式显示在旁边（`F7`）。导航器列出数据库、集合、从文档中抽样得到的字段和索引；编辑器会补全集合、方法和字段。
- 取消会停止正在运行的语句；被放弃的结果会释放连接。
- 在副本集或分片集群上可使用**手动提交**（见[事务](#事务)）。
- **在表格中编辑文档**（见[在表格中编辑文档](#在表格中编辑文档)）。
- 集合的定义以 DDL 形式提供。
- **生成数据…**会用假文档填充集合。
- 同样的细节见 [FEATURES.md](../../../../FEATURES.md#mongodb)。

## 不可用功能

- **只有部分结果可以在表格中编辑**：单个集合的文档（见[在表格中编辑文档](#在表格中编辑文档)）。其他所有结果，例如聚合、视图或 `findOne`，保持只读。请用 `updateOne`、`insertOne` 等来修改这些文档。
- **独立服务器上没有事务**：MongoDB 只在副本集或分片集群上运行事务。那里的手动提交会被拒绝，并说明原因。
- **没有 JavaScript**：编辑器运行的是语句，所以 `forEach`、`map`、`next`、`mapReduce`、较旧的批量构建器（请用 `bulkWrite`）和变量会被拒绝，并说明原因。`watch` 也会被拒绝：变更流会占用整个连接。读取上的 `.comment()` 会被省略：应用自己的注释用来找到要取消的操作。
- `bulkWrite` 不是原子的，与 mongosh 相同：失败之前已运行的内容会保留，错误会说明执行到了哪里。相邻的插入一起发送，其他每个写入各自单独发送。
- 没有 SQL：没有表编辑器、ER 图、模式比较、查询构建器、SQL 导入、变更前快照或终端模式（`mongosh` 只在命令行上接受密码）。

## 在表格中编辑文档

表格可以编辑单个集合上普通 `find` 的结果：数据查看器，或你输入的语句，例如 `db.orders.find({ status: 'paid' }).sort({ _id: 1 }).limit(20)`。集合可以用 `getCollection("my orders")` 或 `db.getSiblingDB("shop").orders` 指定。在 `use` 之后，集合是你切换到的那个数据库的集合。投影只要只列出要包含的字段并保留 `_id` 就可以。视图、聚合、`findOne`、`distinct`、计数、不带 `_id` 的投影以及排除字段的投影保持只读。

文档通过其 `_id` 找到。没有 `_id` 的结果是只读的。**预览 SQL** 显示将要运行的 mongosh，每个文档一条语句：

- 编辑过的文档是 `updateOne({ _id: … }, { $set: { … } })`。NULL 单元格写入 `null`。当数字发生变化时，更新改用管道 `[{ $set: { … } }]`：数字用 `$switch` 按文档存储的类型写入（见下文），其他每个值作为 `$literal`，这样像 `$total` 这样的字符串或带 `$` 键的文档就会原样保存；
- 新文档是 `insertOne({ … })`。保持为 NULL 的单元格不会发送。没有 `_id` 时，应用会生成一个 `ObjectId`；
- 删除的文档是 `deleteOne({ _id: … })`。

值按其列的 BSON 类型写入：`bool`、`string`、`objectId`、`date`，以及根据表格显示的 JSON 得到的 `document` 和 `array`。日期写成 `2024-01-02 10:30:00` 的形式，带不带时区都可以（不带时按 UTC）。文档或数组内部的数字保持 JSON 赋予它们的类型：整数是 int（超过 32 位则是 long），带小数或指数的数字是 double。

只要文档存储的类型 `int`、`long`、`double` 或 `decimal` 能精确容纳新值，数字就保持该类型：列的类型只是其文档共有的类型（int 和 long 组成 `long` 列，其中任何一个与 double 混合则是 `double` 列），所以它并不说明某个文档里存的是什么。被改成 `7` 的 int 仍是 int，long 仍是 long，double 仍是 double。存储的类型无法容纳的值会采用列的类型，或第一个能容纳它的数值类型：int 字段中的 `3.5` 变成 double，`3000000000` 变成 long。超过 2^53 的 long 绝不会写成 double，那样会丢失位数。服务器上不做任何转换：应用只会用它知道能精确容纳该数字的类型来写入。

下面这些会在运行任何内容之前被拒绝，并说明原因：

- 修改 `_id`：MongoDB 从不更改它。请删除该文档再重新插入；
- 混合类型的列（`mixed`）：值的文本无法区分字符串和数字；
- 二进制数据以及表格无法精确写回的其他类型（正则表达式、时间戳、JavaScript）；
- 名称中带 `.` 或以 `$` 开头的字段：`$set` 会把它当作路径或运算符；
- 与其类型不符的值，例如 `int` 列中的 `many` 或 `document` 列中的数组。

每条语句必须恰好修改一个文档。如果有一条没有找到文档，因为该文档已被删除，或其 `_id` 在表格读取之后发生了变化，保存就会停止，并说明是哪条命令。

**要么全部成功要么全部不保存，需要副本集。** 在副本集或分片集群上，保存在它自己的事务中运行，只有每条命令都成功才会提交。如果你已打开事务（手动提交），保存就在其中运行并等待提交。如果某条命令失败，MongoDB 没有保存点：整个事务会被回滚，包括你尚未提交的更改，错误会这样说明。在独立服务器上没有事务：命令依次运行，失败会停止其余命令。它之前的命令保持已保存，错误会说明有多少条。

保存之后的撤销脚本会把文档恢复原状：编辑用旧值的 `$set`，新文档用 `deleteOne`，删除的文档用已加载字段的 `insertOne`。以 `//` 开头的行会说明它无法精确恢复的内容：结果中没有显示的字段、表格无法写入的类型的列、曾经为 NULL 的字段（它们会以 `null` 恢复，或被省略），以及编辑可能改变了其类型的数字（它们会以当时所拥有类型的数字恢复，用同样的 `$switch`）。

## 事务

手动提交适用于副本集或分片集群（`mongos`）；单节点副本集就足够了。在独立服务器上它会被拒绝：**自动提交**按钮被禁用并说明原因。到副本集的生产连接以手动提交模式打开，带提交和回滚。连接只保留一个会话：切换到手动提交之后，或在提交或回滚之后的第一次更改，会在其中打开一个事务（插入、更新或删除、`createCollection`、`createIndex`、保存表格），提交或回滚会结束它。在它之前的读取不会打开事务：它们看到的是已提交的内容，与自动提交相同，所以浏览数据绝不会留下一个等服务器在其生命周期后结束的事务。一旦事务打开，你的读取就在其中运行，并能看到它的写入；其他会话只有在提交之后才能看到。

- **在事务中**，一旦打开（读取）或正在打开（其余）：`find`、`findOne`、`aggregate`、`countDocuments`、`distinct`、插入、更新、替换和删除、`bulkWrite`、`findOneAnd…` 方法、`createCollection` 和 `createIndex`，以及带有读写文档的命令（`find`、`aggregate`、`insert`、`update`、`delete`、`findAndModify`……）的 `db.runCommand` 或 `db.adminCommand`。MongoDB 只在同一事务中创建的集合上接受事务中的索引：对已存在的集合，服务器会拒绝它并结束事务。
- **在事务之外**，与自动提交模式相同，不触及事务：读取元数据或与服务器通信的内容。`show …`、`use`、`db.stats()`、`db.getCollectionNames()`、`getIndexes`、集合的统计信息、`estimatedDocumentCount`、`.explain()`、`db.runCommand` 和 `db.adminCommand` 的其他命令、`rs.*`、`sh.*`、用户和角色、分析器、导航器和补全。对 `admin`、`config` 和 `local` 数据库以及 `system.*` 集合的读取也在事务之外运行。
- **在手动提交中被拒绝**：`drop`、`renameCollection`、`dropIndex`、`dropIndexes`、`hideIndex`、`convertToCapped`、`createView`、时间序列的 `createCollection`、带 `$out` 或 `$merge` 的 `aggregate`，以及对 `admin`、`config`、`local` 或 `system.*` 的写入。MongoDB 不在事务中运行它们，而在事务之外运行它们会立即生效，回滚无法撤销。错误会这样说明；不会发送任何内容，已打开的事务保持原样。请切换到自动提交来运行它们。
- 在事务中读取的结果会在你滚动时继续加载，一次一批，其间可以运行其他语句。在事务打开之前读取的结果会在事务之外继续加载。它的列来自它最初的文档。在提交或回滚之后，其余部分无法读取：请重新运行该语句。

**服务器何时结束事务。** MongoDB 会中止保持打开时间超过 `transactionLifetimeLimitSeconds`（默认 60 秒）的事务。它也会因某些错误而中止事务：重复键、与另一个会话的写入冲突、它拒绝在事务中运行的语句、达到时间限制（`maxTimeMS`）。其他错误，例如未知的管道阶段，会让它保持打开。出错之后，PlatypusDB 会询问服务器事务是否仍然存在。如果不存在，语句或提交的错误会说明服务器结束了事务，其更改已被丢弃（原因是时间限制时会说明时间限制），事务显示为已关闭，下一条语句会打开新的事务。取消事务中的语句会回滚该事务：服务器会结束其操作被终止的事务，而该语句做了什么是未知的。

## 代理

`run_query`、`sample_rows`（`find({}).limit(n)`）和其余工具可用于 mongosh 语句；不是普通查询的语句会先询问。在 MongoDB 上被拒绝的有：`explain`、`get_er_diagram` 和 `query_across_databases`；`estimate_change` 会以一条说明作答。

`propose_edits` 提议要在集合（不是视图）中插入、更新或删除的文档：集合会在其数据查看器中打开，带有针对所提议的 `_id` 的 `find`，更改处于待保存状态，你可以像保存自己的编辑一样保存或丢弃它们。文档只用 `_id` 指明；`ObjectId` 类型的 `_id` 以其 24 位十六进制数字发送，并因为集合的 `_id` 就是 `ObjectId` 而被读作 `ObjectId`。值会按表格保存的规则（见上文）在打开任何内容之前检查。敏感列只按顶层字段遮蔽：文档内部的敏感字段无法区分。见[代理和 CLI](../../../../AGENTS.md)。

## 已知限制

- 低于 5.0：它会连接，并警告某些功能可能无法工作。
- 驱动是会自行重新连接的连接池，所以连接丢失不会像其他数据库那样被报告。

## 测试环境

一致性测试套件在 MongoDB 5.0 和 8.0 上运行：服务器信息、元数据、取消、被放弃的结果、带位置的错误、假文档，以及后面的文档中与第一个文档不同的值。事务在 MongoDB 8.0 的单节点副本集上测试：提交、回滚、事务内外的读取、由多个批次组成的结果、手动提交中的元数据，以及服务器因时间限制而结束的事务。

另见：[连接](../connect.md)、[故障排除](../troubleshooting.md)。
