// Bug NAV: on SQLite, a table-editor change that needs a rebuild (here: another column's type)
// silently drops what the design does not hold: a generated column
// `b INT GENERATED ALWAYS AS (a * 2) VIRTUAL` comes back as a plain `b INT` holding the
// copied values (later changes of `a` no longer update it), `COLLATE NOCASE` disappears from
// its column, and a `WITHOUT ROWID` table becomes a rowid table. The dialog only warns that
// triggers are dropped.
//
// Cause: the rebuild in crates/platypus-schema/src/ddl.rs (`rebuild`) recreates the table from
// the TableDesign, which has no generation expression, collation nor table options
// (crates/platypus-schema/src/lib.rs `ColumnDesign`).
//
// Steps: Edit table… on g, change the type of `a` from INT to INTEGER, Save…, Apply.
// Expected: b still generated, c still NOCASE (or a warning before applying). Actual: both lost.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { menu, row, saveConnections } from "../qa-nav-helpers.mjs";
import { cell, confirm, editor, fill, openSave } from "../qa-nav-table-editor.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "rebuild.db"));
  db.exec(`CREATE TABLE g (id INTEGER PRIMARY KEY, a INT, b INT GENERATED ALWAYS AS (a * 2) VIRTUAL, c TEXT COLLATE NOCASE);
           INSERT INTO g (id, a, c) VALUES (1, 5, 'x');`);
  db.close();
  saveConnections(dir, `[[connections]]
id = "00000000-0000-4000-8000-0000000a0043"
name = "QA Lite"
driver = "sqlite"
database = '${join(dir, "rebuild.db")}'
save_password = false
`);
}

export default async function (page, { dir }) {
  await page.eval(`${row("QA Lite")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("g")} !== undefined`, 15_000);
  await menu(page, "g", "Edit table…");
  await page.waitFor(`${editor}?.querySelectorAll('tbody tr').length === 4`, 10_000);
  await fill(page, cell(1, 1), "INTEGER");
  await openSave(page);
  assert.equal(await confirm(page), null);

  const db = new DatabaseSync(join(dir, "rebuild.db"));
  const sql = db.prepare("SELECT sql FROM sqlite_master WHERE name = 'g'").get().sql;
  db.exec("UPDATE g SET a = 7 WHERE id = 1");
  const b = db.prepare("SELECT b FROM g WHERE id = 1").get().b;
  db.close();
  assert.match(sql, /GENERATED ALWAYS AS/i, `table after the rebuild: ${sql}`);
  assert.equal(b, 14, "b follows a");
  assert.match(sql, /COLLATE NOCASE/i, `table after the rebuild: ${sql}`);
}
