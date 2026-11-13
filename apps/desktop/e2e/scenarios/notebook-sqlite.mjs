// The notebook (group M) on SQLite:
// - a text cell renders its Markdown, with no markup taken from it;
// - a SQL cell runs on the connection, shows its rows as a grid or a chart, and its plan;
// - a cell on the results reads c1 in the in-memory DuckDB without running it again;
// - a parameter without value is asked for above and shared by the cells;
// - an error stays in its cell, and "Run all" stops at it;
// - a change without WHERE asks first, and Cancel leaves the rows;
// - client commands and a read-only connection are refused;
// - moving and deleting cells renames them.
// Saved as a .pnb file, opened again, and exported as HTML with its rows, chart and plan.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { setField } from "../plans.mjs";
import { saveConnections } from "../perf.mjs";
import { treeMenu } from "../grid-helpers.mjs";

const NAME = "E2E Notebook";
const LOCKED = "E2E Notebook (read-only)";
const tab = `[...document.querySelectorAll('.notebook-tab')].find(t => t.style.display !== 'none')`;
const cell = (name) => `${tab}?.querySelector('[data-name=${JSON.stringify(name)}]')`;
const cells = `[...${tab}.querySelectorAll('.notebook-cell')].map(c => c.dataset.name ?? 'text')`;
const statusOf = (name) => `(${cell(name)}?.querySelector('[data-field=status]')?.textContent ?? '')`;
const errorOf = (name) => `(${cell(name)}?.querySelector('[data-field=error]')?.textContent ?? '')`;
const click = (scope, action) => `${scope}.querySelector('[data-action=${JSON.stringify(action)}]').click()`;

const entry = (id, name, file, extra = "") =>
  `[[connections]]\nid = "00000000-0000-4000-8000-0000000f000${id}"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${file}'\nsave_password = false\n${extra}`;

export function setup({ dir }) {
  const file = join(dir, "sales.db");
  const db = new DatabaseSync(file);
  db.exec(`CREATE TABLE sales (id INTEGER PRIMARY KEY, region TEXT, amount INTEGER);
    INSERT INTO sales (region, amount) VALUES ('North', 1200), ('South', 860), ('East', 1430), ('West', 540), ('North', 990);
    CREATE TABLE scratch (id INTEGER); INSERT INTO scratch VALUES (1), (2);`);
  db.close();
  saveConnections(dir, entry(1, NAME, file), entry(2, LOCKED, file, "read_only = true\n"));
}

/** Types `sql` as the whole text of the cell `name`. */
async function typeIn(page, name, sql) {
  await page.eval(`${cell(name)}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`[...${cell(name)}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n') === ${JSON.stringify(sql)}`);
  // The completion list would take the next keys.
  await page.key("Escape", "Escape", 27);
}

const resultOf = (name) => `(${cell(name)}?.querySelector('[data-field=status]')?.dataset.result ?? '')`;

/** Runs `action` and waits until the cell `name` has a new result or another error, and no
 *  longer runs: its result's id changes at each run. An error already shown does not count. */
async function ran(page, name, action) {
  const before = await page.eval(resultOf(name));
  const error = await page.eval(errorOf(name));
  await action();
  await page.waitFor(
    `(() => { const e = ${errorOf(name)}; return !${cell(name)}.classList.contains('running') && (${resultOf(name)} !== ${JSON.stringify(before)} || (e !== '' && e !== ${JSON.stringify(error)})); })()`,
    30_000,
  );
}

/** Waits until `file` is written whole, as `done` tells from its text, and returns the text. */
async function written(page, file, done) {
  let text = "";
  for (let i = 0; i < 100; i++) {
    text = existsSync(file) ? readFileSync(file, "utf8") : "";
    if (done(text)) return text;
    await page.sleep(100);
  }
  throw new Error(`${file} not written: ${text.length} characters`);
}

const isJson = (text) => {
  try {
    JSON.parse(text);
    return true;
  } catch {
    return false;
  }
};

const run = (page, name) => ran(page, name, () => page.eval(click(cell(name), "run")));

async function addSql(page, sql) {
  const before = await page.eval(`${tab}.querySelectorAll('.notebook-cell.sql').length`);
  await page.eval(click(tab, "add_sql"));
  const name = `c${before + 1}`;
  await page.waitFor(`${cell(name)}?.querySelector('.cm-content') != null`);
  await typeIn(page, name, sql);
  return name;
}

async function newNotebook(page, connection) {
  const before = await page.eval(`document.querySelectorAll('.notebook-tab').length`);
  await treeMenu(page, connection, "New notebook");
  await page.waitFor(`document.querySelectorAll('.notebook-tab').length > ${before} && ${cell("c1")} != null`);
}

/** Text, a query, its chart and plan, and a cell on its results. */
async function textQueryAndResults(page) {
  // A new notebook has only the SQL cell: the text cell is added.
  await page.eval(click(tab, "add_text"));
  await page.waitFor(`${tab}.querySelector('textarea.notebook-text') !== null`);
  await setField(page, ".notebook-tab textarea.notebook-text", "# Sales\n\nBy **region**, from `sales`.\n\n<script>alert(1)</script>");
  await page.eval(click(`${tab}.querySelector('.notebook-cell.markdown')`, "edit"));
  await page.waitFor(`${tab}.querySelector('.notebook-markdown h1')?.textContent === 'Sales'`);
  assert.equal(await page.eval(`${tab}.querySelector('.notebook-markdown strong')?.textContent`), "region");
  assert.equal(await page.eval(`${tab}.querySelector('.notebook-markdown script')`), null, "no markup from the text");
  assert.match(await page.eval(`${tab}.querySelector('.notebook-markdown').textContent`), /<script>alert\(1\)<\/script>/);

  await typeIn(page, "c1", "SELECT region, amount FROM sales ORDER BY id");
  await run(page, "c1");
  assert.match(await page.eval(statusOf("c1")), /^5 rows/);
  await page.eval(`[...${cell("c1")}.querySelectorAll('.view-toggle button')].find(b => b.textContent === 'Chart').click()`);
  await page.waitFor(`${cell("c1")}.querySelector('.notebook-result svg') !== null`);
  await page.eval(click(cell("c1"), "plan"));
  await page.waitFor(`${cell("c1")}.querySelectorAll('.notebook-plan li').length > 0`, 15_000);
  assert.match(await page.eval(`${cell("c1")}.querySelector('.notebook-plan').textContent`), /sales/i);

  // On the results: c1's rows, grouped in DuckDB.
  const c2 = await addSql(page, "SELECT region, sum(amount) AS total FROM c1 GROUP BY region ORDER BY total DESC");
  await page.eval(`${cell(c2)}.querySelector('[data-field=on_results]').click()`);
  await page.waitFor(`${cell(c2)}.querySelector('[data-field=on_results]').checked`);
  await typeIn(page, c2, "SELECT region, sum(amount) AS total FROM c1 GROUP BY region ORDER BY total DESC");
  await run(page, c2);
  assert.match(await page.eval(statusOf(c2)), /^4 rows/, await page.eval(errorOf(c2)));
}

/** A shared parameter, an error, Run all, the guard before a change, a client command. */
async function paramsErrorsAndGuards(page, dir) {
  const c3 = await addSql(page, "SELECT count(*) AS n FROM sales WHERE amount > :min");
  await run(page, c3);
  assert.match(await page.eval(errorOf(c3)), /:min/);
  await page.waitFor(`${tab}.querySelector('[data-param=min]') !== null`);
  await setField(page, ".notebook-tab [data-param=min]", "1000");
  await run(page, c3);
  assert.equal(await page.eval(errorOf(c3)), "");
  assert.match(await page.eval(statusOf(c3)), /^1 row/);

  const c4 = await addSql(page, "SELECT nope FROM sales");
  await run(page, c4);
  assert.match(await page.eval(`${cell(c4)}.textContent`), /nope/);
  // Run all stops at the failing cell: the one after it keeps no result.
  const c5 = await addSql(page, "SELECT 1 AS one");
  // Run all ends at c4, the last cell it runs: its result is a new one.
  const c4Before = await page.eval(resultOf(c4));
  await page.eval(click(tab, "run_all"));
  await page.waitFor(`${resultOf(c4)} !== ${JSON.stringify(c4Before)} && !${tab}.querySelector('[data-action=run_all]').disabled`, 30_000);
  assert.equal(await page.eval(statusOf(c5)), "");

  // A change without WHERE asks first; Cancel leaves the rows.
  await typeIn(page, c5, "DELETE FROM scratch");
  await page.eval(click(cell(c5), "run"));
  await page.waitFor(`document.querySelector('.modal.guard') !== null`);
  await page.eval(`[...document.querySelectorAll('.modal.guard button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`document.querySelector('.modal.guard') === null && !${cell(c5)}.classList.contains('running')`);
  const db = new DatabaseSync(join(dir, "sales.db"));
  assert.equal(db.prepare("SELECT count(*) AS n FROM scratch").get().n, 2);
  db.close();

  await typeIn(page, c5, "@set x = 1");
  await run(page, c5);
  assert.match(await page.eval(errorOf(c5)), /Client commands/);

  // Deleting the failing cell renames the ones after it.
  await page.eval(click(cell(c4), "delete"));
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`document.querySelector('.modal.confirm button.danger').click()`);
  await page.waitFor(`${cell("c5")} == null`);
  assert.deepEqual(await page.eval(cells), ["c1", "text", "c2", "c3", "c4"]);
  // Moving c4 above c3 swaps their names.
  await page.eval(click(cell("c4"), "up"));
  await page.waitFor(`${tab}.querySelectorAll('.notebook-cell')[3].querySelector('.cm-line')?.textContent === '@set x = 1'`);
}

/** Save as .pnb, open it again, export as HTML. */
async function filesAndExport(page, dir) {
  const file = join(dir, "sales.pnb");
  await page.eval(`window.__platypus.answer('file', ${JSON.stringify(file)})`);
  await page.eval(click(tab, "save"));
  const saved = JSON.parse(await written(page, file, isJson));
  assert.equal(saved.format, "platypus-notebook");
  assert.equal(saved.cells.length, 5);
  assert.deepEqual(saved.params, { min: "1000" });
  await page.waitFor(`[...document.querySelectorAll('[role=tab]')].some(t => t.textContent.includes('sales —'))`);

  const html = join(dir, "sales.html");
  await page.eval(`window.__platypus.answer('file', ${JSON.stringify(html)})`);
  await page.eval(click(tab, "export_html"));
  const page_ = await written(page, html, (text) => text.includes("</html>"));
  assert.match(page_, /<h1>Sales<\/h1>/);
  assert.match(page_, /&lt;script&gt;/, "the text escaped");
  assert.match(page_, /<svg/, "c1's chart");
  assert.match(page_, /<td>West<\/td>/, "c2's rows");
  assert.match(page_, /<code>:min<\/code> = 1000/);

  // Opened again from the file, on the connection.
  await page.eval(`window.__platypus.answer('file', ${JSON.stringify(file)})`);
  const before = await page.eval(`document.querySelectorAll('.notebook-tab').length`);
  await treeMenu(page, NAME, "Open notebook…");
  await page.waitFor(`document.querySelectorAll('.notebook-tab').length > ${before}`);
  await page.waitFor(`${tab}.querySelectorAll('.notebook-cell').length === 5`);
  assert.equal(await page.eval(`${tab}.querySelector('[data-param=min]').value`), "1000");
}

export default async function (page, { dir }) {
  await newNotebook(page, NAME);
  await textQueryAndResults(page);
  await paramsErrorsAndGuards(page, dir);
  await filesAndExport(page, dir);

  // A read-only connection refuses a cell that may change data.
  await newNotebook(page, LOCKED);
  await typeIn(page, "c1", "UPDATE sales SET amount = 0 WHERE id = 1");
  await run(page, "c1");
  assert.match(await page.eval(errorOf("c1")), /read-only/);
}
