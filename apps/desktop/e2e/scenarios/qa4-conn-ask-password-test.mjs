// QA4-CONN-11 (medium). Test connection in the form of a connection whose password source is
// "Ask every time" (and has no Password field then) answers "invalid configuration: password
// missing": the connection works when expanded (the app asks for the password), but the form
// cannot test it and calls it misconfigured.
// Steps: edit a PostgreSQL connection saved with "Ask every time"; Test connection.
// Expected: the same password prompt as when connecting (then "Connected: …"), or at least a
// message that says it asks each time. Actual: "invalid configuration: password missing".
// Code: apps/desktop/src/components/ConnectionDialog.tsx `test()` (no password prompt for the
// "ask" source; the navigator's connect path has it).
// Skipped when PostgreSQL is not running.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { button, dialog, editConnection, needsPort } from "../conn-helpers.mjs";

export const skip = needsPort(55432, "PostgreSQL");

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000000f1"
name = "QA ask"
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
  await editConnection(page, "QA ask");
  await page.eval(`${button(dialog, "Test connection")}.click()`);
  const asked = `document.querySelector('.modal input[type=password]') !== null && !document.querySelector('.connection-dialog input[type=password]')`;
  const notice = `(document.querySelector('.connection-dialog .notice.error')?.textContent ?? '')`;
  await page.waitFor(`${asked} || ${notice} !== ''`, 15_000);
  const message = await page.eval(notice);
  assert.doesNotMatch(message, /password missing/, `Test connection of an "ask every time" connection: ${message}`);
}
