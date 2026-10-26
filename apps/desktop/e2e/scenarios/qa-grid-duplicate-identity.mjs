// GRID-02 (high), PostgreSQL / MySQL (SQL Server IDENTITY too, seen by hand): the same bug as
// qa-grid-duplicate-autokey-sqlite.mjs on the keys servers number themselves. Duplicate row
// (Ctrl+D) copies the key of a PG `GENERATED ALWAYS AS IDENTITY`, MySQL `AUTO_INCREMENT` and SQL
// Server `IDENTITY` column, which the catalog reports without a default, so saving fails:
// "cannot insert a non-DEFAULT value into column "id"", "Duplicate entry '1' for key
// 'PRIMARY'", "Cannot insert explicit value for identity column".
// Expected: the copy gets a new key from the database. Actual: the INSERT names id = 1.
import { CTRL, assert, button, visibleTab, connections, editBar, expand, focusCell, needs, openData, refresh, save, sh, treeRow } from "../qa-grid.mjs";

export const skip = needs("pg", "mysql");

export function setup({ dir }) {
  sh("pg", `DROP SCHEMA IF EXISTS qa_grid_dup CASCADE; CREATE SCHEMA qa_grid_dup;
    CREATE TABLE qa_grid_dup.ident (id int GENERATED ALWAYS AS IDENTITY PRIMARY KEY, name text);
    INSERT INTO qa_grid_dup.ident (name) VALUES ('a');`);
  sh("mysql", `DROP TABLE IF EXISTS qa_grid_dup_ai;
    CREATE TABLE qa_grid_dup_ai (id int AUTO_INCREMENT PRIMARY KEY, name varchar(10));
    INSERT INTO qa_grid_dup_ai (name) VALUES ('a');`);
  connections(dir, [["QA PG", "pg"], ["QA MY", "mysql"]]);
}

/** Duplicates the first row of `table` and saves; returns the save's outcome. */
async function duplicate(page, table) {
  await openData(page, table);
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await focusCell(page, 0, 1);
  await page.key("d", "KeyD", 68, CTRL);
  await page.waitFor(`${editBar}.includes('1 row changed')`);
  const outcome = await save(page);
  await page.eval(`${button(visibleTab, "Discard")}?.click()`);
  await page.key("w", "KeyW", 87, CTRL);
  return outcome;
}

export default async function (page) {
  try {
    await expand(page, "QA PG", "pg", "public");
    await refresh(page, "QA PG");
    await page.waitFor(`${treeRow("qa_grid_dup")} !== undefined`, 15_000);
    await page.eval(`${treeRow("qa_grid_dup")}.querySelector('.twisty').click()`);
    const pg = await duplicate(page, "ident");
    await expand(page, "QA MY", "mysql", "qa_grid_dup_ai");
    const my = await duplicate(page, "qa_grid_dup_ai");
    assert.deepEqual(
      {
        pg: sh("pg", "SELECT string_agg(id || ':' || name, ',' ORDER BY id) FROM qa_grid_dup.ident"),
        my: sh("mysql", "SELECT group_concat(concat(id, ':', name) ORDER BY id) FROM qa_grid_dup_ai"),
      },
      { pg: "1:a,2:a", my: "1:a,2:a" },
      `saves: pg ${pg} / mysql ${my}`,
    );
  } finally {
    sh("pg", "DROP SCHEMA IF EXISTS qa_grid_dup CASCADE");
    sh("mysql", "DROP TABLE IF EXISTS qa_grid_dup_ai");
  }
}
