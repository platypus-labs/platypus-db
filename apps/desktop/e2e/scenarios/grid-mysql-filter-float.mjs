// BUG QA5-GRID-02 (low). The quick filter "Equal to this value" on a MySQL FLOAT (single
// precision) cell finds no row: the grid shows 0.1 and writes `WHERE f4 = 0.1`, but MySQL compares
// the FLOAT 0.1 (stored as 0.10000000149...) with the DECIMAL literal 0.1 as doubles, so nothing
// matches. The same filter on a DOUBLE or DECIMAL column, and on PostgreSQL `real`, finds the row.
// Steps: MySQL table `qa5_ff (id int primary key, f4 float, f8 double)` with (1, 0.1, 0.1) and
// (2, 0.2, 0.2); open its data; focus the f4 cell of row 1; grid menu > Filter > Equal to this value.
// Expected: 1 row (id 1), as for f8.
// Actual: "0 rows".
// Code: apps/desktop quick filter (the SQL written for the focused value): a FLOAT/REAL value must
// be compared as `f4 = CAST(0.1 AS FLOAT)` (or by a tolerance) on MySQL/MariaDB; the literal of the
// shortest float text is read back as a double there.
import { assert, clearStatus, connections, editBar, editorText, expand, focusCell, gridMenu, idle, menuButton, needs, openData, sh, status } from "../grid-helpers.mjs";

export const skip = needs("mysql");

export function setup({ dir }) {
  sh("mysql", `DROP TABLE IF EXISTS qa5_ff; CREATE TABLE qa5_ff (id int PRIMARY KEY, f4 float, f8 double);
    INSERT INTO qa5_ff VALUES (1, 0.1, 0.1), (2, 0.2, 0.2);`);
  connections(dir, [["QA MY", "mysql"]]);
}

export default async function (page) {
  try {
    await expand(page, "QA MY", "mysql", "qa5_ff");
    await openData(page, "qa5_ff");
    await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
    await focusCell(page, 0, 1);
    await gridMenu(page, "Filter");
    await clearStatus(page);
    await page.eval(`${menuButton("Equal to this value")}.click()`);
    await idle(page, "s.includes('row')", 20_000);
    assert.equal(await page.eval(status), "1 row", `after the filter: ${(await page.eval(editorText)).replace(/\s+/g, " ")}`);
  } finally {
    sh("mysql", "DROP TABLE IF EXISTS qa5_ff");
  }
}

