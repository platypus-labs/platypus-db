// QA4 grid2 DuckDB (medium: a MAP cell cannot be edited as shown). The grid shows a MAP the way
// Arrow prints it, `{k: 1}` (no quotes, a colon), while a STRUCT is shown as a DuckDB literal
// (`{'a': 1}`). The editor sends the typed text as a string cast to MAP, and DuckDB reads only its
// own text form `{k=1}`: the text the grid displays (and the user copies) is refused.
// Steps: a table with a MAP(VARCHAR, INTEGER) column holding {'k': 1}; open its data; F2 on the
// MAP cell, change the shown text `{k: 1}` to `{k: 2}`, Enter, Save.
// Expected: the cell is saved (the text shown for a value can be edited and written back);
// Actual: ERROR: Type VARCHAR with value '{k: 2}' can't be cast to the destination type MAP.
// Code: display.rs renders every column with arrow_cast's ArrayFormatter (maps as `{k: 1}`), and
// the DuckDB driver's edit literal is a plain quoted string; either the display should be a
// MAP literal and the edit should write `MAP {...}`, or the string form should be converted.
import { join } from "node:path";
import { assert, connections, editBar, focusCell, openData, openEditor, previewSql, refresh, run, save, setCell } from "../qa-grid.mjs";

export function setup({ dir }) {
  connections(dir, [["QA DUCK", { duckdb: join(dir, "q.duckdb") }]]);
}

export default async function (page) {
  await openEditor(page, "QA DUCK", null, null);
  await run(page, "CREATE TABLE q4g (id INTEGER PRIMARY KEY, mp MAP(VARCHAR, INTEGER)); INSERT INTO q4g VALUES (1, MAP {'k': 1});", true);
  await refresh(page, "main");
  await openData(page, "q4g");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await focusCell(page, 0, 1);
  await setCell(page, "{k: 2}");
  const result = await save(page);
  assert.doesNotMatch(result, /can't be cast/, result);
}
