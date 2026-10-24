// Group D on a throwaway SQLite database: JOIN completion from foreign keys, the formatter,
// line commands and the editor's context menu.
import assert from "node:assert/strict";
import { ALT, CTRL, SHIFT } from "../cdp.mjs";

export { setup } from "./smoke-sqlite.mjs";

const NAME = "E2E SQLite";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `${visibleTab}?.querySelector('.statusbar span')?.textContent ?? ''`;
const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const options = `[...document.querySelectorAll('.cm-tooltip-autocomplete li')].map(li => li.textContent)`;

async function setEditor(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  // Run only once the editor holds the text (a slow runner may lag behind the keys).
  await page.waitFor(`[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n') === ${JSON.stringify(sql)}`);
}

/** Opens completion at the end of `sql` and accepts the option containing `wanted`. */
async function complete(page, sql, wanted) {
  await setEditor(page, sql);
  await page.key(" ", "Space", 32, CTRL);
  await page.waitFor(`${options}.some(o => o.includes(${JSON.stringify(wanted)}))`, 10_000);
  const all = await page.eval(options);
  // CodeMirror ignores keys for 75 ms after the list opens.
  await page.sleep(200);
  // Move to the wanted option, then accept it.
  const index = all.findIndex((o) => o.includes(wanted));
  for (let i = 0; i < index; i++) await page.key("ArrowDown", "ArrowDown", 40);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.cm-tooltip-autocomplete') === null`);
  return page.eval(editorText);
}

export default async function (page) {
  const connectionRow = `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(NAME)})`;
  await page.waitFor(`${connectionRow} !== undefined`);
  await page.eval(`${connectionRow}.id = 'e2e-connection'`);
  await page.clickSel("#e2e-connection", { clickCount: 2 });
  await page.waitFor(`${visibleTab} !== undefined`);

  // DDL refreshes the completion catalog, foreign keys included.
  await setEditor(
    page,
    "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT);\n" +
      "CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers);",
  );
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.eval(`[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.textContent.includes('Script')).click()`);
  await page.waitFor(`(() => { const s = ${status}; return s !== '' && !s.includes('Executing'); })()`, 15_000);

  // After JOIN: related tables with alias and condition. The key names no target column,
  // so the condition uses the primary key of customers.
  assert.equal(
    await complete(page, "SELECT * FROM orders o JOIN ", "customers c ON"),
    "SELECT * FROM orders o JOIN customers c ON o.customer_id = c.id",
  );
  // After JOIN ... ON: the condition with the tables before it.
  assert.equal(
    await complete(page, "SELECT * FROM customers AS cu\nLEFT JOIN orders ON ", "orders.customer_id"),
    "SELECT * FROM customers AS cu\nLEFT JOIN orders ON orders.customer_id = cu.id",
  );

  // Ctrl+Shift+F formats the whole script.
  await setEditor(page, "select id,name from customers where id=1 and name is not null;select 2");
  await page.key("F", "KeyF", 70, CTRL | SHIFT);
  await page.waitFor(`${editorText}.startsWith('SELECT\\n')`);
  assert.equal(
    await page.eval(editorText),
    "SELECT\n  id,\n  name\nFROM\n  customers\nWHERE\n  id = 1\n  AND name IS NOT NULL;\n\nSELECT\n  2",
  );

  // Line commands: duplicate, then move it up past the other line.
  await setEditor(page, "SELECT 1\nSELECT 2");
  await page.key("ArrowDown", "ArrowDown", 40, SHIFT | ALT);
  await page.waitFor(`${editorText} === 'SELECT 1\\nSELECT 2\\nSELECT 2'`);
  await page.key("ArrowUp", "ArrowUp", 38, ALT);
  await page.key("ArrowUp", "ArrowUp", 38, ALT);
  await page.waitFor(`${editorText} === 'SELECT 2\\nSELECT 1\\nSELECT 2'`);

  // The context menu lists the editor actions.
  await page.eval(`${visibleTab}.querySelector('.cm-content').id = 'e2e-editor'`);
  const r = await page.rect("#e2e-editor");
  await page.click(r.x + 40, r.y + 10, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  const items = await page.eval(`[...document.querySelectorAll('.context-menu button span:first-child')].map(s => s.textContent)`);
  for (const item of ["Format SQL", "Select statement", "Duplicate line", "Add cursor below"]) {
    assert.ok(items.includes(item), `${item} in ${items}`);
  }
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Select statement')).click()`);
  await page.waitFor(`document.querySelector('.context-menu') === null`);
}
