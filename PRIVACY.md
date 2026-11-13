# Privacy policy

PlatypusDB is a desktop application. It has no account, no server of its own and no telemetry:
the maintainers receive nothing from it. This page lists every time the app goes on the network,
and what stays on your computer.

## What stays on your computer

- **Connections, settings, scripts, history and results** are files in the app's data folder (or
  in the `data` folder of a portable copy). **Passwords** are in the system keychain, or in a file
  encrypted with your master password.
- **Logs** of starts, errors and crashes are written to `logs/` in the data folder and are never
  sent. The diagnostic package (Settings › Diagnostics) is a zip you can attach to a bug report
  yourself; look at it before sharing, since error messages may name a server or a table.
- **Agents (MCP)**: the MCP server runs on your computer and answers only the agent that started
  it. What an agent reads from your databases goes to that agent, under the access you set for
  each connection; the agent's own privacy policy applies to what it does with it.

## When the app uses the network

- **Your databases**: the app connects only to the servers, tunnels and proxies you configure.
- **Update check**: each time it starts, the app downloads
  `https://github.com/platypus-labs/platypus-db/releases/latest/download/latest.json` from GitHub
  to see whether a new version is out, and downloads the new version if you install it. GitHub
  sees the request as any download (address, time). Copies installed by a package manager leave
  updates to it.
- **Components downloaded on first use**, when a connection needs them and after you confirm:
  DuckDB's library from DuckDB's releases, ADBC drivers (Snowflake, BigQuery, Databricks, Trino)
  from the driver registry, Oracle Instant Client from Oracle and IBM's ODBC driver from IBM. Each
  download goes to that vendor, under its own terms and privacy policy. The app then looks for new
  versions of the ADBC drivers it downloaded once a day.
- **Plugin catalog**: off until you enter an index address; then the app reads that index when
  you open the catalog, and downloads the plugins you install.
- **What you choose to send**: exports to cloud storage (S3, GCS), project files in a folder you
  sync, links opened in your browser (documentation, bug reports).

Nothing else is sent, and nothing is sent to the maintainers.

## Contact

Questions about privacy: open an issue at
https://github.com/platypus-labs/platypus-db/issues, or for something sensitive use the private
vulnerability report described in [SECURITY.md](SECURITY.md).
