// qa4-ana2-5: a parameter value with leading zeros is put in the SQL as a number, so the
// notebook (and the editor) filters on another value than the one typed.
// Steps: SQLite table codes(code TEXT) holding '007', '7' and '0042'; notebook cell
//   SELECT code FROM codes WHERE code = :c AND length(code) = 3; give c the value 007; run.
// Expected: one row, 007 (the text typed): "1 row".
// Actual: "0 rows": the statement runs with code = 007 as the number 7 (it matches the row 7, not
//   007); the same happens with 0042, zip codes, product codes.
// Code: apps/desktop/src/editor/literals.ts literal(): the numeric test /^[-+]?(\d+\.?\d*)...$/
//   accepts "007" and writes it unquoted; a number with leading zeros (not "0", "0.5") is a code.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { setField } from "../plans.mjs";
import { saveConnections, sqliteEntry } from "../perf.mjs";
import { newNotebook, run, tab, typeIn, statusOf } from "../notebook-helpers.mjs";

const NAME = "Ana2 NB param";
export function setup({ dir }) {
  const file = join(dir, "p.db");
  const db = new DatabaseSync(file);
  db.exec(`CREATE TABLE codes (code TEXT); INSERT INTO codes VALUES ('007'), ('7'), ('0042');`);
  db.close();
  saveConnections(dir, sqliteEntry(NAME, file));
}

export default async function (page) {
  await newNotebook(page, NAME);
  await typeIn(page, "c1", "SELECT code FROM codes WHERE code = :c AND length(code) = 3");
  await run(page, "c1");
  await page.waitFor(`${tab}.querySelector('[data-param=c]') !== null`);
  await setField(page, ".notebook-tab [data-param=c]", "007");
  await run(page, "c1");
  const status = await page.eval(statusOf("c1"));
  assert.match(status, /^1 row/, `only the row 007 matches, the cell reads: ${status}`);
}
