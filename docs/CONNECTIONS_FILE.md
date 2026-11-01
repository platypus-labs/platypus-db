# The connections file (`.platypusconn`)

What *Export connections* writes and *Import connections › PlatypusDB* reads. The code is
`crates/platypus-sync/src/export.rs`.

## Plain file

UTF-8 JSON:

```json
{
  "format": "platypusdb-connections",
  "version": 1,
  "projects": [
    {
      "name": "Team",
      "folders": [["Servers"], ["Servers", "Prod"]],
      "connections": [{ "id": "…", "name": "Prod", "driver": "postgres", "host": "db.example.com", "user": "alice", … }],
      "placement": { "<connection id>": ["Servers", "Prod"] }
    },
    { "connections": [ … ] }
  ]
}
```

- A project without `name` holds the connections outside any project, or all of them when
  the export drops projects.
- `folders` lists each folder as its names from the root, parents first; `placement` gives
  the folder of the connections that are in one.
- Each connection has the fields of a shared project's connection (`ShareConnection` in
  `portable.rs`): an allow-list, so a field added later to the saved connection stays out
  until someone adds it there. Never written: passwords in clear files, password commands,
  `.pgpass`, certificate and key paths, keychain ids, secret driver options, the shell
  commands run before connecting and after disconnecting. A network profile is written into
  the connection; profiles themselves are not exported.
- A file with a higher `version` is refused, not guessed at.

## Encrypted file

With a password the JSON above is sealed in the sync container (`crypto.rs`): the magic
`PLATYSYN`, the container version, the Argon2id settings, a salt and a nonce, then the JSON
encrypted with AES-256-GCM; the header is authenticated too. Only an encrypted file may
carry, per project, a `secrets` object:

```json
"secrets": { "<connection id>": { "password": "…", "ssh": "…" } }
```

`ssh` is the password of the connection's SSH server or the passphrase of its key. A wrong
password, or any changed byte, gives an error and no data.

## Reading

The file is untrusted like a shared project's: each connection gets a new id, its
`${env:…}` references are held until the user edits them, it cannot make the ODBC driver
manager load a library, and agents are kept out of it until the user lets them in.
