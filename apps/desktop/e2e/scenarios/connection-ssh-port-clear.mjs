// CONN-15 (low). The SSH port field cannot be emptied: deleting its last digit puts 22 back at
// once, so changing 22 to 2222 by deleting and retyping gives 222, 2222... depending on where
// the caret lands, and selecting the field to retype does not clear it either.
// Steps: New connection, PostgreSQL; Network tab; Connect through an SSH tunnel; empty the
// SSH Port field.
// Expected: the field stays empty while editing (22 used when left empty).
// Actual: it shows 22 again immediately.
// Suspected: apps/desktop/src/components/SshFields.tsx (`Number(value) || 22` on change).
import assert from "node:assert/strict";
import { fill, newConnection, seedSqlite } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await newConnection(page, "PostgreSQL");
  await page.eval(`[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('Network')).click()`);
  await page.eval(`document.querySelector('[data-page="network"] .check input').click()`);
  await page.waitFor(`document.querySelector('.ssh-section label.field.port input') !== null`);
  const port = `document.querySelector('.ssh-section label.field.port input')`;
  await page.eval(fill(port, ""));
  await page.sleep(100);
  assert.equal(await page.eval(`${port}.value`), "");
}
