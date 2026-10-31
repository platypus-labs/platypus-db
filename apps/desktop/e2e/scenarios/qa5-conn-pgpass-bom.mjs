// QA5-CONN-2 (low). A `.pgpass` / pgpass.conf saved with a UTF-8 byte-order mark (PowerShell 5
// `Out-File`/`>`, Notepad's "UTF-8 with BOM"; libpq skips it) never matches its first line:
// the connection says "no line of ... matches localhost:55432:platypus:platypus".
// Steps: pgpass.conf = BOM + `localhost:55432:platypus:platypus:platypus` as its only line;
//   a PostgreSQL connection with "Password from: .pgpass"; Test connection.
// Expected: Connected. Actual: "no line of ...pgpass.conf matches ...".
// Code: crates/platypus-store/src/auth.rs `pgpass_password` (`read_to_string` keeps U+FEFF,
//   so the host field of the first line is "\u{feff}localhost").
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { dialog, editConnection, needsPort, testConnection } from "../conn-helpers.mjs";

export const skip = needsPort(55432, "PostgreSQL");

export function setup({ dir }) {
  writeFileSync(join(dir, "pgpass.conf"), "\uFEFFlocalhost:55432:platypus:platypus:platypus\r\n");
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000a5102"
name = "QA5 pgpass bom"
driver = "postgres"
host = "localhost"
port = 55432
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = false
pgpass = true
`,
  );
}

export const env = ({ dir }) => ({ PGPASSFILE: join(dir, "pgpass.conf") });

export default async function (page) {
  await editConnection(page, "QA5 pgpass bom");
  const notice = await testConnection(page);
  assert.match(notice, /^Connected/, notice);
}
