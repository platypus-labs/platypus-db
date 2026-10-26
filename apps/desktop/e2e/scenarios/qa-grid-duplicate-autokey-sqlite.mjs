// GRID-02 (high), SQLite (same on PostgreSQL identity columns, MySQL/MariaDB AUTO_INCREMENT and
// SQL Server IDENTITY). Duplicate row (Ctrl+D) copies a key the database numbers itself when the
// catalog reports no default for it: SQLite's INTEGER PRIMARY KEY (the rowid), PG `GENERATED
// ALWAYS AS IDENTITY`, MySQL AUTO_INCREMENT, SQL Server IDENTITY. Only key columns with a
// default are left to the database (apps/desktop/src/grid/edit/TableEditor.tsx,
// duplicateSelection: `keys.columns[c]?.default != null`), so saving fails: "UNIQUE constraint
// failed" (SQLite), "Duplicate entry '1' for key PRIMARY" (MySQL), "cannot insert a non-DEFAULT
// value into column id" (PG identity), IDENTITY_INSERT error (SQL Server).
// Steps: open `items`, focus the first row, Ctrl+D, save.
// Expected: the copy is inserted with a new id. Actual: the INSERT names id = 1 and fails.
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL, assert, connections, editBar, expand, focusCell, openData, previewSql, save } from "../qa-grid.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "dup.db"));
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);
           INSERT INTO items VALUES (1, 'a'), (2, 'b');`);
  db.close();
  connections(dir, [["QA LITE", { sqlite: join(dir, "dup.db") }]]);
}

export default async function (page, { dir }) {
  await expand(page, "QA LITE", null, "items");
  await openData(page, "items");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await focusCell(page, 0, 1);
  await page.key("d", "KeyD", 68, CTRL);
  await page.waitFor(`${editBar}.includes('1 row changed')`);
  // The copy must not repeat the key of the row it copies.
  assert.doesNotMatch(await previewSql(page), /VALUES \(1, 'a'\)/);
  assert.match(await save(page), /to edit a cell/);
  const db = new DatabaseSync(join(dir, "dup.db"));
  const rows = db.prepare("SELECT id, name FROM items ORDER BY id").all().map((r) => `${r.id}:${r.name}`);
  db.close();
  assert.deepEqual(rows, ["1:a", "2:b", "3:a"]);
}
