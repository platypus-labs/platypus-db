// CONN-17 (low). A wrong password typed at the connection's prompt closes the prompt and shows
// a toast ("password authentication failed"); the user has to expand the connection again to
// get another try. DBeaver asks again with the error in the dialog.
// Steps: connection that asks for its password; expand; type a wrong password; Connect.
// Expected: the prompt stays (or comes back) with the error, ready for another try.
// Actual: it closes; the navigator shows the connection collapsed.
// Skipped when PostgreSQL is not running.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { fill, needsPort, treeRow } from "../conn-helpers.mjs";

export const skip = needsPort(55432, "PostgreSQL");

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000000f1"
name = "QA pg ask"
driver = "postgres"
host = "localhost"
port = 55432
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = false
`,
  );
}

export default async function (page) {
  const field = `document.querySelector('.modal input[type=password]')`;
  await page.waitFor(`${treeRow("QA pg ask")} !== undefined`);
  await page.eval(`${treeRow("QA pg ask")}.querySelector(".twisty").click()`);
  await page.waitFor(`${field} !== null`);
  await page.eval(fill(field, "wrong-password"));
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
  // The answer: a toast, or the prompt with the error.
  await page.waitFor(`document.querySelector('.toast') !== null || document.querySelector('.modal .error, .modal .notice.error') !== null`, 15_000);
  assert.equal(await page.eval(`${field} !== null`), true, "the prompt closed after the wrong password");
}
