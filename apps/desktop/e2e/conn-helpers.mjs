// What the connection scenarios (qa-conn-*) share: the connection dialog, the driver picker,
// the navigator's connection rows and a check that a database container is up.
import { writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { join } from "node:path";

export const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
export const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
export const dialog = `document.querySelector('.connection-dialog')`;
export const picker = `document.querySelector('.modal.driver-picker')`;
/** The URL field of the form's "Connect by" URL mode. */
export const urlField = '.connection-dialog [data-field="connection_url"]';
/** The notice under the form: "Connected: …" or the error. */
export const notice = `(document.querySelector('.connection-dialog .notice.ok, .connection-dialog .notice.error')?.textContent ?? '')`;

/** A button of `scope` (an expression) by its words or accessible name. */
export const button = (scope, text) =>
  `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)} || b.getAttribute('aria-label') === ${JSON.stringify(text)})`;

/** The input, select or textarea of the dialog's field labelled `label`. */
export const formField = (label, scope = ".connection-dialog") =>
  `[...document.querySelectorAll(${JSON.stringify(`${scope} label.field`)})].find(l => l.querySelector('span')?.textContent === ${JSON.stringify(label)})?.querySelector('input, select, textarea')`;

/** Sets a React-controlled field (an element expression) and tells React, as typing would. */
export const fill = (element, value) => `(() => {
  const el = ${element};
  const proto = Object.getPrototypeOf(el);
  Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event(el.tagName === 'SELECT' ? 'change' : 'input', { bubbles: true }));
})()`;

/** Pastes `text` into the element `element`, as Ctrl+V would. */
export const paste = (element, text) => `(() => {
  const data = new DataTransfer();
  data.setData('text/plain', ${JSON.stringify(text)});
  ${element}.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
})()`;

/** Opens the picker of a new connection. */
export async function openPicker(page) {
  await page.eval(`document.querySelector('button[title="New connection"]').click()`);
  await page.waitFor(`${picker} !== null`);
}

/** Opens the form of a new connection for the picker's tile `label`. */
export async function newConnection(page, label) {
  await openPicker(page);
  await page.eval(`[...document.querySelectorAll('.driver-tile')].find(b => b.textContent === ${JSON.stringify(label)}).click()`);
  await page.waitFor(`${dialog} !== null`);
}

/** The picker's tile for the database a connection URL is for. */
function tileOf(url) {
  const scheme = /^(?:jdbc:)?([a-z0-9+.-]+)/i.exec(url)?.[1].toLowerCase() ?? "";
  const tiles = [
    [/^(postgres|postgresql)$/, "PostgreSQL"],
    [/^mysql|^mariadb$/, "MySQL"],
    [/^(sqlserver|mssql)$/, "SQL Server"],
    [/^clickhouse/, "ClickHouse"],
    [/^(libsql|turso)/, "Turso / libSQL"],
    [/^mongodb/, "MongoDB"],
    [/^sqlite$/, "SQLite"],
    [/^duckdb$/, "DuckDB"],
    [/^oracle$/, "Oracle"],
  ];
  return tiles.find(([re]) => re.test(scheme))?.[1] ?? "PostgreSQL";
}

/** Opens the form of a new connection for the database of `url` and enters the URL as the
 *  user would: in the form's URL mode where the driver has one (then back to the host
 *  fields, which the scenarios read), else pasted in the name field. The name is the
 *  database's or the host's, as the paste gives it. */
export async function newFromUrl(page, url) {
  if (await page.eval(`${picker} === null`)) await openPicker(page);
  await page.eval(`[...document.querySelectorAll('.driver-tile')].find(b => b.textContent === ${JSON.stringify(tileOf(url))}).click()`);
  await page.waitFor(`${dialog} !== null`);
  const byUrl = `document.querySelector('.connection-dialog [data-field="connect_by_url"]')`;
  if (await page.eval(`${byUrl} === null`)) {
    await page.eval(paste(formField("Name"), url));
    await page.sleep(300);
    return;
  }
  await page.eval(`${byUrl}.click()`);
  await page.waitFor(`document.querySelector(${JSON.stringify(urlField)}) !== null`);
  await page.eval(fill(`document.querySelector(${JSON.stringify(urlField)})`, url));
  // The settings follow the URL once the backend has read it: the printed URL differs from
  // what was typed, or the form shows an error.
  await page.sleep(400);
  await page.eval(`document.querySelector('.connection-dialog [data-field="connect_by_host"]').click()`);
  await page.waitFor(`${formField("Host")} !== undefined`);
  if (await page.eval(`${formField("Name")}.value`) === "") {
    const database = await page.eval(`(${formField("Database")} ?? ${formField("Database (optional)")})?.value ?? ''`);
    const host = await page.eval(`${formField("Host")}.value`);
    await page.eval(fill(formField("Name"), database.split("?")[0] || host));
  }
}

/** Fills the fields of the open form, by label. */
export async function fillForm(page, fields) {
  for (const [label, value] of Object.entries(fields)) {
    await page.eval(fill(formField(label), String(value)));
  }
}

/** Clicks Test connection and returns the notice once it shows. */
export async function testConnection(page, timeoutMs = 30_000) {
  // The notice of the previous test goes away when the next one starts: a new element.
  await page.eval(`document.querySelectorAll('.connection-dialog .notice').forEach(n => n.dataset.qaOld = '1')`);
  await page.eval(`${button(dialog, "Test connection")}.click()`);
  const fresh = `document.querySelector('.connection-dialog .notice.ok:not([data-qa-old]), .connection-dialog .notice.error:not([data-qa-old])')`;
  await page.waitFor(`${fresh} !== null && !${button(dialog, "Test connection")}.disabled`, timeoutMs);
  return page.eval(`${fresh}.textContent`);
}

/** Saves the open form and waits for it to close. */
export async function saveForm(page) {
  await page.eval(`${button(dialog, "Save")}.click()`);
  await page.waitFor(`${dialog} === null`, 15_000);
}

/** Clicks the Edit button of the connection row `name`. */
export async function editConnection(page, name) {
  await page.eval(`[...${treeRow(name)}.querySelectorAll('button')].find(b => b.title === 'Edit').click()`);
  await page.waitFor(`${dialog} !== null`);
}

/** Opens the context menu of the element `selector` and returns its entries. */
export async function menu(page, selector) {
  const r = await page.rect(selector);
  await page.click(r.x + 20, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  return page.eval(`[...document.querySelectorAll('.context-menu button')].map(b => b.textContent.trim())`);
}

/** Clicks the open context menu's entry that starts with `text`. */
export const menuItem = (page, text) =>
  page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.trim().startsWith(${JSON.stringify(text)})).click()`);

/** A `skip()` for a container on `port` of docker/compose.yml. */
export const needsPort = (port, what) => () =>
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

/** connections.toml with a SQLite connection, so the navigator shows a row at start. */
export function seedSqlite(dir, extra = "") {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000000c1"
name = "QA lite"
driver = "sqlite"
database = '${join(dir, "lite.db")}'
save_password = false
${extra}`,
  );
}

/** Expands the connection `name` in the navigator, answering the password prompt with
 *  `password` when there is one. */
export async function expand(page, name, password = null) {
  await page.waitFor(`${treeRow(name)} !== undefined`);
  await page.eval(`${treeRow(name)}.querySelector(".twisty").click()`);
  if (password !== null) {
    await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`, 15_000);
    await page.eval(fill(`document.querySelector('.modal input[type=password]')`, password));
    await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
    await page.waitFor(`document.querySelector('.modal input[type=password]') === null`, 20_000);
  }
}

/** Opens a query tab on the connection `name` (double click on its row). */
export async function openQueryTab(page, name) {
  const before = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await page.eval(`${treeRow(name)}.scrollIntoView({ block: 'center' })`);
  await page.eval(`${treeRow(name)}.id = 'qa-conn-row'`);
  await menu(page, "#qa-conn-row");
  await page.eval(`document.getElementById('qa-conn-row')?.removeAttribute('id')`);
  await menuItem(page, "New SQL script");
  await page.waitFor(`document.querySelectorAll('.query-tab').length > ${before} && ${visibleTab} !== undefined`, 20_000);
}

/** The cells of the visible result grid, row by row. */
export const gridRows = `[...${visibleTab}.querySelectorAll('[role=row]')].map(r => [...r.querySelectorAll('[role=gridcell]')].map(c => c.textContent)).filter(r => r.length)`;

/** Runs `sql` in the visible query tab and returns the status once it is done. */
export async function run(page, sql, timeoutMs = 20_000) {
  const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, 2);
  await page.type(sql);
  await page.waitFor(`[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n').includes(${JSON.stringify(sql.split("\n")[0])})`);
  // Any change of the tab's status bar or result after Ctrl+Enter: the old state cannot match.
  await page.eval(`(() => {
    window.__qaChanged = false;
    window.__qaObserver?.disconnect();
    const inEditor = (n) => (n.nodeType === 1 ? n : n.parentElement)?.closest('.cm-editor, .toolbar');
    window.__qaObserver = new MutationObserver((records) => {
      if (records.some((r) => !inEditor(r.target))) window.__qaChanged = true;
    });
    window.__qaObserver.observe(${visibleTab}, { subtree: true, childList: true, characterData: true });
  })()`);
  await page.key("Enter", "Enter", 13, 2);
  const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
  // A production connection asks before a statement that may change data: Run.
  const confirm = `[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === 'Run')`;
  await page.waitFor(`(() => { const c = ${confirm}; if (c) { c.click(); return false; } const s = ${visibleTab}.querySelector('.statusbar')?.textContent ?? ''; return window.__qaChanged && ${cancel}?.disabled && !s.includes('Executing'); })()`, timeoutMs);
  await page.sleep(150);
  return page.eval(`${status} || (${visibleTab}.querySelector('.notice')?.textContent ?? '')`);
}
