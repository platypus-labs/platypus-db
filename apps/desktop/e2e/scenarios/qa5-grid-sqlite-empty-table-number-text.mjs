// BUG QA5-GRID-01 (medium). In a SQLite table with no rows (or whose rows hold NULL in the column)
// the data editor takes any text in a column declared NUMERIC or DECIMAL(10,2): typing the Italian
// "1,5" in a new row saves the TEXT '1,5' (typeof text); the same cell is refused as "not a
// number" as soon as another row holds a number there, and a REAL column is always checked.
// Steps: SQLite table `CREATE TABLE prod (id INTEGER PRIMARY KEY, price NUMERIC, qty INTEGER, cost REAL, dec DECIMAL(10,2))`
// with no rows; open its data; Insert (new row); type 1,5 in price and 3,5 in dec; Save.
// Expected: "price: '1,5' is not a number", nothing saved.
// Actual: the row is saved with price = '1,5' and dec = '3,5' as text; sums and sorts on the
// columns are then wrong.
// Code: crates/platypus-driver-sqlite/src/columns.rs, `affinity()` knows INT, CHAR/CLOB/TEXT, BLOB
// and REAL/FLOA/DOUB only: with all-NULL values a NUMERIC/DECIMAL column falls back to Text, and
// crates/platypus-sql/src/dml.rs `ValueKind::of_field` then takes the value unchecked.
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { assert, connections, editBar, expand, focusCell, openData, save, setCell, visibleTab } from "../qa-grid.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "n.db"));
  db.exec(`CREATE TABLE prod (id INTEGER PRIMARY KEY, price NUMERIC, qty INTEGER, cost REAL, dec DECIMAL(10,2));`);
  db.close();
  connections(dir, [["QA LITE", { sqlite: join(dir, "n.db") }]]);
}

export default async function (page, { dir }) {
  await expand(page, "QA LITE", null, "prod");
  await openData(page, "prod");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("Insert", "Insert", 45);
  await page.waitFor(`${editBar}.includes('1 row')`, 10_000);
  await focusCell(page, 0, 1);
  await setCell(page, "1,5");
  await focusCell(page, 0, 4);
  await setCell(page, "3,5");
  const saved = await save(page);
  const db = new DatabaseSync(join(dir, "n.db"));
  const row = db.prepare("SELECT typeof(price) AS p, typeof(cost) AS c, typeof(dec) AS d FROM prod").all();
  db.close();
  if (process.env.QA5_DBG) console.log("SAVED:", saved, JSON.stringify(row));
  assert.deepEqual(row, [], `a row with text in numeric columns was saved: ${JSON.stringify(row)} (${saved})`);
}
