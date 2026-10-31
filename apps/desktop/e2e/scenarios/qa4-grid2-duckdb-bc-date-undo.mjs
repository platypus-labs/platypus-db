// QA4 grid2 DuckDB (medium: the undo of a delete cannot put the row back). A DATE before year 1 is
// shown as "0044-03-15 BC" (Arrow's format) while DuckDB reads and writes "0044-03-15 (BC)"; deleting
// a row with such a date and saving offers an Undo script whose INSERT carries the shown text, which
// DuckDB rejects, so the deleted row cannot be restored (the cell cannot be edited back as shown either).
// Steps: table (id INTEGER PRIMARY KEY, d DATE) with the rows 0044-03-15 (BC) and 2024-01-01; open its
// data; focus row 1, Delete, Save, "Undo script", run the script (Alt+X); SELECT count(*).
// Expected: the script runs and the table has 2 rows again. Actual: "invalid date field format:
// 0044-03-15 BC, expected format is (YYYY-MM-DD)"; 1 row.
// Code (likely): apps/desktop/src-tauri/src/commands/data.rs delete_statements builds the undo INSERT
// from row_text (the display text of the cell, display.rs / arrow_cast) and dml.rs quotes it as a plain
// string literal; BC dates (and any type whose display text is not a DuckDB literal) break.
import { join } from "node:path";
import { ALT, assert, button, connections, copyAll, editBar, focusCell, idle, openData, openEditor, refresh, run, save, statusBar, stubClipboard, visibleTab } from "../qa-grid.mjs";

export function setup({ dir }) {
  connections(dir, [["QA DUCK", { duckdb: join(dir, "q.duckdb") }]]);
}

export default async function (page) {
  await openEditor(page, "QA DUCK", null, null);
  await run(page, "CREATE OR REPLACE TABLE q4g (id INTEGER PRIMARY KEY, d DATE); INSERT INTO q4g VALUES (1, DATE '0044-03-15 (BC)'), (2, DATE '2024-01-01');", true);
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
