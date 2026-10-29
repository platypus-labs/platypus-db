// Group K's windows and panels, on SQLite: a tab opened in a new window keeps its text, runs
// there on a session of its own and moves back to the main window; the quick query window
// opens from the palette on the last connection; dragging the editor's height is kept in
// settings.toml for the next start.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { connect, CTRL } from "../cdp.mjs";
import { palette } from "../plans.mjs";

const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const SQL = "SELECT 42 AS answer";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "w.db"));
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY);");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000951"
name = "Win db"
driver = "sqlite"
database = '${join(dir, "w.db")}'
save_password = false
`,
  );
}

async function typeSql(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent.includes(${JSON.stringify(sql)})`);
}

/** The other window of the app (not `main`), once it has loaded. */
async function otherWindow(port, mainId) {
  const other = await connect(port, 30_000, (t) => t.id !== mainId);
  await other.waitFor(`document.querySelector('.window-app') !== null`, 30_000);
  return other;
}

async function dragEditor(page, by) {
  const handle = await page.rect(`.query-tab:not([style*='none']) .panel-resizer.row`);
  const [x, y] = [handle.x + handle.w / 2, handle.y + handle.h / 2];
  await page.mouse("mouseMoved", x, y);
  await page.mouse("mousePressed", x, y);
  for (let i = 1; i <= 5; i++) await page.mouse("mouseMoved", x, y + (by * i) / 5);
  await page.mouse("mouseReleased", x, y + by);
}

export default async function (page, { dir, port }) {
  await page.waitFor(`${row("Win db")} !== undefined`);
  await page.eval(`${row("Win db")}.querySelector('button[aria-label="Open SQL editor"]').click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);

  // The editor's height: dragged, and kept in the settings file.
  const before = await page.eval(`${visibleTab}.querySelector('.cm-editor').getBoundingClientRect().height`);
  await dragEditor(page, 80);
  await page.waitFor(`${visibleTab}.querySelector('.cm-editor').getBoundingClientRect().height > ${before + 40}`);
  assert.match(readFileSync(join(dir, "settings.toml"), "utf8"), /editor_height = \d+/);

  // Open in a new window: the tab leaves the main window and its text goes along.
  await typeSql(page, SQL);
  await page.eval(`document.querySelector('.tab.active').id = 'e2e-tab'`);
  await page.clickSel("#e2e-tab", { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('Open in a new window')).click()`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length === 0`, 15_000);
  const win = await otherWindow(port, page.targetId);
  await win.waitFor(`document.querySelector('.cm-content')?.textContent.includes(${JSON.stringify(SQL)})`, 30_000);
  await win.eval(`document.querySelector('.cm-content').focus()`);
  await win.key("Enter", "Enter", 13, CTRL);
  await win.waitFor(`document.querySelector('.statusbar span')?.textContent === '1 row'`, 30_000);

  // Back to the main window, as an open tab.
  await win.eval(`[...document.querySelectorAll('.window-bar button')].find(b => b.textContent === 'Move to the main window').click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content')?.textContent.includes(${JSON.stringify(SQL)})`, 30_000);
  win.close?.();

  // The quick query, from the palette, on the connection it can use.
  await palette(page, "Quick query");
  const quick = await otherWindow(port, page.targetId);
  await quick.waitFor(`document.querySelector('.window-connection select')?.selectedOptions[0]?.textContent === 'Win db'`, 30_000);
  await quick.waitFor(`document.querySelector('.cm-content') !== null`, 30_000);
  await quick.eval(`window.__TAURI_INTERNALS__.invoke('plugin:window|close', { label: 'quick' })`).catch(() => {});
}
