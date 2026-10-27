// BUG (high, wrong data): "SQL on the loaded rows" sorts and compares a PostgreSQL `numeric`
// without precision as text. Such a column reaches Arrow as Utf8 (columns.rs of
// platypus-driver-pg: Decimal128 only with a typmod), and the copy of the result in DuckDB keeps
// it VARCHAR (the header says VARCHAR), so ORDER BY val DESC puts 999999.0 before 7500000.0 and
// `val > 100` compares strings. Expressions such as `g * 1.5`, sum(), avg() and numeric
// columns declared without (p, s) are all unconstrained numeric.
//   Steps: SELECT g * 1.5 AS val FROM generate_series(1, 20) g; in the field above the grid:
//   ORDER BY val DESC.
//   Expected: 30.0 first.  Actual: 9.0 first (then 7.5, 6.0, 4.5, 30.0 …).
// Needs PostgreSQL (docker compose -f docker/compose.yml up -d).
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { PG, clipboardAfter, expand, gridAt, needsPort, openEditor, pgEntry, run, saveConnections, visibleTab } from "../perf.mjs";

export const skip = needsPort(PG.port, "PostgreSQL");

export function setup({ dir }) {
  saveConnections(dir, pgEntry());
}

export default async function (page) {
  await expand(page, PG.name, PG.password);
  await openEditor(page, PG.name);
  await run(page, "SELECT g * 1.5 AS val FROM generate_series(1, 20) g");
  const field = `${visibleTab}.querySelector('.result-sql .cm-content')`;
  await page.eval(`${field}.focus()`);
  await page.type("ORDER BY val DESC");
  await page.waitFor(`${field}.textContent === 'ORDER BY val DESC'`);
  await page.key("Escape", "Escape", 27);
  await page.eval(`${field}.focus()`);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`/SQL view/.test(${visibleTab}.querySelector('.result-sql').textContent)`, 30_000);
  await page.waitFor(`/^\\d+ rows?$/.test(${visibleTab}.querySelector('.statusbar span')?.textContent.trim() ?? '')`, 30_000);
  const g = await gridAt(page);
  await page.click(g.x + 80, g.y + 60);
  await page.key("a", "KeyA", 65, CTRL);
  const column = (await clipboardAfter(page, "c", "KeyC", 67, CTRL)).split("\n");
  console.log(`  ORDER BY val DESC: ${column.slice(0, 6).join(", ")} …`);
  assert.equal(column[0], "30.0", "the largest value first");
  assert.deepEqual(column.map(Number), [...column.map(Number)].sort((a, b) => b - a));
}
