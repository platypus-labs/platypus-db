<!-- source: 25c359215e93 -->
[← 用户指南](README.md)

# 故障排除与常见问题

先在连接表单中点击**测试连接**：它会报告失败原因，附上服务器自己的消息，并在有帮助时给出服务器版本。

## 连接错误

### 无法访问主机

先检查主机和端口（表单会提示“缺少主机。”或“端口必须是 1 到 65535 之间的数字。”）。然后看你与服务器之间有什么：SSH 隧道、代理、端口转发，或在连接前后执行的、用来启动 VPN 的命令（**网络**页面）。它们各自会给出自己的消息，见下文。

### TLS

在**网络**页面，**SSL 模式**决定检查什么（见[连接](connect.md#tls)）。

- 拒绝未加密连接的服务器需要 `require` 或更高；证书不被你的电脑信任的服务器，需要 `verify-ca` 或 `verify-full` 并提供 **CA 证书 (PEM)**，或用 `require` 只加密而不检查。
- "invalid certificate" 和 "invalid root certificates" 表示 PEM 文件无法读取：检查它是证书本身，而不是密钥。
- "a client certificate needs both the certificate and its key"：请同时填写**客户端证书 (PEM)** 和**客户端密钥（PEM）**。"invalid client certificate or key" 表示两者不匹配，或不是 PEM。
- Redis、Elasticsearch 和 Cassandra 在 SSL 模式为 `require` 或更高时启用 TLS。

### SSH

错误以 `SSH <user>@<host>:<port>:` 开头。

- `no answer within 10 s` 或 `cannot reach <host>:<port>`：从这里无法访问 SSH 服务器（或它前面的代理）。
- `cannot read the key <path>`：**私钥文件**不存在、无法读取，或需要口令。
- `no SSH agent is running`：启动你的 SSH 代理，或改用密钥文件或密码。
- 密码或密钥被拒绝是服务器的回答：检查 **SSH 用户**和验证方式。
- 自第一次连接以来密钥发生变化的服务器会被拒绝。密钥来自 `~/.ssh/known_hosts`，与 OpenSSH 共用：如果变化是预期的，请删除其中的旧行。

### 代理

错误会指明代理，例如 `SOCKS5 proxy host:1080: …` 或 `HTTP proxy host:3128: …`。

- `no answer within 10 s`：代理没有响应。
- `the proxy asks for a user and password, or refused them`：把它们写进 URL（`http://user:password@host:3128`）。
- `cannot reach <host>:<port>: …`：代理有响应，但无法到达数据库。
- 无法识别的 URL 会报告为 `proxy "<url>": …`。端口转发要单独使用：请移除 SSH 隧道或代理。

### 钥匙串

- `keychain error: …`，或 "the system has no keychain to save the password in"：在 Linux 上必须运行 Secret Service，例如 GNOME Keyring 或 KWallet。没有的话，请设置主密码（设置 › **主密码**），或为密码选择**每次询问**。
- “此连接未保存密码：密码将保存在钥匙串中。”只是一个提示。
- 从其他工具导入而没有带上密码的情况（例如使用自己主密码的 DBeaver），会在首次使用时询问密码。

### 主密码

- “PlatypusDB 已锁定”或“请输入主密码以使用它们”：输入主密码并点击**解锁**。空闲锁定在设置 › **安全**；生产连接在使用时会重新连接。
- 它无法找回。如果丢失，已保存的密码也就丢失了：请在每个连接中重新设置密码。
- 要回到钥匙串，设置 › **主密码** › **关闭…**（会要求输入当前的主密码）。

## 驱动下载失败

DuckDB、Snowflake、BigQuery、Databricks、Trino、其他 ADBC 驱动、Oracle 和 IBM Db2 在首次使用时下载它们的库。弹窗显示“正在下载 `<name>` 驱动…”和一个**取消**按钮，失败时显示带有**重试**的消息。

- `the download failed (check the internet connection)` 或 `cannot reach the driver registry`：无法连到注册表；检查网络、代理或防火墙，然后点击**重试**。
- `the downloaded driver is corrupted (its SHA-256 does not match)`：文件在传输中被改动或被截断；请重试，如果问题持续，请反馈。
- `no <name> driver for this platform`，或“PlatypusDB 可在 x64 和 ARM 处理器的 Windows、macOS 和 Linux 上下载 DuckDB；当前系统不支持。”：没有适用于你系统的版本。
- `the driver's license (…) does not allow downloading it here`：应用不会下载许可证禁止下载的驱动。
- IBM Db2 目前只在 Windows 上通过 ODBC 连接；注册它的驱动需要管理员权限（用户帐户控制）。请选择“是”，否则驱动不会被注册。
- **取消**后表单仍可使用；在**测试连接**或第一次打开时会再次提示。

设置 › **驱动**列出已下载的内容、移除驱动，并显示系统的 ODBC 驱动。Windows 拒绝的移除操作会在下次启动时完成。

## 代理看不到连接

按下面的清单逐项排查。

1. **访问已关闭。** 代理回答 "`<name>` is not available to agents"。在设置 › **代理** › **安全**中，把该连接设为**只读**或**读写**。来自共享项目的连接在你的电脑上起初是**关闭**。
2. **主密码。** 代理回答 "the saved password of `<name>` is locked by PlatypusDB's master password, which agents cannot enter"。请为该连接配置密码命令或 `.pgpass`，或关闭主密码。
3. **每次询问密码。** 代理回答 "`<name>` asks for its password at each connection, and agents cannot type it"。请把密码保存在钥匙串中，或使用 `.pgpass` 或密码命令。
4. **写入被拒绝。** 生产连接和只读连接最多只能是只读；其他连接的更改会等待你在应用中回答（[代理](agents.md)）。
5. **客户端没有重启**（在点击**安装**之后），或条目缺失：设置 › **代理** › **配置**会说明 PlatypusDB 是否在客户端的文件中。
6. **便携版或另一个配置文件。** 代理读取的是它启动时所用配置文件的连接；用已安装的应用配置的客户端看不到便携版的 `data` 文件夹。

## 便携版

- 它是一个可执行文件，旁边有一个空的 `data` 文件夹。如果启动时找不到这个文件夹，应用会改用系统的文件夹，所以重命名过的 `data` 文件夹看起来就像连接丢失了：把它改回原名。
- 所有内容都在 `data` 中，包括日志；这个文件夹随可执行文件一起移动。已保存的密码留在这台电脑的系统钥匙串中，除非主密码把它们也保存在该文件夹中：在另一台电脑上，要么先设置主密码，要么重新输入密码。
- 它不会自动更新：用新的可执行文件替换它。欢迎界面在版本号旁边显示 `portable`。

## 数据和日志的位置

应用把连接和设置（`connections.toml`、`settings.toml`，以及使用主密码时的 `secrets.vault`）放在它的配置文件夹中，把历史、脚本、已下载的驱动和日志放在它的数据文件夹中，两者都叫 `dev.platypusdb.desktop`：

| 系统 | 配置文件夹 | 数据文件夹 |
|---|---|---|
| Windows | `%APPDATA%\dev.platypusdb.desktop` | `%APPDATA%\dev.platypusdb.desktop` |
| macOS | `~/Library/Application Support/dev.platypusdb.desktop` | 相同 |
| Linux | `~/.config/dev.platypusdb.desktop` | `~/.local/share/dev.platypusdb.desktop` |

便携版把所有内容保存在它的 `data` 文件夹中，环境变量 `PLATYPUS_DATA_DIR` 则会把所有内容放进它所指定的文件夹。

![设置，诊断页面](../../../../screenshots/guide/diagnostics.png)

日志是数据文件夹中的 `logs/platypus.log`：每次启动、错误和崩溃各占一行，达到 1 MB 时轮换，保留两个旧文件（`platypus.1.log`、`platypus.2.log`）。设置 › **诊断**显示该文件夹（**复制**）和**保存诊断包…**，这是一个 zip，包含版本、系统、不含密码的设置和日志，可附在错误报告中。不会向任何地方发送任何内容；分享之前请先阅读日志，因为错误中可能出现服务器或表的名称。崩溃之后，下次启动时会提供同样的诊断包。

## 更新

- 启动时应用会检查是否有更新的版本，并显示“PlatypusDB `<version>` 已可用”，带有**稍后**和**安装并重启**。
- 便携版只会提示：请替换它的可执行文件。通过包管理器（Flatpak、AUR）安装的版本也会提示，并把更新交给包管理器。
- 已下载的驱动单独更新：设置 › **驱动** › **检查更新**，或设置项**启动时检查驱动更新**（开启，最多每天一次）和**自动安装驱动更新**（关闭）。内置驱动随应用更新。

## 常见问题

**PlatypusDB 能离线使用吗？** 可以，配合本地数据库。下载（驱动、更新）需要网络。

**在哪里报告错误？** 在[问题跟踪页面](https://github.com/platypus-labs/platypus-db/issues)，如果可以，请附上诊断包。

**快捷键在哪里？** 见 [SHORTCUTS.md](../../../SHORTCUTS.md)，要更改它们，请到设置 › 键盘快捷键。
