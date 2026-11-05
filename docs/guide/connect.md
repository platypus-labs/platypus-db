[← User guide](README.md)

# Connect to a database

![Choosing the database for a new connection](../screenshots/connections.png)

## From a URL

1. Click **New connection** (or the `+` of the navigator).
2. Paste the URL under the logos: `postgres://`, `mysql://`, `sqlserver://`, `clickhouse://`,
   `libsql://`, `mongodb://`, `redis://`, `rediss://`, `elasticsearch://`, `opensearch://`, a
   JDBC URL or an ADO.NET / ODBC string. The form opens filled in for that database.
3. Put the password in the **Password** field, not in the URL: the form says so if the URL
   holds one.
4. Click **Test connection**, then **Save**.

Or pick the database from the logos (the search on top takes `Enter` for the first match).
The choice sets the driver; **Back** returns to the logos.

## The form

The form has four pages: **General**, **Appearance and safety**, **Network** and **Init**. A
dot after a page's name means it holds settings.

- **General**: host, port, user, database and where the password comes from (below).
- **Appearance and safety**: the environment (Development, Test or Production), a color, the
  **Read-only (warn before modifying data)** box, the boxes that stop exports and data editing,
  and the agents' access. See [Safety](safety.md) and [Agents](agents.md).
- **Network**: SSH tunnel, SSL mode and certificates, proxy, port forward and commands around the
  connection.
- **Init**: a variables file, a default schema or time zone, and statements that run at every
  connect.

## Passwords

In **General**, the password comes from one of:

- **Saved in the keychain**: the system keychain.
- **Ask every time**: typed at each connection. Agents cannot open such a connection.
- **.pgpass file** (PostgreSQL only).
- **Shell command**: a command that prints the password.

Any field can read an environment variable written `${env:NAME}`. Password managers
(1Password, Bitwarden, HashiCorp Vault, AWS Secrets Manager) are listed in the same place when
the database supports them: give the secret's reference. Their command-line tool must be
installed and signed in.

### Master password

A master password keeps the saved passwords in an encrypted file instead of the keychain.

1. Open Settings (`Ctrl+,`) › **Master password**.
2. Click **Set a master password…**, type it twice (8 characters at least) and confirm.
3. To go back to the keychain, click **Turn off…** on the same page. **Lock now** locks at once.

It cannot be recovered: without it the saved passwords are lost. Agents cannot enter it (see
[Troubleshooting](troubleshooting.md#an-agent-does-not-see-a-connection)).

## TLS

On the **Network** page, **SSL mode** takes the libpq values: `disable`, `prefer`, `require`,
`verify-ca` and `verify-full`. `prefer` and `require` encrypt without checking the certificate;
`verify-ca` and `verify-full` check it, so a **CA certificate (PEM)** may be needed. For servers
that authenticate clients by certificate, give the **Client certificate (PEM)** and the **Client
key (PEM)** (PostgreSQL and MySQL).

## SSH tunnel

1. On the **Network** page, tick **Connect through an SSH tunnel**.
2. Fill in **SSH host**, the port and **SSH user**.
3. Authenticate with a password, a **Private key file** (and its passphrase) or the **SSH agent**.
   For a bastion, add a **Jump host** (**Through a jump host first**).
4. Click **Test connection**.

The server's key is checked against `~/.ssh/known_hosts`: an unknown server is learned the first
time, and a key that changed since is refused. A **Network profile** shares one tunnel and SSL setup
between connections.

## Proxy and port forward

Also on **Network**: a SOCKS5 or HTTP proxy (`socks5://user:password@host:1080`,
`http://host:3128`), or a port forward through `kubectl` or an AWS Systems Manager session (the
CLI must be installed and signed in). A port forward goes alone: remove the SSH tunnel or the proxy.

## Next

Details for each database: [Databases](databases/README.md). Cloud sign-in, poolers, imports from
DBeaver, DataGrip and pgAdmin (Db2 connections included), and exporting connections are in
[FEATURES.md](../FEATURES.md#connections). If it does not connect, see
[Troubleshooting](troubleshooting.md#connection-errors).
