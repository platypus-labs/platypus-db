// QA4 grid2 libSQL (same as the SQLite scenario, medium: a normal edit is refused). SQLite types a value by what is stored,
// and the grid types a column of a result from the values it holds (crates/platypus-driver-sqlite/
// src/columns.rs and the libSQL driver, Seen::kind): a column declared DECIMAL(10,2) / NUMERIC whose rows all hold whole
// numbers (SQLite keeps 10 as an INTEGER there) is a whole-number column for the editor, so a
// decimal price cannot be typed into it. The same goes for a column with no declared type.
// Steps: (libSQL server, port 58080) table q4g_prices(id INTEGER PRIMARY KEY, price DECIMAL(10,2)) with the rows 10 and 20;
// open its data; F2 on the first price, type 9.99, Enter, Save.
// Expected: price becomes 9.99 (SQLite's NUMERIC affinity stores a real). Actual: the edit is
// refused, "price: '9.99' is not a whole number", nothing can be saved.
// Code: ValueKind::Integer comes from the Int64 Arrow type that columns.rs infers from the
// stored values; for NUMERIC affinity (and no declared type) the declared type should keep
// the column numeric (Float) or the editor should accept decimals.
import { assert, conns, editBar, focusCell, libsql, needsPorts, openData, openPath, save, setCell } from "../qa-dbs.mjs";

export const skip = needsPorts(58080);

export function setup({ dir }) {
  libsql("DROP TABLE IF EXISTS q4g_prices", "CREATE TABLE q4g_prices (id INTEGER PRIMARY KEY, price DECIMAL(10,2))", "INSERT INTO q4g_prices VALUES (1, 10), (2, 20)");
  conns(dir, [["QA libSQL", "libsql"]]);
}

export default async function (page) {
  try {
  await openPath(page, ["QA libSQL", "main", "Tables", "q4g_prices"]);
  await openData(page, "q4g_prices");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await focusCell(page, 0, 1);
  await setCell(page, "9.99");
  const result = await save(page);
  assert.doesNotMatch(result, /not a whole number/, result);
  // The HTTP API gives the rows as arrays of plain values.
  assert.deepEqual(libsql("SELECT price, typeof(price) FROM q4g_prices WHERE id = 1"), [[9.99, "real"]]);
  } finally {
    libsql("DROP TABLE IF EXISTS q4g_prices");
  }
}


