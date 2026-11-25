<!-- source: 284fda1cc34a -->
[← 用户指南](README.md)

# 连接数据库

![为新连接选择数据库](../../../../screenshots/connections.png)

## 通过 URL

1. 点击**新建连接**（或导航器的 `+`）。
2. 在标志下方粘贴 URL：`postgres://`、`mysql://`、`sqlserver://`、`clickhouse://`、`libsql://`、`mongodb://`、`redis://`、`rediss://`、`elasticsearch://`、`opensearch://`、JDBC URL，或 ADO.NET / ODBC 字符串。表单打开时已按该数据库填好。
3. 把密码填在**密码**字段，而不是 URL 里：如果 URL 中带有密码，表单会提示。
4. 点击**测试连接**，然后点击**保存**。

或者从标志中选择数据库（顶部的搜索框按 `Enter` 选中第一个匹配项）。这个选择决定驱动；**返回**会回到标志界面。

## 表单

表单有四页：**常规**、**外观与安全**、**网络**和**初始化**。页面名称后的圆点表示该页中有设置。

- **常规**：主机、端口、用户、数据库，以及密码的来源（见下文）。
- **外观与安全**：环境（开发、测试或生产）、颜色、**只读（拒绝修改数据）**复选框、禁止导出和禁止编辑数据的复选框，以及代理的访问权限。见[安全](safety.md)和[代理](agents.md)。
- **网络**：SSH 隧道、SSL 模式和证书、代理、端口转发，以及围绕连接执行的命令。
- **初始化**：变量文件、默认模式或时区，以及每次连接时运行的语句。

## 密码

在**常规**中，密码来自以下之一：

- **保存在钥匙串中**：系统钥匙串。**测试连接**只会把它发送给保存它时对应的服务器：如果你更改了主机、端口或途经的 SSH 服务器，就会要求你为这次测试输入密码。同一个 PostgreSQL 或 MySQL 服务器上的另一个数据库不算另一台服务器。
- **每次询问**：每次连接时输入。代理无法打开这样的连接。
- **.pgpass 文件**（仅限 PostgreSQL；由连接的主机、端口、数据库和用户来选定行，与 libpq 相同）。
- **Shell 命令**：打印出密码的命令。

任何字段都可以读取写成 `${env:NAME}` 的环境变量。密码管理器（1Password、Bitwarden、HashiCorp Vault、AWS Secrets Manager）出现在同一个列表中，适用于表单要求填写密码或令牌的每一种数据库；没有哪一个只限于某一种数据库。填写密钥的引用：应用据此构建 shell 命令（`op read`、`bw get password`、`vault kv get`、`aws secretsmanager get-secret-value`），并在每次连接时运行。必须已安装并登录它们的命令行工具。这里的内容都没有针对真实的 1Password、Bitwarden、Vault 或 AWS 账户运行过：只测试了应用构建的命令和 shell 命令本身。

有些数据库完全不用密码登录，同一个列表也提供这些方式：来自云命令行工具的令牌（PostgreSQL 和 MySQL 用 AWS IAM 和 Google Cloud IAM，这两者和 SQL Server 用 Microsoft Entra ID）、SQL Server 的 Windows 登录，以及 Windows 和 Linux 上 PostgreSQL 的 **Kerberos 票据**。先运行 `kinit`，或设置 `KRB5CCNAME` 或 `KRB5_CLIENT_KTNAME`；在 Windows 上，票据是你当前会话的。发送给服务器的只有票据，没有票据的测试会说明该运行什么。详细信息，以及哪些测试过、哪些没有测试过，见 [PostgreSQL 页面](databases/postgresql.md)：已有一张真实票据在 Linux 上登录成功，但尚未通过 Windows 的 SSPI。

### 主密码

主密码会把已保存的密码放在加密文件中，而不是钥匙串中。

1. 打开设置（`Ctrl+,`）› **主密码**。
2. 点击**设置主密码…**，输入两次（至少 8 个字符）并确认。
3. 要回到钥匙串，在同一页面点击**关闭…**。**立即锁定**会马上锁定。

它无法找回：没有它，已保存的密码就会丢失。代理无法输入它（见[故障排除](troubleshooting.md#代理看不到连接)）。

## TLS

在**网络**页面，**SSL 模式**取 libpq 的值：`disable`、`prefer`、`require`、`verify-ca` 和 `verify-full`。`prefer` 和 `require` 只加密而不检查证书；`verify-ca` 和 `verify-full` 会检查证书，因此可能需要 **CA 证书 (PEM)**。对于通过证书验证客户端的服务器，请提供**客户端证书 (PEM)** 和**客户端密钥（PEM）**（PostgreSQL 和 MySQL）。

## SSH 隧道

![网络页面上的 SSH 隧道](../../../../screenshots/guide/connect-network.png)

1. 在**网络**页面，勾选**通过 SSH 隧道连接**。
2. 填写 **SSH 主机**、端口和 **SSH 用户**。
3. 用密码、**私钥文件**（及其口令）或 **SSH 代理**进行验证。如果有堡垒机，添加**跳板机**（**先经过跳板机**）。
4. 点击**测试连接**。

服务器的密钥会与 `~/.ssh/known_hosts` 核对：未知的服务器在第一次连接时被记住，之后密钥发生变化的服务器会被拒绝。**网络配置**可以在多个连接之间共用同一套隧道和 SSL 设置。

## 代理和端口转发

同样在**网络**页面：SOCKS5 或 HTTP 代理（`socks5://user:password@host:1080`、`http://host:3128`），或通过 `kubectl` 或 AWS Systems Manager 会话进行端口转发（必须已安装并登录相应的 CLI）。端口转发要单独使用：请移除 SSH 隧道或代理。

## 下一步

各数据库的详细信息：[数据库](databases/README.md)。云登录、连接池、从 DBeaver、DataGrip 和 pgAdmin 导入（包括 Db2 连接）以及导出连接，见 [FEATURES.md](../../../FEATURES.md#connections)。如果无法连接，见[故障排除](troubleshooting.md#连接错误)。
