// What the performance scenarios (qa-perf-*) share: saved connections, the SQL editor of a
// connection, running statements, fetching a whole result, the clipboard and the memory of the
// app's processes.
import { spawnSync } from "./proc.mjs";
import { writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { join } from "node:path";
import { CTRL } from "./cdp.mjs";

export const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
export const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
export const statusbar = `(${visibleTab}?.querySelector('.statusbar')?.textContent ?? '')`;
export const grid = `${visibleTab}.querySelector('.grid-scroll')`;
const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;
export const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

export const PG = { name: "QA perf PG", port: 55432, password: "platypus" };
export const MSSQL = { name: "QA perf SQL Server", port: 51433, password: "Platypus-2026" };

/** `skip()` of a scenario that needs the database container listening on `port`. */
export function needsPort(port, what) {
  return () =>
    new Promise((resolve) => {
      const socket = tcp({ host: "127.0.0.1", port, timeout: 1000 });
      const done = (reason) => {
        socket.destroy();
        resolve(reason);
      };
      socket.on("connect", () => done(null));
      socket.on("error", () => done(`no ${what} on port ${port} (docker compose -f docker/compose.yml up -d)`));
      socket.on("timeout", () => done(`no ${what} on port ${port}`));
    });
}

export const sqliteEntry = (name, file) =>
  `[[connections]]\nid = "00000000-0000-4000-8000-0000000e0001"\nname = "${name}"\ndriver = "sqlite"\ndatabase = '${file}'\nsave_password = false\n`;

export const pgEntry = () =>
  `[[connections]]\nid = "00000000-0000-4000-8000-0000000e0002"\nname = "${PG.name}"\ndriver = "postgres"\nhost = "localhost"\nport = ${PG.port}\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"\nsave_password = false\n`;

export const mssqlEntry = () =>
  `[[connections]]\nid = "00000000-0000-4000-8000-0000000e0003"\nname = "${MSSQL.name}"\ndriver = "sqlserver"\nhost = "localhost"\nport = ${MSSQL.port}\ndatabase = "platypus"\nuser = "sa"\nsave_password = false\n`;

export function saveConnections(dir, ...entries) {
  writeFileSync(join(dir, "connections.toml"), `version = 6\n\n${entries.join("\n")}`);
}

/** Expands the connection `name`, answering its password prompt when `password` is given. */
export async function expand(page, name, password) {
  await page.waitFor(`${treeRow(name)} !== undefined`, 30_000);
  await page.eval(`${treeRow(name)}.querySelector(".twisty").click()`);
  if (password) {
    await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`, 15_000);
    await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
    await page.type(password);
    await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
    await page.waitFor(`document.querySelector('.modal input[type=password]') === null`, 30_000);
  }
}

/** Opens a SQL editor on the connection `name` (a double click on its row). */
export async function openEditor(page, name) {
  await page.eval(`document.querySelectorAll('#e2e-editor-of').forEach(e => e.removeAttribute('id'));
    ${treeRow(name)}.id = 'e2e-editor-of'; document.getElementById('e2e-editor-of').scrollIntoView({ block: 'center' })`);
  const tabs = await page.eval(`document.querySelectorAll('.tab').length`);
  await page.clickSel("#e2e-editor-of", { clickCount: 2 });
  await page.waitFor(`document.querySelectorAll('.tab').length > ${tabs} && ${visibleTab}?.querySelector('.cm-content') != null`);
}

/** Opens the context menu of the navigator row `label` and picks `item`. */
export async function rowMenu(page, label, item) {
  await page.eval(`document.querySelectorAll('#e2e-menu-of').forEach(e => e.removeAttribute('id'));
    ${treeRow(label)}.id = 'e2e-menu-of'; document.getElementById('e2e-menu-of').scrollIntoView({ block: 'center' })`);
  const r = await page.rect("#e2e-menu-of");
  await page.click(r.x + 20, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(item)})).click()`);
}

/** Replaces the editor's text with `sql` and runs it (`script`: all of it); returns once the
 *  run is over. The status is emptied first, so the old text cannot end the wait. */
export async function run(page, sql, { script = false, timeout = 60_000 } = {}) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent.includes(${JSON.stringify(sql.slice(-20))})`);
  // A completion list left open would take the keys.
  await page.key("Escape", "Escape", 27);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  if (script) await page.key("x", "KeyX", 88, 1);
  else await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${cancel}?.disabled === true && ${status} !== '' && !${status}.includes('Executing')`, timeout);
}

/** Clicks "Fetch all" and waits until the status says every row is loaded. */
export async function fetchAll(page, timeout = 120_000) {
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.eval(`[...${visibleTab}.querySelectorAll('.statusbar button')].find(b => b.textContent === 'Fetch all').click()`);
  await page.waitFor(`/^[\\d.,]+ rows?$/.test(${status}.trim())`, timeout);
}

/** The grid's top-left corner on screen. */
export const gridAt = (page) => page.eval(`(() => { const r = ${grid}.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height }; })()`);

/** Presses `key` with `modifiers` on the grid and returns what it wrote to the clipboard. */
export async function clipboardAfter(page, key, code, keyCode, modifiers, timeout = 15_000) {
  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }; window.__copied = undefined`);
  // A key right after the grid took the focus is sometimes lost: press again until it copies.
  const deadline = Date.now() + timeout;
  for (;;) {
    await page.key(key, code, keyCode, modifiers);
    try {
      await page.waitFor(`window.__copied !== undefined`, Math.min(3000, Math.max(0, deadline - Date.now())));
      return page.eval(`window.__copied`);
    } catch (e) {
      if (Date.now() >= deadline) throw e;
    }
  }
}

/** How long the page takes to answer: a busy main thread answers late. */
export async function lag(page) {
  const started = performance.now();
  await page.eval("1");
  return performance.now() - started;
}

/** Private memory in MB of the app (`app`) and of its WebView2 processes (`webview`), found
 *  by the profile folder `dir` of the scenario. */
export function memory(dir) {
  const script =
    `$w = @(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like '*${dir.replaceAll("'", "''")}*' });` +
    `$ids = $w | ForEach-Object { $_.ProcessId };` +
    `$app = $w | Where-Object { $ids -notcontains $_.ParentProcessId } | Select-Object -First 1;` +
    `$a = if ($app) { (Get-Process -Id $app.ParentProcessId).PrivateMemorySize64 } else { 0 };` +
    `$v = ($w | ForEach-Object { (Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue).PrivateMemorySize64 } | Measure-Object -Sum).Sum;` +
    `"$a $v"`;
  const out = spawnSync("powershell", ["-NoProfile", "-Command", script], { encoding: "utf8" }).stdout.trim();
  const [app, webview] = out.split(/\s+/).map((n) => Math.round(Number(n) / 1e6));
  return { app, webview };
}
