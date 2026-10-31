// QA4 grid2 DuckDB (high: the undo of a delete cannot put the row back). Deleting a row that has a
// MAP column and saving offers an Undo script; its INSERT writes the MAP as the text the grid shows,
// '{k: 1}' (Arrow's format), which DuckDB cannot cast to MAP (it reads only {k=1} or MAP {'k': 1}),
// so running the script fails and the deleted row is gone for good.
// Steps: table (id INTEGER PRIMARY KEY, mp MAP(VARCHAR, INTEGER)) with two rows; open its data;
// focus row 1, Delete, Save, "Undo script", run the script (Alt+X); SELECT count(*).
// Expected: the script runs and the table has 2 rows again. Actual: "Type VARCHAR with value '{k: 1}'
// can't be cast to the destination type MAP(VARCHAR, INTEGER)"; 1 row.
// Code (likely): apps/desktop/src-tauri/src/commands/data.rs delete_statements builds the undo
// INSERT from row_text (the display text of the cell, display.rs / arrow_cast) and dml.rs quotes it
// as a plain string literal; MAP (and any type whose display text is not a DuckDB literal) breaks.
import { join } from "node:path";
import { ALT } from "../qa-grid.mjs";
import { assert, button, connections, copyAll, editBar, editorText, focusCell, idle, openData, openEditor, refresh, run, save, statusBar, stubClipboard, visibleTab } from "../qa-grid.mjs";

export function setup({ dir }) {
  connections(dir, [["QA DUCK", { duckdb: join(dir, "q.duckdb") }]]);
}

export default async function (page) {
  await openEditor(page, "QA DUCK", null, null);
  await run(page, "CREATE OR REPLACE TABLE q4g (id INTEGER PRIMARY KEY, mp MAP(VARCHAR, INTEGER)); INSERT INTO q4g VALUES (1, MAP {'k': 1}), (2, MAP {'z': 2});", true);
  await refresh(page, "main");
  await openData(page, "q4g");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await focusCell(page, 0, 1);
  await page.key("Delete", "Delete", 46);
  assert.match(await save(page), /to edit a cell/);
  await page.waitFor(`${statusBar}.includes('Undo script')`, 15_000);
  await page.eval(`${button(visibleTab, "Undo script")}.click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content')?.textContent.includes('INSERT')`, 15_000);
  await page.key("x", "KeyX", 88, ALT);
  await idle(page);
  await stubClipboard(page);
  await run(page, "SELECT count(*) FROM q4g");
  assert.equal(await copyAll(page), "2", "the undo script put the deleted row back");
}
