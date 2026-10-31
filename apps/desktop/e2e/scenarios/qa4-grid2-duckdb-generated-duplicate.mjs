// QA4 grid2 DuckDB (medium: Duplicate row and the delete undo fail on a table with a generated column).
// SQLite and PostgreSQL tables with generated columns are handled (the column is left out of the INSERT
// of a duplicated row and of the undo script), but on DuckDB the catalog does not report
// `GENERATED ALWAYS AS (...)` columns as generated, so the INSERT of a duplicate names them:
// INSERT INTO main.q4g (name, n, c, g) VALUES ('a', 7, ..., 14) -> "Cannot insert into a generated column".
// Steps: table q4g (id INTEGER DEFAULT nextval(...) PRIMARY KEY, name, n INTEGER, g INTEGER GENERATED ALWAYS
// AS (n * 2)) with two rows; open its data; focus the first row, Ctrl+D (duplicate), Save.
// Expected: the copy is inserted (3 rows). Actual: ERROR: Cannot insert into a generated column; the same
// error breaks the Undo script of a deleted row.
// Code (likely): the DuckDB driver's column metadata (duckdb_columns / table info) gives no generated flag,
// so Changes.generated (apps/desktop/src-tauri/src/commands/data.rs) is empty for DuckDB.
import { join } from "node:path";
import { CTRL, assert, connections, copyAll, editBar, focusCell, openData, openEditor, refresh, run, save, stubClipboard } from "../qa-grid.mjs";

export function setup({ dir }) {
  connections(dir, [["QA DUCK", { duckdb: join(dir, "q.duckdb") }]]);
}

export default async function (page) {
  await openEditor(page, "QA DUCK", null, null);
  await run(page, "CREATE SEQUENCE q4s; CREATE OR REPLACE TABLE q4g (id INTEGER DEFAULT nextval('q4s') PRIMARY KEY, name VARCHAR, n INTEGER, g INTEGER GENERATED ALWAYS AS (n * 2)); INSERT INTO q4g (name, n) VALUES ('a', 7), ('b', 8);", true);
  await refresh(page, "main");
  await openData(page, "q4g");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await focusCell(page, 0, 1);
  await page.key("d", "KeyD", 68, CTRL);
  await page.waitFor(`${editBar}.includes('1 row changed')`, 5_000);
  const result = await save(page);
  assert.doesNotMatch(result, /generated column/, result);
  await stubClipboard(page);
  await run(page, "SELECT count(*) FROM q4g");
  assert.equal(await copyAll(page), "3");
}
