// Group C against the dev PostgreSQL: the database level, the object tab, generated SQL
// and the rename and drop dialogs. Skipped when PostgreSQL is not running.
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";
import { setup as pgSetup, skip as pgSkip } from "./dev-postgres.mjs";

export const skip = pgSkip;
export const setup = pgSetup;

const NAME = "E2E PG";
const visibleTab = `[...document.querySelectorAll('.query-tab, .object-tab')].find(t => t.style.display !== 'none')`;
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

async function rightClick(page, label) {
  await page.eval(`${row(label)}.id = 'e2e-target'`);
  const r = await page.rect("#e2e-target");
  await page.eval(`document.getElementById('e2e-target').removeAttribute('id')`);
  await page.click(r.x + 20, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
}

async function choose(page, item) {
  await page.eval(
    `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span').textContent === ${JSON.stringify(item)}).click()`,
  );
}

async function runInEditor(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  // Run only once the editor holds the text (a slow runner may lag behind the keys).
  await page.waitFor(`[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n') === ${JSON.stringify(sql)}`);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`(() => { const s = ${visibleTab}.querySelector('.statusbar span')?.textContent ?? ''; return s && !s.includes('Executing'); })()`, 15_000);
}

export default async function (page, { shots }) {
  const shot = (name) => shots && page.screenshot(`${shots}/navigator-objects-${name}.png`);
  await page.waitFor(`${row(NAME)} !== undefined`);
  await page.eval(`${row(NAME)}.click()`);
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`);
  await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
  await page.type("platypus");
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);

  // PostgreSQL shows the server's databases; the saved one is open.
  await page.waitFor(`${row("postgres")} !== undefined && ${row("demo")} !== undefined`, 15_000);
  await page.eval(`${row("demo")}.click()`);
  await page.waitFor(`${row("orders")} !== undefined`, 15_000);

  // Rows: an SVG icon per kind of object, 24px high, and an icon as the expand arrow.
  assert.equal(await page.eval(`!!${row("orders")}.querySelector('.tree-icon svg')`), true);
  assert.equal(await page.eval(`${row("orders")}.querySelector('.tree-icon').dataset.kind`), "table");
  assert.equal(await page.eval(`${row("postgres")}.querySelector('.tree-icon').dataset.kind`), "database");
  assert.equal(await page.eval(`${row("demo")}.querySelector('.tree-icon').dataset.kind`), "schema");
  assert.equal(await page.eval(`getComputedStyle(${row("orders")}).height`), "24px");
  assert.equal(await page.eval(`${row("orders")}.querySelector('.twisty').textContent`), "");
  assert.equal(await page.eval(`!!${row("demo")}.querySelector('.twisty svg')`), true);

  // Object tab with DDL.
  await rightClick(page, "orders");
  await choose(page, "Open details");
  await page.waitFor(`document.querySelector('.object-tab .object-title strong')?.textContent === 'orders'`);
  await page.waitFor(`[...document.querySelectorAll('.object-sections button')].some(b => b.textContent.startsWith('Foreign keys1'))`, 15_000);
  await page.eval(`[...document.querySelectorAll('.object-sections button')].find(b => b.textContent === 'DDL').click()`);
  await page.waitFor(`/CREATE TABLE demo\\.orders/.test(document.querySelector('.object-ddl .cm-content')?.textContent ?? '')`);
  await shot("ddl");

  // Generated INSERT opens in an editor.
  await rightClick(page, "orders");
  await shot("menu");
  await choose(page, "Generate INSERT");
  await page.waitFor(`/INSERT INTO demo\\.orders/.test(${visibleTab}?.querySelector('.cm-content')?.textContent ?? '')`);

  // Create a table, then rename and drop it through the dialogs.
  const table = `e2e_c_${Date.now()}`;
  await runInEditor(page, `CREATE TABLE demo.${table} (id int PRIMARY KEY)`);
  await rightClick(page, "demo");
  await choose(page, "Refresh");
  await page.waitFor(`${row(table)} !== undefined`, 15_000);

  await rightClick(page, table);
  await choose(page, "Rename…");
  await page.waitFor(`document.querySelector('.modal.sql-action input') !== null`);
  await page.eval(`(() => { const i = document.querySelector('.modal.sql-action input'); i.focus(); i.select(); })()`);
  await page.type(`${table}_r`);
  await page.waitFor(`/RENAME TO ${table}_r;/.test(document.querySelector('.sql-preview').textContent)`);
  await page.eval(`document.querySelector('.modal.sql-action button[type=submit]').click()`);
  await page.waitFor(`${row(`${table}_r`)} !== undefined && ${row(table)} === undefined`, 15_000);

  await rightClick(page, `${table}_r`);
  await choose(page, "Drop…");
  await page.waitFor(`/DROP TABLE demo\\.${table}_r;/.test(document.querySelector('.sql-preview')?.textContent ?? '')`);
  await page.eval(`document.querySelector('.modal.sql-action input[type=checkbox]').click()`);
  await page.waitFor(`/CASCADE;/.test(document.querySelector('.sql-preview').textContent)`);
  await shot("drop");
  await page.eval(`document.querySelector('.modal.sql-action button[type=submit]').click()`);
  await page.waitFor(`${row(`${table}_r`)} === undefined && document.querySelector('.modal.sql-action') === null`, 15_000);

  // Another database opens through its own session.
  await page.eval(`${row("postgres")}.click()`);
  await page.waitFor(`[...document.querySelectorAll('.tree-row')].filter(r => r.querySelector('.tree-label')?.textContent === 'public').length >= 2`, 15_000);
}
