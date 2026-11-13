// Regression guard (passes): query history on DuckDB. Statements of single runs and scripts
// are listed newest first, failed ones marked as errors, the search treats `_` literally, and
// "Insert into editor" appends the statement to the editor's text.
import assert from "node:assert/strict";
import { button, editorText, openEditor, runScript, runStatement, saveConnections, setEditor } from "../editor-helpers.mjs";

export const setup = ({ dir }) => saveConnections(dir, [], { duckdb: true });

const entries = `[...document.querySelectorAll('.history-entry')].map(e => e.querySelector('pre').textContent + ' | ' + (e.classList.contains('error') ? 'error' : 'ok'))`;

export default async function (page) {
  await openEditor(page, "QA DuckDB");
  await runStatement(page, "SELECT 'qa_sql_h1' AS a");
  await runStatement(page, "SELECT qa_sql_h_nosuch");
  await runScript(page, "SELECT 'qa_sql_h2' AS b;\nSELECT 'qa_sqlXh3' AS c;");
  await page.eval(`${button("History")}.click()`);
  await page.waitFor(`document.querySelectorAll('.history-entry').length >= 4`, 10_000);
  assert.deepEqual((await page.eval(entries)).slice(0, 4), [
    "SELECT 'qa_sqlXh3' AS c | ok",
    "SELECT 'qa_sql_h2' AS b | ok",
    "SELECT qa_sql_h_nosuch | error",
    "SELECT 'qa_sql_h1' AS a | ok",
  ]);
  await page.eval(`document.querySelector('.history-search').focus()`);
  await page.type("qa_sql_h2");
  await page.waitFor(`document.querySelectorAll('.history-entry').length === 1`, 5000);
  await setEditor(page, "-- before");
  await page.eval(`document.querySelector('.history-entry button[aria-label="Insert into editor"]').click()`);
  await page.waitFor(`${editorText}.includes('qa_sql_h2')`);
  assert.equal(await page.eval(editorText), "-- before\n\nSELECT 'qa_sql_h2' AS b;\n");
}
