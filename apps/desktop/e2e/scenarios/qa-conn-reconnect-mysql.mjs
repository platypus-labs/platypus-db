// CONN-02 (medium). MySQL and MariaDB: when the server closes an idle session (wait_timeout,
// a KILL, a restart), the next statement fails with "Input/output error: … (os error 10053)"
// and only the one after it reconnects. PostgreSQL reconnects on the first statement (see
// qa-conn-ok-reconnect-postgres), as the session's `live()` promises.
// Steps: query tab on MySQL; `SET SESSION wait_timeout = 1`; wait 3 s; run `SELECT 1`.
// Expected: `SELECT 1` reopens the connection and returns 1 row.
// Actual: Error (I/O error); a second run works.
// Suspected: apps/desktop/src-tauri/src/commands/mod.rs `live()` only reconnects when
// `is_closed()` already says so; the MySQL driver learns it only from the failed statement.
// Skipped when MySQL or MariaDB is not running.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { needsPort, openQueryTab, run, visibleTab } from "../conn-helpers.mjs";

const mysqlUp = needsPort(53306, "MySQL");
const mariaUp = needsPort(53307, "MariaDB");
export const skip = async () => (await mysqlUp()) ?? (await mariaUp());

const connection = (id, name, port) => `
[[connections]]
id = "${id}"
name = "${name}"
driver = "mysql"
host = "localhost"
port = ${port}
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = false
password_command = "echo platypus"
`;

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7
${connection("00000000-0000-4000-8000-0000000000e5", "QA mysql", 53306)}
${connection("00000000-0000-4000-8000-0000000000e8", "QA mariadb", 53307)}`,
  );
}

export default async function (page) {
  for (const name of ["QA mysql", "QA mariadb"]) {
    await openQueryTab(page, name);
    assert.equal(await run(page, "SET SESSION wait_timeout = 1"), "0 rows affected");
    // The server drops the idle session.
    await page.sleep(3500);
    const status = await run(page, "SELECT 1 AS one");
    const text = await page.eval(`${visibleTab}.innerText`);
    assert.equal(status, "1 row", `${name}: the first statement after the server closed the session failed: ${text.split("\n").slice(-4).join(" | ")}`);
  }
}
