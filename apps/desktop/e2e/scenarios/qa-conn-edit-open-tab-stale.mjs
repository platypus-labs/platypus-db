// CONN-01 (high). Editing a connection that has an open query tab leaves the tab on its old
// session: after changing the database and making the connection Production, the tab (now
// titled with the new name) still runs on the old database and in autocommit, so the
// production safeguard (manual commit) does not apply to it.
// Steps: PostgreSQL connection on database `platypus`; open a query tab and run a statement;
// Edit the connection: database `postgres`, type Production, Save; run in the same tab
// `SELECT current_database()`.
// Expected: the tab uses the saved settings (database `postgres`, manual commit as every
// production session), or asks to reconnect. Actual: it still runs on `platypus`, autocommit.
// Skipped when PostgreSQL is not running.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { editConnection, expand, fillForm, needsPort, openQueryTab, run, saveForm, treeRow, visibleTab } from "../conn-helpers.mjs";

export const skip = needsPort(55432, "PostgreSQL");

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000000a1"
name = "QA pg edit"
driver = "postgres"
host = "localhost"
port = 55432
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = false
password_command = "echo platypus"
`,
  );
}

/** A statement that fails (division by zero) unless the session is on `db`. */
const onDatabase = (db) => `SELECT 1/(CASE WHEN current_database() = '${db}' THEN 1 ELSE 0 END) AS ok`;

export default async function (page) {
  await expand(page, "QA pg edit");
  await page.waitFor(`${treeRow("public")} !== undefined`, 15_000);
  await openQueryTab(page, "QA pg edit");
  assert.equal(await run(page, onDatabase("platypus")), "1 row");

  await editConnection(page, "QA pg edit");
  await fillForm(page, { Name: "QA pg renamed", Database: "postgres", Type: "prod" });
  await saveForm(page);
  await page.waitFor(`${treeRow("QA pg renamed")}?.querySelector('.type-badge')?.textContent === 'PROD'`);

  // The tab follows the saved connection: the new database...
  assert.equal(await run(page, onDatabase("postgres")), "1 row", "the open tab still runs on the old database");
  // ...and manual commit, as every production session: a SELECT opens a transaction.
  await page.waitFor(`${visibleTab}.querySelector('.toolbar').innerText.includes('Transaction open')`, 5_000);
}
