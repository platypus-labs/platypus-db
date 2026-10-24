// Group E on a throwaway SQLite database: the explain tab opened from the editor, and its
// tree, text and table views.
import assert from "node:assert/strict";
import { CTRL, SHIFT } from "../cdp.mjs";

export { setup } from "./smoke-sqlite.mjs";

const NAME = "E2E SQLite";
const queryTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `${queryTab}?.querySelector('.statusbar span')?.textContent ?? ''`;
const explainTab = `[...document.querySelectorAll('.explain-tab')].find(t => t.style.display !== 'none')`;
const operations = `[...${explainTab}.querySelectorAll('.explain-op')].map(e => e.textContent)`;

async function setEditor(page, sql) {
  await page.eval(`${queryTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`[...${queryTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n') === ${JSON.stringify(sql)}`);
}

async function showView(page, label) {
  await page.eval(`[...${explainTab}.querySelectorAll('.object-sections button')].find(b => b.textContent === ${JSON.stringify(label)}).click()`);
}

export default async function (page) {
  const connectionRow = `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(NAME)})`;
  await page.waitFor(`${connectionRow} !== undefined`);
  await page.eval(`${connectionRow}.id = 'e2e-connection'`);
  await page.clickSel("#e2e-connection", { clickCount: 2 });
  await page.waitFor(`${queryTab} !== undefined`);

  await setEditor(page, "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT)");
  await page.eval(`(() => { const s = ${queryTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`(() => { const s = ${status}; return s !== '' && !s.includes('Executing'); })()`, 15_000);

  // Ctrl+Shift+E: the plan of the statement under the cursor, as a tree.
  await setEditor(page, "SELECT * FROM items WHERE name = 'x'");
  await page.key("E", "KeyE", 69, CTRL | SHIFT);
  await page.waitFor(`${explainTab}?.querySelector('.explain-node') != null`, 15_000);
  assert.match(await page.eval(`document.querySelector('.tab.active').textContent`), /^Explain/);
  const ops = await page.eval(operations);
  assert.ok(ops.some((o) => o.startsWith("SCAN items")), `a scan in ${ops}`);

  // The same steps as indented text, and as table rows.
  await showView(page, "Text");
  await page.waitFor(`${explainTab}.querySelector('.explain-text')?.textContent.includes('-> SCAN items')`);
  await showView(page, "Table");
  await page.waitFor(`${explainTab}.querySelector('.explain-table') != null`);
  assert.equal(await page.eval(`${explainTab}.querySelectorAll('.explain-table tbody tr').length`), ops.length);

  // Back in the editor, its context menu offers explain analyze too.
  await page.eval(`[...document.querySelectorAll('.tab')].find(t => !t.textContent.startsWith('Explain')).click()`);
  await page.waitFor(`${queryTab} !== undefined`);
  await page.eval(`${queryTab}.querySelector('.cm-content').id = 'e2e-editor'`);
  const r = await page.rect("#e2e-editor");
  await page.click(r.x + 40, r.y + 10, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Explain analyze')).click()`);
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('Explain analyze')`, 10_000);
  await page.waitFor(`${explainTab}?.querySelector('.explain-node') != null`, 15_000);
}
