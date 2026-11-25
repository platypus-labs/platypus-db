<!-- source: fd0bf4386fb3 -->
[← 数据库](README.md)

# Oracle

驱动使用 Oracle 的客户端库（ODPI-C，通过 `oracle` crate），在连接时加载。这个库就是 Oracle Instant Client，应用会在首次使用时下载它。

## 首次使用：Instant Client

当你为新连接选择 **Oracle**，而这台机器上没有该客户端时，表单上方会弹出一个窗口：*Oracle 需要 Oracle Instant Client*。它会显示软件包（Basic Light 23.26.4）、大小、许可证（Oracle Free Use Terms and Conditions）和来源，并用 SHA-256 校验下载。**下载**会把它取到应用的数据文件夹中；**取消**则表单仍可使用，并在*测试连接*时和第一次打开时再次提示。*设置 › 驱动*会列出该客户端并可移除它。

- 应用在 Windows（x64）和 Linux（x64 和 ARM64）上下载它。在 macOS 上不下载任何东西：请从 Oracle 安装 Instant Client，并把它的文件夹放进库路径。
- 如果应用没有下载过，它会使用系统能够加载的 Instant Client 或完整客户端。
- Windows 需要 Microsoft Visual C++ Redistributable，Linux 需要 `libaio` 库（根据发行版是 `libaio1`、`libaio1t64` 或 `libaio`）；客户端无法加载时，错误会指出缺少的部分。

## 连接

没有 `oracle://` URL；请改为粘贴 JDBC URL，例如 `jdbc:oracle:thin:@host:1521/service`（thin 和 OCI 形式都能读取），或填写表单：

- **主机**和**端口**（默认 1521），以及“数据库”字段中的**服务名**（例如 `FREEPDB1`）。它们会写成 Easy Connect 字符串 `host:port/service`。
- **没有主机时**，“数据库”字段是 `tnsnames.ora` 中的网络服务名，或完整的连接描述符，按原样写入。
- **用户和密码。** 没有用户时，由操作系统的账户或 Oracle 钱包登录。用户可以写成 SQL*Plus 的方式 `sys as sysdba`，也可以选择**角色**（`SYSDBA`、`SYSOPER`、`SYSBACKUP`、`SYSDG`、`SYSKM`、`SYSASM`）。
- **TLS**：`disable` 和 `prefer` 模式使用普通 TCP；`require` 及以上使用 `tcps://` 和系统信任的证书。`require` 跳过服务器名称匹配；`verify-ca` 和 `verify-full` 会检查它。连接超时同样适用。通过 SSH 隧道时，使用隧道的本地端。

## 权限

浏览读取 `ALL_*` 视图，所以该登录能看到它有权限的对象。取消会发送一个中断；如果语句在几秒后仍在运行，应用会用 `ALTER SYSTEM CANCEL SQL` 从第二个会话取消它，这需要该权限，如果不行，就放弃该会话（它的事务被回滚，并打开新的连接）。

## 可用功能

- 在表格中按主键（或非空列上的唯一键，或虚拟键）编辑数据。
- 手动提交和回滚。
- PL/SQL 块：脚本用一行 `/` 结束一个块。`DBMS_OUTPUT` 的文本会作为消息返回。
- 导航器（表、视图、物化视图、序列、同义词、函数、过程、触发器、类型和包）、对象 DDL 和模式工具。
- 执行计划显示 `EXPLAIN PLAN` 和 `DBMS_XPLAN` 给出的计划，仅为预估：没有分析执行计划。
- 编译错误、数字和纳秒时间、JSON、向量、游标、长 LOB 和文件列，都按数据库中的原样读回。
- 没有会话、锁、仪表板、语句、用户或参数标签页。
- 终端模式不可用。

## 代理

只读代理只依靠语句检查：这里的 Oracle 没有服务器端的只读会话。调用 Oracle 包（例如 `UTL_HTTP`）的语句在运行前会先询问，视为一次更改。见[代理和 CLI](../../../../AGENTS.md)。

## 已知限制

- 应用不会把服务器的版本与最低版本比较；它下载的客户端是 23.26.4。
- 对于低于 12 的服务器，导航器以另一种方式读取数据字典（没有 `ORACLE_MAINTAINED`，没有 `FETCH FIRST`）。测试中只运行 Oracle Free 23.26，所以这条路径没有被测试过。Autonomous Database 和其他 Oracle 云服务也没有测试。
- 在 macOS 上不下载 Instant Client。

## 测试环境

一致性测试套件在 Oracle Free 23.26（容器 `gvenzl/oracle-free`）上运行，使用下载的或系统的 Instant Client：类型、长结果、指出错误位置的错误、PL/SQL、导航器、提交和回滚、保存大量已编辑的行、取消（包括 PL/SQL 的休眠）、计划、可以反向执行的 DDL、以 `SYSDBA` 连接，以及模式比较。其他版本没有测试。

另见：[连接](../connect.md)、[故障排除](../troubleshooting.md)。
