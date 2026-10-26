// SQL on results: "Query with SQL" in the grid's menu copies the rows into the table `result`
// of an in-memory DuckDB, queried in a new tab; after the source tab runs again, "Reload from
// result" copies them again and runs the query once more.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const NAME = "E2E sql on results";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const statusSpan = `${visibleTab}?.querySelector('.statusbar span')`;
const status = `(${statusSpan}?.textContent ?? '')`;
const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const menuButton = (label) =>
  `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === ${JSON.stringify(label)})`;
const tab = (start) => `[...document.querySelectorAll('.tab')].find(t => t.textContent.startsWith(${JSON.stringify(start)}))`;

export function setup({ dir }) {
  const path = join(dir, "results.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY, grp INTEGER, name TEXT);
           INSERT INTO items VALUES (1, 1, 'c'), (2, 2, 'b'), (3, 1, 'a'), (4, 2, 'd'), (5, 1, 'e');`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000033"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

async function afterRun(page, action) {
  await page.eval(`(() => { const s = ${statusSpan}; if (s) s.textContent = ''; })()`);
  await action();
  await page.waitFor(
    `(() => { const s = ${status}; return ${cancel}?.disabled === true && s !== '' && !s.includes('Executing'); })()`,
    15_000,
  );
}

async function runQuery(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await afterRun(page, () => page.key("Enter", "Enter", 13, CTRL));
}

export default async function (page, { dir }) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow("items")} !== undefined`);
  await page.eval(`${treeRow("items")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.waitFor(`${status} === '5 rows'`, 15_000);

  // From the grid's menu: a new tab on the copied rows.
  const grid = await page.eval(`(() => { const r = ${visibleTab}.querySelector('.grid-scroll').getBoundingClientRect(); return { x: r.x, y: r.y }; })()`);
  await page.click(grid.x + 40, grid.y + 52, { button: "right" });
  await page.waitFor(`${menuButton("Query with SQL")} !== undefined`);
  await page.eval(`${menuButton("Query with SQL")}.click()`);
  await page.waitFor(`${tab("SQL on results")}?.classList.contains('active') === true`, 15_000);
  await page.waitFor(`${editorText} === 'SELECT * FROM result'`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await afterRun(page, () => page.key("Enter", "Enter", 13, CTRL));
  assert.equal(await page.eval(status), "5 rows");
  await runQuery(page, "SELECT grp, count(*) AS n FROM result GROUP BY grp ORDER BY grp");
  assert.equal(await page.eval(status), "2 rows");

  // A new row read by the source tab, then Reload from result: the query runs again on it.
  const db = new DatabaseSync(join(dir, "results.db"), { timeout: 5_000 });
  db.exec("INSERT INTO items VALUES (6, 3, 'f')");
  db.close();
  await page.eval(`${tab("items")}.click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.viewer-bar') != null`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await afterRun(page, () => page.key("Enter", "Enter", 13, CTRL));
  assert.equal(await page.eval(status), "6 rows");
  await page.eval(`${tab("SQL on results")}.click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.result-source') != null`);
  await page.eval(`[...${visibleTab}.querySelectorAll('.result-source button')].find(b => b.textContent === 'Reload from result').click()`);
  await page.waitFor(`${status} === '3 rows'`, 15_000);
}
