<!-- source: fd6d4b4d7dd6 -->
[← 用户指南](README.md)

# 快速入门

安装 PlatypusDB，打开一个连接，并学会几个承担大部分工作的快捷键。所有功能按区域分类，见 [FEATURES.md](../FEATURES.md)。

![欢迎界面](../../screenshots/guide/welcome.png)

## 安装

从[发布页面](https://github.com/platypus-labs/platypus-db/releases/latest)下载适合你系统的安装程序：Windows 用 `.exe` 或 `.msi`，macOS 用 `.dmg`，Linux 用 `.AppImage`、`.deb` 或 `.rpm`。应用会自动更新（见[更新](troubleshooting.md#更新)）。

**便携版**：把可执行文件放进一个文件夹，旁边放一个空的 `data` 文件夹。连接、设置、历史和脚本就会保存在 `data` 中，这个文件夹可以移到另一台电脑或 U 盘上。

## 第一个连接

1. 打开 PlatypusDB，点击**新建连接**，或粘贴连接 URL（`postgres://…`、`mysql://…`、JDBC URL 或 ADO.NET 字符串）。
2. 在标志网格中选择数据库。如果它需要尚未安装的驱动（Snowflake、BigQuery、Databricks 和 Trino、DuckDB、Oracle、IBM Db2），应用会提示立即下载；点击**取消**则保持表单不变。
3. 填写表单，点击**测试连接**，然后点击**保存**。
4. 双击连接打开 SQL 编辑器，或展开它来浏览。

表单的说明见[连接](connect.md)；每种数据库在[数据库](databases/README.md)中都有自己的页面。

手边没有数据库？点击欢迎界面上的**试用示例数据库**：一个关于鸭嘴兽野外调查的 SQLite 数据库，带有关联的表、一个视图和几个已保存的脚本。

## 最先要学的快捷键

每个操作、连接和表，在命令面板中敲几个字母就能找到。

| 快捷键 | 操作 |
|---|---|
| `Ctrl+K` | 命令面板 |
| `Ctrl+Enter` | 运行光标所在的语句（或所选内容） |
| `Alt+X` | 运行脚本 |
| `Ctrl+Space` | 自动补全 |
| `Ctrl+Shift+F` | 格式化脚本（或所选内容） |
| `Ctrl+Shift+E` | 解释光标所在的语句 |
| `Ctrl+T` | 新建标签页 |

在 macOS 上用 `Cmd` 代替 `Ctrl`。每个快捷键都可以在“设置 › 键盘快捷键”中更改；完整列表见 [SHORTCUTS.md](../SHORTCUTS.md)。

## 下一步

[运行查询](queries.md)、[编辑数据](edit-data.md)，或在连接生产环境之前先阅读[安全](safety.md)。
