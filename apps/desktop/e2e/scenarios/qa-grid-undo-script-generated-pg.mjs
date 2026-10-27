// GRID-16 (medium), PostgreSQL (SQL Server too: rowversion and computed columns). The undo
// script that a save offers re-inserts deleted rows with every table column, including an
// identity column (`GENERATED ALWAYS AS IDENTITY`) and a generated column, so running it fails:
// "cannot insert a non-DEFAULT value into column "id"" / "...into column "twice"". On SQL
// Server the same script names the rowversion column ("Cannot insert an explicit value into a
// timestamp column") and writes datetime values the server cannot parse (see GRID-07).
// (apps/desktop/src-tauri/src/commands/data.rs, delete_statements: the undo INSERT takes
// `table_columns`, which knows nothing of identity or generated columns.)
// Steps: delete one row, save, open the undo script, run it.
// Expected: the row is back (OVERRIDING SYSTEM VALUE for the identity, generated column left
// out). Actual: the script fails.
import { assert, button, connections, editBar, expand, focusCell, needs, openData, refresh, save, sh, statusBar, treeRow, visibleTab } from "../qa-grid.mjs";

export const skip = needs("pg");

export function setup({ dir }) {
  sh("pg", `DROP SCHEMA IF EXISTS qa_grid_undo CASCADE; CREATE SCHEMA qa_grid_undo;
    CREATE TABLE qa_grid_undo.items (id int GENERATED ALWAYS AS IDENTITY PRIMARY KEY, total numeric(10,2), twice numeric GENERATED ALWAYS AS (total * 2) STORED);
    INSERT INTO qa_grid_undo.items (total) VALUES (1), (2);`);
  connections(dir, [["QA PG", "pg"]]);
}

export default async function (page) {
  try {
    await expand(page, "QA PG", "pg", "public");
    await refresh(page, "QA PG");
    await page.waitFor(`${treeRow("qa_grid_undo")} !== undefined`, 15_000);
    await page.eval(`${treeRow("qa_grid_undo")}.querySelector('.twisty').click()`);
    await openData(page, "items");
    await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
    await focusCell(page, 0, 1);
    await page.key("Delete", "Delete", 46);
    assert.match(await save(page), /to edit a cell/);
    await page.waitFor(`${statusBar}.includes('Undo script')`, 15_000);
    await page.eval(`${button(visibleTab, "Undo script")}.click()`);
    await page.waitFor(`${visibleTab}?.querySelector('.cm-content')?.textContent.includes('INSERT')`, 15_000);
    const script = await page.eval(`${visibleTab}.querySelector('.cm-content').textContent`);
    assert.equal(sh("pg", "SELECT count(*) FROM qa_grid_undo.items"), "1");
    let error = null;
    try {
      sh("pg", script);
    } catch (e) {
      error = String(e);
    }
    assert.equal(error, null, `undo script ${script}`);
    assert.equal(sh("pg", "SELECT count(*) FROM qa_grid_undo.items"), "2");
  } finally {
    sh("pg", "DROP SCHEMA IF EXISTS qa_grid_undo CASCADE");
  }
}
