// GRID-03 (medium), PostgreSQL (same on MySQL/MariaDB `AS (...) STORED` and SQL Server computed
// columns). Generated columns are treated as ordinary writable columns: the catalog the editor
// reads (ColumnMeta in apps/desktop/src/api.ts) has no "generated" flag.
// 1. Editing a generated column is accepted in the grid, and saving fails with the database's
//    error ("column "twice" can only be updated to DEFAULT").
// 2. Duplicate row copies the generated column's value, so duplicating any row of such a table
//    fails ("cannot insert a non-DEFAULT value into column "twice"").
// Expected: the generated column is read-only in the grid (like an expression column) and a
// duplicate leaves it out. Actual: both end in a database error.
import { CTRL, assert, button, connections, editBar, expand, focusCell, needs, openData, previewSql, refresh, save, setCell, sh, treeRow, visibleTab } from "../qa-grid.mjs";

export const skip = needs("pg");

export function setup({ dir }) {
  sh("pg", `DROP SCHEMA IF EXISTS qa_grid_gen CASCADE; CREATE SCHEMA qa_grid_gen;
    CREATE TABLE qa_grid_gen.items (id serial PRIMARY KEY, total numeric(10,2), twice numeric GENERATED ALWAYS AS (total * 2) STORED);
    INSERT INTO qa_grid_gen.items (total) VALUES (1), (2);`);
  connections(dir, [["QA PG", "pg"]]);
}

export default async function (page) {
  try {
    await expand(page, "QA PG", "pg", "public");
    await refresh(page, "QA PG");
    await page.waitFor(`${treeRow("qa_grid_gen")} !== undefined`, 15_000);
    await page.eval(`${treeRow("qa_grid_gen")}.querySelector('.twisty').click()`);
    await openData(page, "items");
    await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);

    // The duplicate of a row leaves the generated column (and the serial key) to the database.
    await focusCell(page, 0, 1);
    await page.key("d", "KeyD", 68, CTRL);
    await page.waitFor(`${editBar}.includes('1 row changed')`);
    assert.doesNotMatch(await previewSql(page), /twice/);
    assert.match(await save(page), /to edit a cell/);
    assert.equal(sh("pg", "SELECT count(*) FROM qa_grid_gen.items"), "3");

    // The generated column cannot be edited: no pending change, a message instead.
    await focusCell(page, 0, 2);
    await page.key("F2", "F2", 113);
    await page.sleep(300);
    if (await page.eval(`document.querySelector('.cell-editor') !== null`)) {
      await setCell(page, "100").catch(() => {});
    }
    await page.sleep(300);
    assert.ok(!(await page.eval(editBar)).includes("changed"), `generated column edited: ${await page.eval(editBar)}`);
  } finally {
    await page.eval(`${button(visibleTab, "Discard")}?.click()`).catch(() => {});
    sh("pg", "DROP SCHEMA IF EXISTS qa_grid_gen CASCADE");
  }
}
