// qa4-nav-4 (low). A bookmark of an object stays in the navigator's Bookmarks folder after the
// object is dropped from the navigator's own Drop… dialog (and when it is dropped in the editor).
// Opening it then fails ("no such table", details: "main.t not found"). Bookmarks follow a
// rename (navigator-bookmark-after-rename) but are never removed on a drop.
// Steps: SQLite; Add bookmark on table `t`; Drop… `t`, confirm.
// Expected: the Bookmarks folder no longer lists `t` (or shows it as missing).
// Actual: `t` is still bookmarked; its menu entries all fail.
// Code: apps/desktop/src/objects/actions.ts `object.drop` `after` only reloads the schema; the
//   bookmarks in apps/desktop/src/navigator/prefs.ts (`toggleBookmark`/`renameBookmarks`) have
//   no remove-on-drop counterpart.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { childRow, menu, row, saveConnections } from "../navigator-helpers.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "bm.db"));
  db.exec(`CREATE TABLE t (id INTEGER); CREATE TABLE keep (id INTEGER);`);
  db.close();
  saveConnections(dir, `[[connections]]
id = "00000000-0000-4000-8000-0000000a4004"
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
  await menu(page, "t", "Drop…", 1);
  await page.waitFor(`/DROP TABLE t;/.test(document.querySelector('.modal.sql-action .sql-preview')?.textContent ?? '')`);
  await page.eval(`document.querySelector('.modal.sql-action button[type=submit]').click()`);
  await page.waitFor(`${childRow("main", "keep")} !== undefined && ${childRow("main", "t")} === undefined`, 10_000);
  await page.sleep(500);
  assert.equal(await page.eval(`${childRow("Bookmarks", "t")} === undefined`), true, "the bookmark of the dropped table is gone");
}
