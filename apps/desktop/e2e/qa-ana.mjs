// What the analysis QA scenarios (`scenarios/qa-ana-*.mjs`) share: saved connections to SQLite,
// DuckDB and the Docker databases, running statements in the main editor (only its lines: the
// SQL field above a result is a CodeMirror too), the analysis panels' selectors.
import { writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "./cdp.mjs";

export const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const statusSpan = `${visibleTab}?.querySelector('.statusbar span')`;
export const status = `(${statusSpan}?.textContent ?? '')`;
/** The main editor's text, without the SQL field of the result. */
export const editorText = `[...(${visibleTab}?.querySelector('.editor')?.querySelectorAll('.cm-line') ?? [])].map(l => l.textContent).join('\\n')`;
const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;
export const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

// Analysis selectors.
export const profile = `${visibleTab}?.querySelector('.column-profile')`;
export const chart = `${visibleTab}?.querySelector('.chart-svg')`;
export const toggle = (label) => `[...${visibleTab}.querySelectorAll('.view-toggle button')].find(b => b.textContent === ${JSON.stringify(label)})`;
export const yBox = (column) =>
  `[...${visibleTab}.querySelectorAll('.chart-side fieldset label')].find(l => l.textContent === ${JSON.stringify(column)})?.querySelector('input')`;
export const compareButton = `[...(${visibleTab}?.querySelectorAll('.statusbar button') ?? [])].find(b => b.textContent === 'Compare with previous run')`;
export const diffTab = `[...document.querySelectorAll('.diff-tab')].find(t => t.style.display !== 'none')`;
export const field = `${visibleTab}?.querySelector('.result-sql .cm-content')`;
export const fieldText = `(() => { const l = ${visibleTab}?.querySelector('.result-sql .cm-line'); return !l || l.querySelector('.cm-placeholder') ? '' : l.textContent; })()`;
export const mark = `(${visibleTab}?.querySelector('.result-sql-mark')?.textContent ?? '')`;
export const fieldError = `(${visibleTab}?.querySelector('.result-sql .edit-error')?.textContent ?? '')`;
const completing = `${visibleTab}?.querySelector('.result-sql .cm-tooltip-autocomplete') != null`;

/** Sets a select of the chart's side panel as a user would. */
export const choose = (label, value) => `(() => {
  const el = ${visibleTab}.querySelector('select[aria-label=${JSON.stringify(label)}]');
  Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('change', { bubbles: true }));
})()`;

/** `null` when something listens on `port`, else why the scenario skips. */
export function listening(port, what) {
  return new Promise((resolve) => {
    const socket = tcp({ host: "127.0.0.1", port, timeout: 1000 });
    const done = (reason) => {
      socket.destroy();
      resolve(reason);
    };
    socket.on("connect", () => done(null));
    socket.on("error", () => done(`no ${what} on port ${port}`));
    socket.on("timeout", () => done(`no ${what} on port ${port}`));
  });
}

/** A saved connection entry of connections.toml. */
export function entry({ id, name, driver, host, port, database, user, password }) {
  return `
[[connections]]
id = "${id}"
name = "${name}"
driver = "${driver}"
${host ? `host = "${host}"\nport = ${port}\n` : ""}database = '${database}'
${user ? `user = "${user}"\n` : ""}${password ? `password_command = "echo ${password}"\nssl_mode = "disable"\n` : ""}save_password = false
`;
}

/** Writes connections.toml with `entries`. */
export function saveConnections(dir, entries) {
  writeFileSync(join(dir, "connections.toml"), `version = 1\n${entries.map(entry).join("")}`);
}

/** Creates the SQLite file `file` in `dir` with `sql`; returns its path. */
export function sqliteFile(dir, file, sql) {
  const path = join(dir, file);
  const db = new DatabaseSync(path);
  db.exec(sql);
  db.close();
  return path;
}

/** Runs `sql` on the SQLite file at `path` from outside the app. */
export function sqliteExec(path, sql) {
  const db = new DatabaseSync(path, { timeout: 5_000 });
  db.exec(sql);
  db.close();
}

/** Opens a SQL editor on connection `name` from its context menu ("New SQL script"). */
export async function newEditor(page, name) {
  await page.waitFor(`${treeRow(name)} !== undefined`);
  const before = await page.eval(`document.querySelectorAll('.query-tab').length`);
  const r = await page.eval(`(() => { const b = ${treeRow(name)}.getBoundingClientRect(); return { x: b.x, y: b.y, h: b.height }; })()`);
  await page.click(r.x + 30, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith('New SQL script')).click()`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length > ${before} && ${visibleTab}?.querySelector('.editor .cm-content') != null`, 15_000);
}

/** Empties the status first: the old text may be the same. The run's end is a new text with
 *  Cancel disabled again. */
async function afterRun(page, action, timeout = 30_000) {
  await page.eval(`(() => { const s = ${statusSpan}; if (s) s.textContent = ''; })()`);
  await action();
  await page.waitFor(
    `(() => { const s = ${status}; return ${cancel}?.disabled === true && s !== '' && !s.includes('Executing'); })()`,
    timeout,
  );
}

/** Types `sql` as the main editor's whole text and runs it (Ctrl+Enter: the statement at the
 *  cursor). */
export async function runQuery(page, sql, timeout) {
  await page.eval(`${visibleTab}.querySelector('.editor .cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await page.key("Escape", "Escape", 27);
  await afterRun(page, () => page.key("Enter", "Enter", 13, CTRL), timeout);
}

/** Runs the main editor's text again. */
export async function rerun(page, timeout) {
  await page.eval(`${visibleTab}.querySelector('.editor .cm-content').focus()`);
  await afterRun(page, () => page.key("Enter", "Enter", 13, CTRL), timeout);
}

/** Runs the editor's whole text as a script (Alt+X). */
export async function runScript(page, sql, timeout) {
  await page.eval(`${visibleTab}.querySelector('.editor .cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await page.key("Escape", "Escape", 27);
  await afterRun(page, () => page.key("x", "KeyX", 88, 1), timeout);
}

/** Opens `table` of connection `name` from the navigator and waits for its `rows`. */
export async function openTable(page, name, table, rows, schema) {
  await page.waitFor(`${treeRow(name)} !== undefined`);
  await page.eval(`${treeRow(name)}.querySelector(".twisty").click()`);
  if (schema) {
    await page.waitFor(`${treeRow(schema)} !== undefined`, 15_000);
    const open = await page.eval(`${treeRow(table)} !== undefined`);
    if (!open) await page.eval(`${treeRow(schema)}.querySelector(".twisty").click()`);
  }
  await page.waitFor(`${treeRow(table)} !== undefined`, 15_000);
  await page.eval(`${treeRow(table)}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table')?.removeAttribute('id')`);
  await page.waitFor(`${status} === ${JSON.stringify(rows)}`, 20_000);
}

/** The grid is drawn on a canvas: the mouse finds a column's header by moving along the header
 *  row until the statistics box names it. Returns the box's text and its parts. */
export async function profileOf(page, name) {
  const at = await page.eval(`(() => {
    const s = ${visibleTab}.querySelector('.grid-scroll');
    s.scrollLeft = 0;
    const g = s.getBoundingClientRect();
    return { x: g.x, y: g.y, w: g.width };
  })()`);
  const head = `(${profile}?.querySelector('.column-profile-head strong')?.textContent ?? null)`;
  await page.mouse("mouseMoved", at.x + 40, at.y + 300);
  await page.waitFor(`${profile} == null`, 3_000);
  let previous = null;
  for (let x = at.x + 40; x < at.x + at.w - 10; x += 28) {
    await page.mouse("mouseMoved", x, at.y + 14);
    try {
      await page.waitFor(`${head} !== null && ${head} !== ${JSON.stringify(previous)}`, 1_500);
    } catch {
      continue;
    }
    previous = await page.eval(head);
    if (previous === name) {
      return page.eval(`(() => {
        const p = ${profile};
        const dl = [...p.querySelectorAll('dt')].map(dt => [dt.textContent, dt.nextElementSibling?.textContent]);
        return {
          text: p.textContent,
          fields: Object.fromEntries(dl),
          top: [...p.querySelectorAll('.column-profile-top span')].map(s => s.textContent),
          range: [...p.querySelectorAll('.column-profile-range span')].map(s => s.textContent),
          bars: [...p.querySelectorAll('.column-profile-bars rect')].length,
          scope: p.querySelector('.column-profile-scope')?.textContent,
        };
      })()`);
    }
  }
  throw new Error(`no header named ${name}`);
}

/** The visible grid's rows as tab-separated text with headers (Ctrl+A, then Copy with headers). */
export async function gridTsv(page) {
  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
  await page.eval(`window.__copied = undefined; ${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.key("C", "KeyC", 67, CTRL | 8);
  await page.waitFor(`window.__copied !== undefined`);
  return page.eval(`window.__copied`);
}

/** Types `sql` as the result field's whole text. It ends with a character that closes the
 *  completion list, which would otherwise take Enter. */
export async function typeInField(page, sql) {
  await page.waitFor(`${field} != null`);
  await page.eval(`${field}.focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${fieldText} === ${JSON.stringify(sql)}`);
  await page.sleep(100);
  if (await page.eval(completing)) await page.key("Escape", "Escape", 27);
  await page.waitFor(`!(${completing})`);
}

/** Runs `sql` in the result's field: its mark ("SQL view · …") and error. */
export async function inField(page, sql) {
  await typeInField(page, sql);
  return runField(page);
}

/** The tooltips of the chart's marks matching `selector` (e.g. `rect.chart-bar`). */
export const markTitles = (page, selector) =>
  page.eval(`[...(${chart}?.querySelectorAll(${JSON.stringify(selector)}) ?? [])].map(e => e.querySelector('title')?.textContent)`);

/** Shows the chart of the result and waits until it is drawn with marks matching `selector`
 *  (any SVG when `null`). */
export async function showChart(page, selector = null) {
  await page.eval(`${toggle("Chart")}.click()`);
  await page.waitFor(selector ? `${chart}?.querySelectorAll(${JSON.stringify(selector)}).length > 0` : `${chart} != null`, 10_000);
  await page.sleep(500);
}

/** Changes a select of the chart and leaves time for the backend to compute it again (the
 *  chart keeps its element, nothing tells a new drawing apart). */
export async function chooseAndRedraw(page, label, value) {
  await page.eval(choose(label, value));
  await page.sleep(1200);
}

/** A DuckDB file connection in `dir`, created by the app on first use. */
export function duckConnection(dir, name, id) {
  saveConnections(dir, [{ id, name, driver: "duckdb", database: join(dir, "qa_ana.duckdb") }]);
}

/** A SQLite file connection in `dir` made with `sql`; returns the file's path. */
export function liteConnection(dir, name, id, sql) {
  const path = sqliteFile(dir, "qa_ana.db", sql);
  saveConnections(dir, [{ id, name, driver: "sqlite", database: path }]);
  return path;
}

/** Presses Enter in the field and waits for the field to finish. */
export async function runField(page) {
  // The field shows "Executing…" while it runs: a MutationObserver sees it however briefly.
  await page.eval(`(() => {
    window.__fieldBusy = false;
    window.__fieldObserver?.disconnect();
    const bar = ${visibleTab}.querySelector('.result-sql');
    window.__fieldObserver = new MutationObserver(() => {
      if (bar.textContent.includes('Executing…')) window.__fieldBusy = true;
    });
    window.__fieldObserver.observe(bar, { subtree: true, childList: true, characterData: true });
  })()`);
  await page.eval(`${field}.focus()`);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`window.__fieldBusy === true`, 5_000);
  await page.waitFor(`!(${visibleTab}.querySelector('.result-sql')?.textContent.includes('Executing…'))`, 30_000);
  await page.sleep(200);
  return { mark: await page.eval(mark), error: await page.eval(fieldError) };
}
