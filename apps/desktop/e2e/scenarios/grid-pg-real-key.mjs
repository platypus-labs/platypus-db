// GRID-08 (medium), PostgreSQL. A `real` (float4) value is written back as a bare numeric
// literal, which PostgreSQL compares as double precision: `r = 0.1` matches no float4 0.1
// (0.100000001490116). So:
// 1. a table keyed by a real column (or a virtual key including one) cannot save an edit:
//    "the row was not found; it may have been changed or deleted since it was loaded";
// 2. the quick filter "Equal to this value" on a real column returns no rows.
// (crates/platypus-sql/src/dml.rs, Column::literal: Float values are written bare; a cast to
// the column type, e.g. 0.1::real or CAST('0.1' AS real), would match.)
// Expected: the edit saves and the filter finds the row. Actual: neither.
import { assert, clearStatus, connections, editBar, expand, focusCell, gridMenu, idle, menuButton, needs, openData, refresh, save, setCell, sh, status, treeRow } from "../grid-helpers.mjs";

export const skip = needs("pg");

export function setup({ dir }) {
  sh("pg", `DROP SCHEMA IF EXISTS qa_grid_real CASCADE; CREATE SCHEMA qa_grid_real;
    CREATE TABLE qa_grid_real.f4key (r real PRIMARY KEY, v text);
    INSERT INTO qa_grid_real.f4key VALUES (0.1, 'a');`);
  connections(dir, [["QA PG", "pg"]]);
}

export default async function (page) {
  try {
    await expand(page, "QA PG", "pg", "public");
    await refresh(page, "QA PG");
    await page.waitFor(`${treeRow("qa_grid_real")} !== undefined`, 15_000);
    await page.eval(`${treeRow("qa_grid_real")}.querySelector('.twisty').click()`);
    await openData(page, "f4key");
    await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);

    // The quick filter on the key's value finds its row.
    await focusCell(page, 0, 0);
    await clearStatus(page);
    await gridMenu(page, "Filter");
    const equal = menuButton("Equal to this value");
    await page.waitFor(`${equal} !== undefined && ${equal}.title !== 'Reading the columns of the result…'`);
    await page.eval(`${equal}.click()`);
    assert.equal(await idle(page), "1 row");

    // An edit of the row saves.
    await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
    await focusCell(page, 0, 1);
    await setCell(page, "b");
    assert.match(await save(page), /to edit a cell/);
    assert.equal(sh("pg", "SELECT v FROM qa_grid_real.f4key"), "b");
  } finally {
    sh("pg", "DROP SCHEMA IF EXISTS qa_grid_real CASCADE");
  }
}
