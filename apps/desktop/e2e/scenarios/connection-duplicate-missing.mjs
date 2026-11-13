// CONN-24 (medium). There is no way to duplicate a connection: its menu has New connection…,
// Copy as URL, Edit…, Delete… but no "Duplicate" (DBeaver: Copy/Paste, DataGrip: Duplicate),
// so a second connection to another database of the same server (or a staging copy of a
// production one, with its SSH tunnel, TLS files and init script) has to be typed again.
// Copy as URL and paste is no substitute: it leaves out the tunnel, the init statements, the
// type, colour and password source.
// Steps: right-click a connection in the navigator.
// Expected: a "Duplicate" entry that opens the form filled in with a new name.
// Actual: none.
import assert from "node:assert/strict";
import { menu, seedSqlite, treeRow } from "../conn-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await page.waitFor(`${treeRow("QA lite")} !== undefined`);
  await page.eval(`${treeRow("QA lite")}.id = 'qa-lite'`);
  const items = await menu(page, "#qa-lite");
  assert.ok(items.some((i) => /^Duplicate/.test(i)), items.join(", "));
}
