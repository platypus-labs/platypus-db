# Security policy

PlatypusDB holds the keys to your databases, so security reports are welcome and handled first.

## Supported versions

Only the latest release receives security fixes. The app updates itself, so the fix reaches
users with the next release.

| Version | Supported |
|---|---|
| Latest release | Yes |
| Older releases | No |

## Reporting a vulnerability

Please do not open a public issue. Report it privately through GitHub's
[private vulnerability reporting](https://github.com/platypus-labs/platypus-db/security/advisories/new)
(the repository's **Security** tab, then **Report a vulnerability**).

Include the version, the operating system, the steps to reproduce and what an attacker gains.
We aim to answer within a week. Once a fix is released, the advisory is published with
credit to the reporter, unless you prefer otherwise.

## Scope

In scope:

- **Stored credentials**: passwords in the system keychain, the file encrypted with the master
  password (Argon2id and AES-256-GCM), SSH passwords and keys, client certificates, passwords
  read from `.pgpass`, password commands and imports from other tools.
- **Connection data**: hosts, users or secrets leaking into logs, the history, exports, copied
  URLs or error messages.
- **Agent access (MCP and CLI)**: an agent reading or writing beyond the access set for a
  connection (none, read only, read and write), getting more than read-only access to a
  production or read-only connection, or seeing a host, a user or a secret.
- **Safety checks**: statements that bypass the read-only mode or the confirmations before risky
  statements.
- **Network**: TLS verification (`sslmode`), SSH host key checks, tunnels.
- **Updates and downloads**: the auto-updater and drivers downloaded on first use.
- **Files**: exports, imports, schema documentation and project files (`.dbp`) that run code or
  overwrite files they should not.

Out of scope: vulnerabilities in the database servers themselves, in third-party drivers the app
downloads, or attacks that need an attacker already in control of the user's account.
