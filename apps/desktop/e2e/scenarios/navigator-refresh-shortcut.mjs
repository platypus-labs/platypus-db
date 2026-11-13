// Bug NAV (low, UX): the navigator has no keyboard shortcuts for its object actions. F5 on a
// selected node does not refresh it (DBeaver: F5 Refresh, F2 Rename, Delete Drop, F4 Edit);
// Refresh is reachable only through the context menu. None of the navigator.* actions in
// apps/desktop/src/components/Navigator.tsx and objects/actions.ts declares a `shortcut`.
//
// Steps: expand the SQLite connection, select `main`, create a table from another connection
// to the file, press F5.
// Expected: the new table is listed. Actual: nothing happens.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { row, saveConnections } from "../navigator-helpers.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "f5.db"));
  db.exec(`CREATE TABLE first (id INTEGER);`);
  db.close();
  saveConnections(dir, `[[connections]]
id = "00000000-0000-4000-8000-0000000a0046"
name = "QA Lite"
driver = "sqlite"
database = '${join(dir, "f5.db")}'
save_password = false
`);
}

export default async function (page, { dir }) {
  await page.eval(`${row("QA Lite")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("first")} !== undefined`, 15_000);
  const db = new DatabaseSync(join(dir, "f5.db"));
  db.exec(`CREATE TABLE second (id INTEGER);`);
  db.close();
  await page.eval(`${row("main")}.id = 'e2e-main'`);
  await page.clickSel("#e2e-main");
  await page.eval(`document.getElementById('e2e-main').focus()`);
  await page.key("F5", "F5", 116);
  const listed = await page.waitFor(`${row("second")} !== undefined`, 5000).then(() => true, () => false);
  assert.ok(listed, "F5 refreshes the selected schema");
}
