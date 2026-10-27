// CONN-05 (medium). A connection whose password source is "Saved in the keychain" but that
// has no password saved (saved without typing one, the default of a new connection; switched
// back from "Ask every time", which deleted it; or moved to another machine) never asks for
// it: expanding fails with "invalid configuration: password missing" (PostgreSQL) or
// "Access denied … (using password: NO)" (MySQL), and the only way out is to edit it.
// Steps: PostgreSQL connection with save_password = true and nothing in the keychain;
// expand it in the navigator.
// Expected: the password prompt (as for "Ask every time"), optionally offering to save it.
// Actual: an error toast, no prompt.
// Skipped when PostgreSQL is not running.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { needsPort, treeRow } from "../conn-helpers.mjs";

export const skip = needsPort(55432, "PostgreSQL");

export function setup({ dir }) {
  // An id no keychain entry was ever saved under.
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "7a0e0c3e-5b1d-4c1e-9a55-0000000c0e01"
name = "QA no secret"
driver = "postgres"
host = "localhost"
port = 55432
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = true
`,
  );
}

export default async function (page) {
  await page.waitFor(`${treeRow("QA no secret")} !== undefined`);
  await page.eval(`${treeRow("QA no secret")}.querySelector(".twisty").click()`);
  const outcome = `document.querySelector('.modal input[type=password]') !== null || document.querySelector('.toast') !== null`;
  await page.waitFor(outcome, 15_000);
  const toast = await page.eval(`document.querySelector('.toast')?.textContent ?? null`);
  assert.equal(toast, null, "no prompt, only an error");
  assert.equal(await page.eval(`document.querySelector('.modal input[type=password]') !== null`), true);
}
