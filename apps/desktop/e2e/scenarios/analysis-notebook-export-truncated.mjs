// qa4-ana2-2: the notebook's HTML export cuts a result to 500 rows without saying so.
// Steps: SQLite table of 1200 rows; notebook, a SQL cell "SELECT * FROM big", run it (status
//   "1,200 rows"); Export as HTML.
// Expected: the page lists every row, or says that only the first 500 of 1,200 are shown.
// Actual: the summary line says "1,200 rows" over a table of 500 rows, nothing about the cut:
//   whoever reads the exported page takes it for the whole result (sums by eye, counts).
// Code: apps/desktop/src/notebook/NotebookTab.tsx exportCells (EXPORT_ROWS = 500, fetchRows)
//   and notebook/html.ts cellHtml (summary and table, no "first N rows" note).
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { saveConnections, sqliteEntry } from "../perf.mjs";
import { click, newNotebook, run, tab, typeIn, written } from "../notebook-helpers.mjs";

const NAME = "Ana2 NB export";
export function setup({ dir }) {
  const file = join(dir, "nb.db");
  const db = new DatabaseSync(file);
  db.exec(`CREATE TABLE big (id INTEGER PRIMARY KEY, v TEXT);
    WITH RECURSIVE g(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM g WHERE n < 1200) INSERT INTO big SELECT n, 'row' || n FROM g;`);
  db.close();
  saveConnections(dir, sqliteEntry(NAME, file));
}

export default async function (page, { dir }) {
  await newNotebook(page, NAME);
  await typeIn(page, "c1", "SELECT * FROM big");
  await run(page, "c1");
  const html = join(dir, "export.html");
  await page.eval(`window.__platypus.answer('file', ${JSON.stringify(html)})`);
  await page.eval(click(tab, "export_html"));
  const text = await written(page, html, (t) => t.includes("</html>"));
  const rows = (text.match(/<tr>/g) ?? []).length - 1;
  assert.ok(rows === 1200 || /\b500\b[^<]*(first|only|of)|first 500|only 500/i.test(text), `${rows} rows of 1,200 exported, with no word about the cut`);
}
