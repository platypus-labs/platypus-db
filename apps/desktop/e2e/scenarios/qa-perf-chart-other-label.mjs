// BUG (low, misleading chart): with more than 50 categories the bars chart gathers the rest in a
// last "Other" bar (crates/platypus-local/src/chart.rs, fold), but the X axis thins its labels
// and "Other" is not among those drawn: the (usually tallest) Other bar stands at the right end
// next to the label of the last real category. On a 1M-row table by name, a 500-billion bar
// looked like "name 999999". The Other bar's label should always be drawn (or the axis say so).
//   Steps: 200 categories, Chart, Bars, X = category, Y = value.
//   Expected: "Other" among the axis labels.  Actual: missing (only the tooltip names it).
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { openEditor, run, saveConnections, sqliteEntry, visibleTab } from "../perf.mjs";

const NAME = "QA perf chart";

export function setup({ dir }) {
  const file = join(dir, "chart.db");
  const db = new DatabaseSync(file);
  db.exec(`CREATE TABLE sales (category TEXT, value INTEGER);
    WITH RECURSIVE s(g) AS (SELECT 1 UNION ALL SELECT g + 1 FROM s WHERE g < 200)
    INSERT INTO sales SELECT 'cat ' || printf('%03d', g), g FROM s;`);
  db.close();
  saveConnections(dir, sqliteEntry(NAME, file));
}

export default async function (page) {
  await openEditor(page, NAME);
  await run(page, "SELECT category, value FROM sales");
  await page.eval(`[...${visibleTab}.querySelectorAll('.view-toggle button')].find(b => b.textContent === 'Chart').click()`);
  await page.waitFor(`${visibleTab}.querySelector('.chart-svg') != null`, 15_000);
  await page.waitFor(`/grouped to fit/.test(${visibleTab}.textContent)`, 15_000);
  const titles = await page.eval(`[...${visibleTab}.querySelectorAll('.chart-svg title')].map(t => t.textContent)`);
  assert.ok(titles.some((t) => t.startsWith("Other")), "there is an Other bar");
  const labels = await page.eval(`[...${visibleTab}.querySelectorAll('.chart-svg text')].map(t => t.textContent)`);
  assert.ok(labels.includes("Other"), `axis labels: ${labels.join(", ")}`);
}
