// QA4 grid2 SQLite (medium: a normal edit is refused). SQLite types a value by what is stored,
// and the grid types a column of a result from the values it holds (crates/platypus-driver-sqlite/
// src/columns.rs, Seen::kind): a column declared DECIMAL(10,2) / NUMERIC whose rows all hold whole
// numbers (SQLite keeps 10 as an INTEGER there) is a whole-number column for the editor, so a
// decimal price cannot be typed into it. The same goes for a column with no declared type.
// Steps: table prices(id INTEGER PRIMARY KEY, price DECIMAL(10,2)) with the rows 10 and 20;
// open its data; F2 on the first price, type 9.99, Enter, Save.
// Expected: price becomes 9.99 (SQLite's NUMERIC affinity stores a real). Actual: the edit is
// refused, "price: '9.99' is not a whole number", nothing can be saved.
// Code: ValueKind::Integer comes from the Int64 Arrow type that columns.rs infers from the
// stored values; for NUMERIC affinity (and no declared type) the declared type should keep
// the column numeric (Float) or the editor should accept decimals.
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { assert, connections, editBar, expand, focusCell, openData, save, setCell } from "../grid-helpers.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "d.db"));
  db.exec(`CREATE TABLE prices (id INTEGER PRIMARY KEY, price DECIMAL(10,2));
           INSERT INTO prices VALUES (1, 10), (2, 20);`);
  db.close();
  connections(dir, [["QA LITE", { sqlite: join(dir, "d.db") }]]);
}

export default async function (page, { dir }) {
  await expand(page, "QA LITE", null, "prices");
  await openData(page, "prices");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await focusCell(page, 0, 1);
  await setCell(page, "9.99");
  const result = await save(page);
  assert.doesNotMatch(result, /not a whole number/, result);
  const db = new DatabaseSync(join(dir, "d.db"));
  const row = db.prepare("SELECT price FROM prices WHERE id = 1").get();
  db.close();
  assert.equal(row.price, 9.99);
}
