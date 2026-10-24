// The core loop on a throwaway SQLite database: open an editor from the navigator, run a
// script and a query, use the context menus of the navigator and the grid, change a
// setting and use the tab shortcuts.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";

const NAME = "E2E SQLite";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `${visibleTab}?.querySelector('.statusbar span')?.textContent ?? ''`;
const menuItems = `[...document.querySelectorAll('.context-menu button span:first-child')].map(s => s.textContent)`;

export function setup({ dir }) {
  // The app opens existing SQLite files only; an empty file is an empty database.
  writeFileSync(join(dir, "e2e.db"), "");
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000001"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "e2e.db")}'
save_password = false
`,
  );
}

async function setEditor(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  // Run only once the editor holds the text (a slow runner may lag behind the keys).
  await page.waitFor(`[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n') === ${JSON.stringify(sql)}`);
  // Blank the previous status, so waitDone sees the next run's.
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
}

async function waitDone(page) {
  await page.waitFor(`(() => { const s = ${status}; return s !== '' && !s.includes('Executing'); })()`, 15_000);
  return page.eval(status);
}

async function rightClick(page, selector) {
  const r = await page.rect(selector);
  assert.ok(r, `not found: ${selector}`);
  await page.click(r.x + Math.min(40, r.w / 2), r.y + Math.min(40, r.h / 2), { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  const items = await page.eval(menuItems);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);
  return items;
}

export default async function (page, { dir }) {
  const connectionRow = `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(NAME)})`;
  await page.waitFor(`${connectionRow} !== undefined`);

  // Navigator menu, built from the registered actions.
  await page.eval(`${connectionRow}.id = 'e2e-connection'`);
  assert.deepEqual(await rightClick(page, "#e2e-connection"), [
    "New SQL script",
    "Search objects…",
    "Refresh",
    "Disconnect",
    "Navigator filters…",
    "Edit…",
    "Delete…",
  ]);

  // Double-click opens an editor.
  await page.clickSel("#e2e-connection", { clickCount: 2 });
  await page.waitFor(`${visibleTab} !== undefined`);

  await setEditor(page, "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT);\nINSERT INTO t VALUES (1, 'a'), (2, NULL);");
  await page.eval(`[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.textContent.includes('Script')).click()`);
  await waitDone(page);

  // DISTINCT keeps the result read-only: a plain query of one table would be editable.
  await setEditor(page, "SELECT DISTINCT * FROM t ORDER BY id");
  await page.key("Enter", "Enter", 13, CTRL);
  assert.equal(await waitDone(page), "2 rows");

  // Grid menu and shortcuts come from the "grid" actions.
  await page.waitFor(`${visibleTab}.querySelector('.grid-scroll') !== null`);
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').id = 'e2e-grid'`);
  assert.deepEqual(await rightClick(page, "#e2e-grid"), [
    "Copy",
    "Copy with headers",
    "Copy as",
    "Select all",
    "Find…",
    "Value panel",
    "Record view",
    "Count by value",
    "Column metadata",
    "Calculate",
    "Columns",
  ]);
  // The page may not read the clipboard back: record what is written instead.
  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
  await page.eval(`document.getElementById('e2e-grid').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.key("c", "KeyC", 67, CTRL);
  await page.waitFor(`window.__copied !== undefined`);
  assert.equal(await page.eval(`window.__copied`), "1\ta\n2\t");

  // Settings: Ctrl+, opens the dialog; a change lands in settings.toml.
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  const limitInput = `[...document.querySelectorAll('.setting')].find(s => s.textContent.includes('LIMIT of Generate SELECT')).querySelector('input')`;
  await page.eval(`(${limitInput}).focus(); (${limitInput}).select()`);
  await page.type("50");
  await page.key("Enter", "Enter", 13);
  const settingsFile = join(dir, "settings.toml");
  const deadline = Date.now() + 5000;
  let saved = "";
  while (!saved.includes("select_limit = 50") && Date.now() < deadline) {
    await page.sleep(100);
    try {
      saved = readFileSync(settingsFile, "utf8");
    } catch {
      // Not written yet.
    }
  }
  assert.match(saved, /\[navigator\]\s+select_limit = 50/);
  await page.eval(`[...document.querySelectorAll('.modal.settings button')].find(b => b.textContent === 'Done').click()`);

  // Tab shortcuts are global actions.
  const tabCount = `document.querySelectorAll('.tab').length`;
  const before = await page.eval(tabCount);
  await page.key("t", "KeyT", 84, CTRL);
  await page.waitFor(`${tabCount} === ${before + 1}`);
  await page.key("w", "KeyW", 87, CTRL);
  await page.waitFor(`${tabCount} === ${before}`);

  // Workspaces saved by 0.1.0 have no tab kind: they still restore as SQL editors.
  await page.sleep(600); // let the debounced save of the current tabs pass
  // As after an update from 0.1.0: only the old workspace is there.
  await page.eval(`localStorage.removeItem('platypus.workspace.v2');
    localStorage.setItem('platypus.workspace.v1', JSON.stringify({
    tabs: [{ connectionId: '00000000-0000-4000-8000-000000000001', sql: 'SELECT 42', filePath: null, savedSql: null, title: 'legacy' }],
    active: 0,
  }))`);
  await page.send("Page.reload");
  await page.waitFor(`[...document.querySelectorAll('.tab')].some(t => t.textContent.startsWith('legacy'))`);
  assert.match(await page.eval(`${visibleTab}.querySelector('.cm-content').textContent`), /SELECT 42/);
}
