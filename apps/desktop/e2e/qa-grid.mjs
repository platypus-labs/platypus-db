// What the QA scenarios of the data grid share: saved connections to the dev databases of
// docker/compose.yml, SQL run on them through `docker exec` (to seed and check tables
// outside the app), and the grid's usual gestures.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { join } from "node:path";
import { ALT, CTRL, SHIFT } from "./cdp.mjs";

export { ALT, CTRL, SHIFT };

export const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
export const statusSpan = `${visibleTab}?.querySelector('.statusbar span')`;
export const status = `(${statusSpan}?.textContent ?? '')`;
export const statusBar = `(${visibleTab}?.querySelector('.statusbar')?.textContent ?? '')`;
export const editBar = `(${visibleTab}?.querySelector('.edit-bar:not([hidden])')?.textContent ?? '')`;
export const grid = `${visibleTab}.querySelector('.grid-scroll')`;
/** The SQL editor's text; the field above a result grid is another editor. */
export const editorText = `[...${visibleTab}.querySelector('.cm-content').querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;
export const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
export const menuButton = (label) =>
  `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === ${JSON.stringify(label)})`;
export const button = (scope, label) =>
  `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(label)} || b.getAttribute('aria-label') === ${JSON.stringify(label)})`;

/** The dev databases: port, container, how to reach them from the app and from a shell. */
export const DBS = {
  pg: {
    port: 55432,
    password: "platypus",
    toml: `driver = "postgres"\nhost = "localhost"\nport = 55432\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"`,
    shell: ["exec", "-i", "platypus-db-postgres-1", "psql", "-U", "platypus", "-d", "platypus", "-v", "ON_ERROR_STOP=1", "-q", "-A", "-t", "-F", "\t"],
  },
  mysql: {
    port: 53306,
    password: "platypus",
    toml: `driver = "mysql"\nhost = "localhost"\nport = 53306\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"`,
    shell: ["exec", "-i", "platypus-db-mysql-1", "mysql", "-uplatypus", "-pplatypus", "-N", "-B", "--default-character-set=utf8mb4", "platypus"],
  },
  mariadb: {
    port: 53307,
    password: "platypus",
    toml: `driver = "mysql"\nhost = "localhost"\nport = 53307\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"`,
    shell: ["exec", "-i", "platypus-db-mariadb-1", "mariadb", "-uplatypus", "-pplatypus", "-N", "-B", "--default-character-set=utf8mb4", "platypus"],
  },
  mssql: {
    port: 51433,
    password: "Platypus-2026",
    toml: `driver = "sqlserver"\nhost = "localhost"\nport = 51433\ndatabase = "platypus"\nuser = "sa"\nssl_mode = "disable"`,
    shell: [
      "exec", "-i", "platypus-db-sqlserver-1", "/opt/mssql-tools18/bin/sqlcmd", "-C", "-b", "-S", "localhost", "-U", "sa",
      "-P", "Platypus-2026", "-d", "platypus", "-h", "-1", "-W", "-s", "\t", "-f", "65001",
    ],
  },
  mongo: {
    port: 57017,
    password: "platypus",
    toml: `driver = "mongo"\nhost = "localhost"\nport = 57017\ndatabase = "platypus?authSource=admin"\nuser = "platypus"\nssl_mode = "disable"`,
    shell: ["exec", "-i", "platypus-db-mongo-1", "mongosh", "--quiet", "-u", "platypus", "-p", "platypus", "--authenticationDatabase", "admin", "platypus"],
  },
  clickhouse: {
    port: 58123,
    password: "platypus",
    toml: `driver = "clickhouse"\nhost = "localhost"\nport = 58123\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"`,
    shell: ["exec", "-i", "platypus-db-clickhouse-1", "clickhouse-client", "--user", "platypus", "--password", "platypus", "-d", "platypus", "-n"],
  },
};

/** A `skip()` for scenarios that need the databases `names`. */
export function needs(...names) {
  return async () => {
    for (const name of names) {
      const port = DBS[name].port;
      const up = await new Promise((resolve) => {
        const socket = tcp({ host: "127.0.0.1", port, timeout: 1000 });
        const done = (ok) => {
          socket.destroy();
          resolve(ok);
        };
        socket.on("connect", () => done(true));
        socket.on("error", () => done(false));
        socket.on("timeout", () => done(false));
      });
      if (!up) return `no ${name} on port ${port} (docker compose -f docker/compose.yml up -d)`;
    }
    return null;
  };
}

/** Runs `sql` on the dev database `db` from a shell in its container; returns the output. */
export function sh(db, sql) {
  const r = spawnSync("docker", DBS[db].shell, { input: sql, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`${db}: ${r.stderr || r.stdout}`);
  return r.stdout.replace(/\r/g, "").trim();
}

/** Writes connections.toml with one connection per `[name, db]` (db: a key of DBS, or
 *  `{ sqlite: path }` / `{ duckdb: path }`). */
export function connections(dir, list) {
  let text = "version = 1\n";
  list.forEach(([name, db], i) => {
    const id = `00000000-0000-4000-8000-0000000009${String(i).padStart(2, "0")}`;
    const body =
      typeof db === "string"
        ? DBS[db].toml
        : db.sqlite
          ? `driver = "sqlite"\ndatabase = '${db.sqlite}'`
          : `driver = "duckdb"\ndatabase = '${db.duckdb}'`;
    text += `\n[[connections]]\nid = "${id}"\nname = "${name}"\n${body}\nsave_password = false\n`;
  });
  writeFileSync(join(dir, "connections.toml"), text);
}

/** Expands the connection `name`, answering the password prompt for `db` if there is one,
 *  and waits for `child` in the tree. */
export async function expand(page, name, db, child) {
  await page.waitFor(`${treeRow(name)} !== undefined`);
  await page.eval(`${treeRow(name)}.querySelector(".twisty").click()`);
  if (db && DBS[db]) {
    await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`, 15_000);
    await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
    await page.type(DBS[db].password);
    await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
  }
  if (child) await page.waitFor(`${treeRow(child)} !== undefined`, 30_000);
}

/** Connects `name` (answering the password prompt for `db`) and opens a SQL editor on it. */
export async function openEditor(page, name, db, child) {
  await expand(page, name, db, child);
  await page.eval(`document.querySelectorAll('#qa-conn').forEach(e => e.removeAttribute('id')); ${treeRow(name)}.id = 'qa-conn'`);
  await page.clickSel("#qa-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 30_000);
}

/** Opens a tree row's context menu and runs `item`. */
export async function treeMenu(page, label, item) {
  await page.waitFor(`${treeRow(label)} !== undefined`, 15_000);
  await page.eval(`document.querySelectorAll('#qa-row').forEach(e => e.removeAttribute('id')); ${treeRow(label)}.id = 'qa-row'; document.getElementById('qa-row').scrollIntoView({ block: 'center' })`);
  await page.sleep(100);
  const r = await page.rect("#qa-row");
  await page.click(r.x + 20, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.waitFor(`[...document.querySelectorAll('.context-menu button')].some(b => b.textContent.startsWith(${JSON.stringify(item)}))`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(item)})).click()`);
}

/** Refreshes the tree row `label` (a schema or database), so new tables show. */
export async function refresh(page, label) {
  await treeMenu(page, label, "Refresh");
}

/** Waits until no statement runs and the status passes `test` (a JS expression on `s`). */
export async function idle(page, test = "true", timeout = 30_000) {
  await page.waitFor(
    `(() => { const s = ${status}; return ${cancel}?.disabled === true && s !== '' && !s.includes('Executing') && ${visibleTab}.querySelector('.result-loading') === null && (${test}); })()`,
    timeout,
  );
  return page.eval(status);
}

export const clearStatus = (page) => page.eval(`(() => { const s = ${statusSpan}; if (s) s.textContent = ''; })()`);

/** Opens the data of the table `label` (already shown in the tree) and waits for its rows. */
export async function openData(page, label) {
  await treeMenu(page, label, "Open data");
  await page.waitFor(`${visibleTab}?.querySelector('.viewer-bar') != null`, 15_000);
  return idle(page, "s.includes('row')");
}

/** Types `sql` as the editor's text and runs it (`script`: the whole script, Alt+X). */
export async function run(page, sql, script = false) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await page.key("Escape", "Escape", 27);
  await clearStatus(page);
  if (script) await page.key("x", "KeyX", 88, ALT);
  else await page.key("Enter", "Enter", 13, CTRL);
  return idle(page);
}

export async function focusCell(page, row, col) {
  await page.eval(`${grid}.focus()`);
  await page.key("Home", "Home", 36, CTRL);
  for (let i = 0; i < row; i++) await page.key("ArrowDown", "ArrowDown", 40);
  for (let i = 0; i < col; i++) await page.key("ArrowRight", "ArrowRight", 39);
}

export async function selectDown(page, from, to, col) {
  await focusCell(page, from, col);
  for (let i = from; i < to; i++) await page.key("ArrowDown", "ArrowDown", 40, SHIFT);
}

/** Runs a grid menu item through its submenus; the menu opens on the row numbers, which keep
 *  the selection. */
export async function gridMenu(page, ...path) {
  await page.eval(`document.querySelectorAll('#qa-grid').forEach(e => e.removeAttribute('id')); ${grid}.id = 'qa-grid'`);
  const r = await page.rect("#qa-grid");
  await page.click(r.x + 4, r.y + 52, { button: "right" });
  for (const label of path) {
    await page.waitFor(`${menuButton(label)} !== undefined`);
    await page.eval(`${menuButton(label)}.click()`);
  }
}

/** Replaces the clipboard's writeText so copies can be read. */
export const stubClipboard = (page) => page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);

export async function copied(page, action) {
  await page.eval(`window.__copied = undefined`);
  await action();
  await page.waitFor(`window.__copied !== undefined`);
  return page.eval(`window.__copied`);
}

/** Selects everything and copies it (`headers`: with the column names). */
export async function copyAll(page, headers = false) {
  return copied(page, async () => {
    await page.eval(`${grid}.focus()`);
    await page.key("a", "KeyA", 65, CTRL);
    if (headers) await page.key("C", "KeyC", 67, CTRL | SHIFT);
    else await page.key("c", "KeyC", 67, CTRL);
  });
}

/** The full values of the result rows, as the editor reads them. */
export async function rows(page) {
  const text = await copyAll(page);
  return text === "" ? [] : text.split("\n").map((l) => l.split("\t"));
}

/** Types `text` in the focused cell's editor (F2) and commits it with Enter. */
export async function editCell(page, text) {
  await page.key("F2", "F2", 113);
  await page.waitFor(`document.querySelector('.cell-editor, .cell-popup-text') !== null`);
  await page.eval(`(() => {
    const el = document.querySelector('.cell-editor, .cell-popup-text');
    el.focus();
    if (el.select) el.select();
  })()`);
  await page.type(text);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.cell-editor') === null`);
}

/** Sets the focused cell to `text` through its editor (F2), whatever the editor: the inline
 *  input, a select (bool, enum) or the popup (JSON, long text); returns the editor's kind. */
export async function setCell(page, text) {
  await page.key("F2", "F2", 113);
  await page.waitFor(`document.querySelector('.cell-editor, .cell-popup-text, .list-editor') !== null`);
  const kind = await page.eval(`(() => {
    const el = document.querySelector('.cell-editor, .cell-popup-text');
    if (!el) return 'list';
    const proto = el.tagName === 'SELECT' ? HTMLSelectElement : el.tagName === 'TEXTAREA' ? HTMLTextAreaElement : HTMLInputElement;
    if (el.tagName === 'SELECT' && ![...el.options].some(o => o.value === ${JSON.stringify(text)})) {
      const o = document.createElement('option'); o.value = ${JSON.stringify(text)}; el.appendChild(o);
    }
    Object.getOwnPropertyDescriptor(proto.prototype, 'value').set.call(el, ${JSON.stringify(text)});
    el.dispatchEvent(new Event(el.tagName === 'SELECT' ? 'change' : 'input', { bubbles: true }));
    return el.tagName === 'SELECT' ? 'select' : el.tagName === 'TEXTAREA' ? 'popup' : el.type;
  })()`);
  if (kind === "popup") {
    await page.eval(`[...document.querySelectorAll('.cell-popup button')].find(b => b.textContent === 'Apply').click()`);
  } else if (kind !== "select" && kind !== "list") {
    await page.eval(`document.querySelector('.cell-editor').focus()`);
    await page.key("Enter", "Enter", 13);
  }
  if (kind !== "list") await page.waitFor(`document.querySelector('.cell-editor, .cell-popup') === null`);
  return kind;
}

export async function previewSql(page) {
  await page.eval(`${button(visibleTab, "Preview SQL")}.click()`);
  await page.waitFor(`document.querySelector('.sql-preview') !== null`, 15_000);
  const sql = await page.eval(`document.querySelector('.sql-preview').textContent`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.sql-preview') === null`);
  return sql;
}

/** Saves with the edit bar's Save button; returns the edit bar's or the error's text. */
export async function save(page) {
  await clearStatus(page);
  // A notice left by an edit (cells a mass edit skipped) is not the save's.
  await page.eval(`${visibleTab}.querySelector('.edit-error button')?.click()`);
  await page.eval(`${visibleTab}.querySelector('.edit-bar button.primary').click()`);
  await page.waitFor(
    `${editBar}.includes('to edit a cell') || ${visibleTab}.querySelector('.edit-bar .error, .edit-error, .notice.error') != null || document.querySelector('.modal .error') != null`,
    60_000,
  );
  return page.eval(`(() => {
    const tab = ${visibleTab};
    const err = tab.querySelector('.edit-bar .error, .edit-error, .notice.error') ?? document.querySelector('.modal .error');
    return err ? 'ERROR: ' + err.textContent : tab.querySelector('.edit-bar:not([hidden])')?.textContent;
  })()`);
}

export const shotter = (page, shots, prefix) => async (name) => {
  if (shots) await page.screenshot(join(shots, `${prefix}-${name}.png`));
};

export { assert };
