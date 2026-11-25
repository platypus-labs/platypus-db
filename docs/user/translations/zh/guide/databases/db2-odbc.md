<!-- source: 1206da228de0 -->
[← 数据库](README.md)

# IBM Db2 和 ODBC（Windows）

Db2，以及任何带 ODBC 驱动或数据源（DSN）的数据库，都通过系统的 ODBC 驱动管理器连接。目前这**仅适用于 Windows**：在 macOS 和 Linux 上，Db2 或 ODBC 连接会提示 ODBC 驱动只能在 Windows 上使用。

## 首次使用：IBM 的 ODBC 驱动（Db2）

当你为新连接选择 **IBM Db2**，而这台机器上没有注册任何 IBM ODBC 驱动时，表单上方会弹出一个窗口：*IBM Db2 需要 IBM Data Server Driver for ODBC and CLI*。它会显示软件包（版本 12.1.4）、大小、IBM 的条款和来源，并用 SHA-256 校验下载。**下载并安装**会把它取到应用的数据文件夹中，并用 `db2cli install -setup` 注册；为此，**Windows 会请求管理员权限（用户帐户控制）**。**取消**则表单仍可使用，并在*测试连接*时和第一次打开时再次提示。

系统中已有的驱动也算数：应用会查找名为 `IBM DB2 ODBC DRIVER…` 或 `IBM DATA SERVER…` 的驱动，无论是它自己注册的还是完整的 IBM 客户端注册的，并按名称取最新的。连接的 `driver` 选项会替换这个选择。*设置 › 驱动*会列出并移除下载的驱动，并显示系统的 ODBC 驱动和数据源。

## 连接

**IBM Db2**：主机、端口（默认 50000）、**数据库**（主机和数据库都是必填的）、用户和密码。应用自己构建连接字符串（`DATABASE`、`HOSTNAME`、`PORT`、`PROTOCOL=TCPIP`、`UID`、`PWD`）。TLS：`disable` 和 `prefer` 模式不使用它连接；`require` 及以上会加上 `Security=SSL`，`sslrootcert` 变成 `SSLServerCertificate`。Db2 服务器在与明文端口不同的端口上监听 TLS，所以请设置端口。通过 SSH 隧道时，使用隧道的本地端。

**平台**决定 Db2 服务器的类型：

- *Db2 for Linux, UNIX and Windows (LUW)*，默认值，也适用于旧的 8.x 服务器。
- *Db2 for z/OS*：同一个 IBM 驱动，默认端口 446，**数据库**是位置名称。IBM 的驱动要在这台机器上有 Db2 Connect 许可证才能访问 z/OS。
- *Db2 for IBM i*（AS/400）：IBM 自己的 *IBM i Access ODBC Driver*（IBM i Access Client Solutions，从 IBM 安装，而不是由应用安装；较旧的 iSeries Access 驱动也可以）。主机是系统名称，数据库可选，用来设置默认库，没有端口，TLS 会加上 `SSL=1`。它不能通过 SSH 隧道，因为该驱动会打开服务器的多个端口。

**其他 ODBC 驱动**（通用选项）：给出以下之一

- 系统已知的**数据源（DSN）**，手动输入或从建议中选取；
- **驱动**名称（从系统的 ODBC 驱动中建议），加上包含其余内容的**连接字符串**，例如 `Server=host;Database=sales`；
- 单独的**连接字符串**，原样使用（它可以指定自己的 `DRIVER=`）。

用户和密码字段会作为 `UID` 和 `PWD` 添加，除非字符串中已有它们。粘贴带 `Driver={…}` 的 ADO.NET 或 ODBC 字符串时，如果驱动是 SQL Server、PostgreSQL、MySQL/MariaDB 或 ClickHouse，会被读入对应的表单；任何其他驱动或 `DSN=` 会变成通用 ODBC 连接，字符串保持原样。

## 权限

由数据库登录所拥有的权限决定。导航器使用 ODBC 的目录调用（`SQLTables`、`SQLColumns`、`SQLPrimaryKeys`、`SQLForeignKeys`）。没有管理标签页。

## 可用功能

- 查询和结果，使用驱动报告的类型。
- 手动提交和回滚。
- 取消：无法调用 ODBC 的取消，所以尚未开始的语句会连同其会话一起被放弃（应用会打开一个新的会话）；正在返回行的语句在其流被丢弃时停止。
- 导航器，来自目录调用。Db2 的 SQL 按 Db2 的引号规则书写（不带引号的名称折叠为大写，行数用 `FETCH FIRST` 限制）；通用驱动书写标准 SQL。

## 不可用功能

- 没有执行计划、没有对象 DDL、没有管理标签页、没有批量加载（导入使用 `INSERT`）。
- **长度超过 64 KiB 的文本和二进制值会报错**，而不是分配巨大的缓冲区。
- **在表格中编辑数据**没有在这些数据库上测试过。表格不会阻止它，但由于没有保存点，在手动提交模式下保存失败会回滚会话的整个事务，并会这样提示。
- 终端模式不可用。

## 代理

只读代理只依靠语句检查。见[代理和 CLI](../../AGENTS.md)。

## 已知限制

- 仅限 Windows。
- 测试环境中没有 Db2 的容器，所以 Db2 的连接字符串和驱动检测只经过单元测试。

## 测试环境

通用 ODBC 驱动通过 Windows 自带的 `SQL Server` 驱动针对 SQL Server 2022 运行：类型、长结果、带 SQLSTATE 的错误、元数据、提交和回滚、取消等待中的语句、SQL Server 自己的字面量和类型，以及导入。IBM Db2 没有针对服务器测试过。

另见：[连接](../connect.md)、[故障排除](../troubleshooting.md)。
