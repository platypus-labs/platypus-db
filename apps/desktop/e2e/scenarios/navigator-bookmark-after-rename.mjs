// Bug NAV (low): a bookmark does not follow its object when the object is renamed from the
// navigator's Rename… dialog: the Bookmarks folder keeps the old name, and opening it reads a
// table that no longer exists. Notes and tags of the catalog do follow the rename
// (`renameCatalog` in the action's `after`), bookmarks do not.
//
// Cause: the `object.rename` action in apps/desktop/src/objects/actions.ts updates the catalog
// only; the bookmarks in apps/desktop/src/navigator/prefs.ts keep the old name.
//
// Steps: Add bookmark on table `t`; Rename… `t` to `t2`.
// Expected: the bookmark shows (and opens) `t2`. Actual: it still shows `t`.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { childRow, menu, row, rows, saveConnections } from "../navigator-helpers.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "bm.db"));
  db.exec(`CREATE TABLE t (id INTEGER);`);
  db.close();
  saveConnections(dir, `[[connections]]
id = "00000000-0000-4000-8000-0000000a004a"
name = "QA Lite"
driver = "sqlite"
database = '${join(dir, "bm.db")}'
save_password = false
`);
}

export default async function (page) {
  await page.eval(`${row("QA Lite")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("t")} !== undefined`, 15_000);
  await menu(page, "t", "Add bookmark");
  await page.waitFor(`${childRow("Bookmarks", "t")} !== undefined`);
  // The table row under main (the bookmark is the first row named t).
  await menu(page, "t", "Rename…", 1);
  await page.waitFor(`document.querySelector('.modal.sql-action input') !== null`);
  await page.eval(`(() => { const i = document.querySelector('.modal.sql-action input'); i.focus(); i.select(); })()`);
  await page.type("t2");
  await page.waitFor(`/RENAME TO t2;/.test(document.querySelector('.sql-preview').textContent)`);
  await page.eval(`document.querySelector('.modal.sql-action button[type=submit]').click()`);
  await page.waitFor(`${childRow("main", "t2")} !== undefined`, 10_000);
  await page.sleep(300);
  assert.equal(await page.eval(`${childRow("Bookmarks", "t")} === undefined`), true, "no bookmark of the old name");
  assert.equal(await page.eval(`${childRow("Bookmarks", "t2")} !== undefined`), true, "the bookmark follows the rename");
  assert.ok(await page.eval(`${rows("t2")}.length >= 1`));
}
