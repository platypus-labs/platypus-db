// WebAssembly plugins (Settings > Plugins) with the example packages of examples/plugins:
// a package that does not match its manifest is refused with its reason; the review shows
// what a package provides and the permissions it asks for, Cancel installs nothing, Install
// copies it to <data>/plugins; a second install of a version says what it replaces. The
// actions of sql-tools run from the "Plugins" submenus of the editor (and its shortcut), the
// grid (onto the clipboard) and the navigator (a new SQL tab); turned off they are gone, and a
// removed plugin leaves no folder behind. The toy driver shows in the driver picker with a
// plugin field in the form, and a saved connection through it runs statements against a
// local HTTP server, shows the server's errors, and fills the navigator.
//
// The sandbox's limits (endless loops, memory, traps), the HTTP permissions and .zip packages
// are tested in Rust (`platypus-plugins`), where they can be provoked directly.
import assert from "node:assert/strict";
import { cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";
import { ALT, CTRL, SHIFT } from "../cdp.mjs";
import { setValue } from "../admin-users.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const EXAMPLES = resolve(here, "../../../../examples/plugins");
const NAME = "E2E plugins";
const TOY = "E2E toy";
const TOKEN = "t0ken";
// The toy server's port, fixed before the app starts: the saved connection names it.
const PORT = 41000 + Math.floor(Math.random() * 4000);

const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const editorText = `[...${visibleTab}.querySelector('.cm-content').querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const menuButton = (label) =>
  `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === ${JSON.stringify(label)})`;
const panel = `document.querySelector('.plugins-panel')`;
const pluginRow = (id) => `${panel}?.querySelector('.driver-block[data-plugin="${id}"]:not(.plugin-review)')`;
const review = `${panel}?.querySelector('[data-field="plugin-review"]')`;
const panelError = `(${panel}?.querySelector('[data-field="plugin-error"]')?.textContent ?? '')`;
const toast = `(document.querySelector('.toast-item')?.textContent ?? '')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "plugins.db"));
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);
           INSERT INTO items VALUES (1, 'apple'), (2, 'pear|green'), (3, NULL);`);
  db.close();
  // The packages as a user would have them, outside the data folder's plugins.
  const packages = join(dir, "packages");
  cpSync(join(EXAMPLES, "sql-tools"), join(packages, "sql-tools"), { recursive: true });
  cpSync(join(EXAMPLES, "toy-http-driver"), join(packages, "toy-http-driver"), { recursive: true });
  // A module of actions under a manifest that promises a driver.
  mkdirSync(join(packages, "mismatch"), { recursive: true });
  writeFileSync(
    join(packages, "mismatch", "plugin.toml"),
    `id = "mismatch"\nname = "Mismatch"\nversion = "0.1.0"\napi = 1\nkinds = ["driver"]\n`,
  );
  cpSync(join(EXAMPLES, "sql-tools", "plugin.wasm"), join(packages, "mismatch", "plugin.wasm"));
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 8

[[connections]]
id = "00000000-0000-4000-8000-0000000e0b01"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "plugins.db")}'
save_password = false

[[connections]]
id = "00000000-0000-4000-8000-0000000e0b02"
name = "${TOY}"
driver = "plugin"
host = "127.0.0.1"
port = ${PORT}
database = "shop"
user = ""
ssl_mode = "disable"
save_password = false
password_command = "echo ${TOKEN}"

[connections.options]
plugin = "toy-http-driver"
`,
  );
  writeFileSync(join(dir, "settings.toml"), `version = 1\n\n[drivers]\nauto_update = false\n`);
}

/** A database speaking the toy driver's API (see examples/plugins/toy-http-driver). */
function toyServer(seen) {
  const answer = (req, body) => {
    if (req.headers.authorization !== `Bearer ${TOKEN}`) return [401, { error: "bad token" }];
    if (req.method === "GET" && req.url === "/shop/hello") return [200, { product: "ToyDB", version: "1.2" }];
    if (req.method === "POST" && req.url === "/shop/query") {
      const sql = JSON.parse(body).sql.trim();
      seen.push(sql);
      if (/^select/i.test(sql)) {
        return [200, { columns: [{ name: "id", type: "int" }, { name: "fruit" }], rows: [[1, "apple"], [2, "pear"], [3, null]] }];
      }
      if (/^delete/i.test(sql)) return [200, { affected: 2 }];
      return [400, { error: `syntax error near "${sql}"` }];
    }
    if (req.method === "GET" && req.url === "/shop/tables") return [200, ["fruits"]];
    if (req.method === "GET" && req.url === "/shop/tables/fruits") {
      return [200, { columns: [{ name: "id", type: "int", nullable: false, primary_key: true }, { name: "fruit", type: "text" }] }];
    }
    return [404, { error: "not found" }];
  };
  const server = createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      const [code, json] = answer(req, body);
      res.writeHead(code, { "Content-Type": "application/json" });
      res.end(JSON.stringify(json));
    });
  });
  return new Promise((done) => server.listen(PORT, "127.0.0.1", () => done(server)));
}

async function openPluginsPage(page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Plugins");
  await page.waitFor(`${panel} !== null`);
}

async function closeSettings(page) {
  // Escape closes the dialog only while the focus is inside it, and a dialog that was just
  // closed or a row that was just removed leaves it elsewhere: focus its selected tab, then press.
  await page.eval(`document.querySelector('.modal.settings [role="tab"][aria-selected="true"]')?.focus()`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

/** Picks a package folder in Settings > Plugins and waits for its review, or an error. */
async function pickPackage(page, dir, name) {
  await page.eval(`(() => { const e = ${panel}.querySelector('[data-field="plugin-error"]'); if (e) e.textContent = ''; })()`);
  await page.eval(`window.__platypus.answer("folder", ${JSON.stringify(join(dir, "packages", name))})`);
  await page.eval(`${panel}.querySelector('[data-action="install-folder"]').click()`);
  await page.waitFor(`${review} !== null || ${panelError} !== ''`, 15_000);
}

async function setEditor(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
}

/** Runs the editor's statement and waits for its final status (blanked before, so not the old
 *  one; "N rows loaded · more available" is the first block of a fetch that goes on). */
async function run(page, sql) {
  await setEditor(page, sql);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`(() => { const s = ${status}; return s !== '' && !s.includes('Executing') && !s.includes('more available'); })()`, 20_000);
  return page.eval(status);
}

/** Opens the context menu at `selector`, then its Plugins submenu; returns the submenu's items. */
async function pluginsMenu(page, selector, dx = 40, dy = 10) {
  const r = await page.rect(selector);
  assert.ok(r, `not found: ${selector}`);
  await page.click(r.x + dx, r.y + dy, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  if ((await page.eval(`${menuButton("Plugins")} === undefined`)) === true) return null;
  await page.eval(`${menuButton("Plugins")}.click()`);
  await page.waitFor(`document.querySelector('.context-menu.submenu') !== null`);
  return page.eval(`[...document.querySelectorAll('.context-menu.submenu button span:first-child')].map(s => s.textContent)`);
}

async function closeMenu(page) {
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);
}

async function installs(page, dir) {
  await openPluginsPage(page);
  assert.match(await page.eval(`${panel}.textContent`), /No plugin installed\./);

  // A package whose module does not match its manifest: the reason, and no review.
  await pickPackage(page, dir, "mismatch");
  assert.match(await page.eval(panelError), /lacks "actions"/);
  assert.equal(await page.eval(`${review} === null`), true);

  // The review: what it provides and what it asks; Cancel installs nothing.
  await pickPackage(page, dir, "sql-tools");
  assert.equal(await page.eval(`${review}.dataset.plugin`), "sql-tools");
  const text = await page.eval(`${review}.textContent`);
  assert.match(text, /SQL tools · 1\.0\.0/);
  assert.match(text, /Action “Uppercase SQL keywords” in SQL editor/);
  assert.match(text, /Action “Copy rows as Markdown table” in Result grid/);
  assert.match(text, /Action “Count rows” in Navigator/);
  assert.match(text, /Put text on the clipboard/);
  assert.equal(await page.eval(`${review}.querySelector('[data-field="replaces"]') === null`), true);
  await page.eval(`${review}.querySelector('[data-action="cancel-install"]').click()`);
  await page.waitFor(`${review} === null`);
  assert.equal(existsSync(join(dir, "plugins", "sql-tools")), false, "Cancel installs nothing");

  // Install and allow.
  await pickPackage(page, dir, "sql-tools");
  await page.eval(`${review}.querySelector('[data-action="confirm-install"]').click()`);
  await page.waitFor(`${pluginRow("sql-tools")} != null && ${review} === null`, 15_000);
  assert.equal(await page.eval(`${pluginRow("sql-tools")}.dataset.enabled`), "true");
  assert.ok(existsSync(join(dir, "plugins", "sql-tools", "1.0.0", "plugin.wasm")));
  const state = JSON.parse(readFileSync(join(dir, "plugins", "plugins.json"), "utf8"));
  assert.equal(state.plugins["sql-tools"].enabled, true);
  assert.equal(state.plugins["sql-tools"].approved.clipboard, true);

  // The driver plugin asks for nothing.
  await pickPackage(page, dir, "toy-http-driver");
  assert.match(await page.eval(`${review}.textContent`), /Driver “Toy HTTP database”/);
  assert.match(await page.eval(`${review}.textContent`), /No permissions/);
  await page.eval(`${review}.querySelector('[data-action="confirm-install"]').click()`);
  await page.waitFor(`${pluginRow("toy-http-driver")} != null && ${review} === null`, 15_000);

  // The same package again says what it replaces.
  await pickPackage(page, dir, "sql-tools");
  assert.match(await page.eval(`${review}.querySelector('[data-field="replaces"]').textContent`), /1\.0\.0/);
  await page.eval(`${review}.querySelector('[data-action="cancel-install"]').click()`);
  await page.waitFor(`${review} === null`);
  await closeSettings(page);
}

async function editorActions(page) {
  await page.eval(`${treeRow(NAME)}.id = 'e2e-sqlite'`);
  await page.clickSel("#e2e-sqlite", { clickCount: 2 });
  await page.waitFor(`${visibleTab} !== undefined`);

  // From the menu, on the whole text: keywords only, not strings or names.
  await setEditor(page, "select name as \"from\" from items where name = 'select' and id is not null");
  await page.eval(`${visibleTab}.querySelector('.cm-content').id = 'e2e-editor'`);
  const items = await pluginsMenu(page, "#e2e-editor");
  assert.ok(items?.includes("Uppercase SQL keywords"), `${items}`);
  // The grid's and the navigator's plugin actions are not offered in the editor.
  assert.equal(items.includes("Count rows"), false);
  await page.eval(`${menuButton("Uppercase SQL keywords")}.click()`);
  await page.waitFor(`${editorText} === ${JSON.stringify("SELECT name AS \"from\" FROM items WHERE name = 'select' AND id IS NOT NULL")}`, 15_000);

  // The shortcut on a selection: only the selected text changes.
  await setEditor(page, "select 1;\nselect 2");
  await page.key("Home", "Home", 36, CTRL);
  for (let i = 0; i < 6; i++) await page.key("ArrowRight", "ArrowRight", 39, SHIFT);
  await page.key("u", "KeyU", 85, CTRL | ALT);
  await page.waitFor(`${editorText} === 'SELECT 1;\\nselect 2'`, 15_000);
}

async function gridAction(page) {
  assert.equal(await run(page, "SELECT id, name FROM items ORDER BY id"), "3 rows");
  await page.waitFor(`${visibleTab}.querySelector('.grid-scroll') !== null`);
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').id = 'e2e-grid'`);
  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
  await page.eval(`window.__copied = undefined`);
  await page.eval(`document.querySelector('#e2e-grid').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  const items = await pluginsMenu(page, "#e2e-grid", 40, 52);
  assert.deepEqual(items.filter((i) => i === "Copy rows as Markdown table"), ["Copy rows as Markdown table"]);
  await page.eval(`${menuButton("Copy rows as Markdown table")}.click()`);
  await page.waitFor(`window.__copied !== undefined`, 15_000);
  assert.equal(
    await page.eval(`window.__copied`),
    "| id | name |\n| --- | --- |\n| 1 | apple |\n| 2 | pear\\|green |\n| 3 | NULL |\n",
  );
  await page.waitFor(`${toast}.includes('Copied')`);
  await page.eval(`document.querySelector('.toast-item')?.click()`);
}

async function navigatorAction(page) {
  await page.eval(`${treeRow(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow("items")} !== undefined`, 15_000);
  await page.eval(`${treeRow("items")}.id = 'e2e-items'`);
  const tabs = await page.eval(`document.querySelectorAll('.query-tab').length`);
  const items = await pluginsMenu(page, "#e2e-items", 20, 8);
  assert.ok(items?.includes("Count rows"), `${items}`);
  await page.eval(`${menuButton("Count rows")}.click()`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length === ${tabs + 1}`, 15_000);
  await page.waitFor(`/^SELECT COUNT\\(\\*\\) FROM .*items.*;$/.test(${editorText})`);
  // The tab is on the same connection: it runs there.
  assert.equal(await run(page, await page.eval(editorText)), "1 row");
}

async function turnOffAndRemove(page, dir) {
  await openPluginsPage(page);
  await page.eval(`${pluginRow("sql-tools")}.querySelector('[data-action="toggle"]').click()`);
  await page.waitFor(`${pluginRow("sql-tools")}.dataset.enabled === 'false'`);
  await closeSettings(page);
  // Turned off: no Plugins submenu in the editor any more (the toy driver has no actions).
  await page.eval(`${visibleTab}.querySelector('.cm-content').id = 'e2e-editor-off'`);
  assert.equal(await pluginsMenu(page, "#e2e-editor-off"), null);
  await closeMenu(page);

  await openPluginsPage(page);
  await page.eval(`${pluginRow("sql-tools")}.querySelector('[data-action="toggle"]').click()`);
  await page.waitFor(`${pluginRow("sql-tools")}.dataset.enabled === 'true'`);
  // Removing asks first.
  await page.eval(`${pluginRow("sql-tools")}.querySelector('[data-action="remove"]').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  assert.match(await page.eval(`document.querySelector('.modal.confirm').textContent`), /Remove the plugin SQL tools\?/);
  await page.eval(`document.querySelector('.modal.confirm button.danger, .modal.confirm button.primary').click()`);
  await page.waitFor(`${pluginRow("sql-tools")} == null`);
  assert.equal(existsSync(join(dir, "plugins", "sql-tools")), false);
  assert.ok(existsSync(join(dir, "plugins", "toy-http-driver")));
  await closeSettings(page);
}

async function driverPicker(page) {
  await page.eval(`document.querySelector('button[title="New connection"]').click()`);
  const picker = `document.querySelector('.modal.driver-picker')`;
  await page.waitFor(`${picker} !== null`);
  const tile = `[...document.querySelectorAll('.driver-tile')].find(b => b.textContent === 'Toy HTTP database')`;
  await page.waitFor(`${tile} !== undefined`);
  assert.match(await page.eval(`${picker}.textContent`), /Plugins/);
  // The search finds it by its name.
  await setValue(page, ".driver-picker .modal-search input", "toy");
  await page.waitFor(`[...document.querySelectorAll('.driver-tile')].length === 1 && ${tile} !== undefined`);
  await page.eval(`${tile}.click()`);
  await page.waitFor(`document.querySelector('.connection-dialog [data-field="plugin-driver"] select') !== null`);
  assert.equal(await page.eval(`document.querySelector('.connection-dialog [data-field="plugin-driver"] select').value`), "toy-http-driver");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.connection-dialog') === null`);
}

async function driverConnection(page, seen) {
  await page.eval(`document.querySelector('.toast')?.click()`);
  const tabs = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await page.eval(`${treeRow(TOY)}.id = 'e2e-toy'`);
  await page.clickSel("#e2e-toy", { clickCount: 2 });
  await page.waitFor(`document.querySelectorAll('.query-tab').length === ${tabs + 1}`, 20_000);

  // The statement goes to the server, the token with it (the server refuses others).
  assert.equal(await run(page, "SELECT * FROM fruits"), "3 rows");
  assert.ok(seen.includes("SELECT * FROM fruits"), seen.join(", "));

  // The server's error reaches the result as it wrote it.
  // An error leaves the status bar empty, so there is no status to wait for: the error is.
  await setEditor(page, "NONSENSE");
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`/syntax error near "NONSENSE"/.test(${visibleTab}.querySelector('.result-error')?.textContent ?? '') || /syntax error near/.test(${toast})`, 15_000);

  // A statement without rows reports what it changed.
  // A DELETE without WHERE asks first; the plugin's driver is no exception.
  await setEditor(page, "DELETE FROM fruits");
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("Enter", "Enter", 13, CTRL);
  const guard = `[...document.querySelectorAll('.modal button')].find(b => b.textContent === 'Run')`;
  await page.waitFor(`${guard} !== undefined`, 15_000);
  await page.eval(`${guard}.click()`);
  await page.waitFor(`/2/.test(${status})`, 20_000);

  // The navigator: the plugin's schema and table.
  await page.eval(`${treeRow(TOY)}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow("fruits")} !== undefined`, 15_000);
}

export default async function (page, { dir }) {
  const seen = [];
  const server = await toyServer(seen);
  try {
    await page.waitFor(`${treeRow(NAME)} !== undefined`);
    await installs(page, dir);
    await editorActions(page);
    await gridAction(page);
    await navigatorAction(page);
    await turnOffAndRemove(page, dir);
    await driverPicker(page);
    await driverConnection(page, seen);
  } finally {
    server.close();
  }
}
