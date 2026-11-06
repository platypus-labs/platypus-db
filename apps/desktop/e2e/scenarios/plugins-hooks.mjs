// Event hooks of a plugin (Settings > Plugins, examples/plugins/query-guard). A package that does
// not ask for the permission, or names one the app does not know, is refused by name. A hook blocks
// a statement with a message naming the plugin; it sees the statement with sensitive literals and
// passwords masked. The app's own checks come first and stay: a hook that allows does not lift the
// question before a DELETE without WHERE, nor the read-only connection's refusal, and a refused
// statement never runs. A hook that crashes or loops lets the statement run and the user is told
// once. It runs on connect and after a statement (in the log), and may block a save of edited
// rows. An update that asks for a new permission says so before installing. Turned off, removed or
// with its permission taken away, the plugin stops nothing.
//
// The sandbox's limits, the time limit and the cost of a hook are tested in Rust
// (`platypus-plugins`, tests/hooks.rs), where they can be provoked directly.
import assert from "node:assert/strict";
import { cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { pressF2 } from "../qa-grid.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const EXAMPLES = resolve(here, "../../../../examples/plugins");
const NAME = "E2E hooks";
const READ_ONLY = "E2E hooks read-only";
const BLOCKED = "The plugin “Query guard” blocked this:";

const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const editBar = `(${visibleTab}?.querySelector('.edit-bar')?.textContent ?? '')`;
const resultError = `(${visibleTab}?.querySelector('.result-error')?.textContent ?? '')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const modalButton = (text) => `[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
const panel = `document.querySelector('.plugins-panel')`;
const pluginRow = (id) => `${panel}?.querySelector('.driver-block[data-plugin="${id}"]:not(.plugin-review)')`;
const review = `${panel}?.querySelector('[data-field="plugin-review"]')`;
const panelError = `(${panel}?.querySelector('[data-field="plugin-error"]')?.textContent ?? '')`;
const toasts = `[...document.querySelectorAll('.toast')].map(t => t.textContent).join(' | ')`;

function manifestOf(name) {
  return readFileSync(join(EXAMPLES, name, "plugin.toml"), "utf8");
}

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "hooks.db"));
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, email TEXT);
           INSERT INTO items VALUES (1, 'apple', 'a@b.c'), (2, 'pear', 'p@b.c');
           CREATE TABLE forbidden_table (id INTEGER PRIMARY KEY, v TEXT);
           INSERT INTO forbidden_table VALUES (1, 'kept');
           CREATE TABLE locked_items (id INTEGER PRIMARY KEY, v TEXT);
           INSERT INTO locked_items VALUES (1, 'kept');`);
  db.close();
  const packages = join(dir, "packages");
  cpSync(join(EXAMPLES, "query-guard"), join(packages, "query-guard"), { recursive: true });
  // The same module under a manifest that does not ask for the permission.
  const noPermission = join(packages, "no-permission");
  mkdirSync(noPermission, { recursive: true });
  cpSync(join(EXAMPLES, "query-guard", "plugin.wasm"), join(noPermission, "plugin.wasm"));
  writeFileSync(join(noPermission, "plugin.toml"), manifestOf("query-guard").replace("hooks = true", ""));
  // And under one that asks for something the app does not know.
  const unknown = join(packages, "unknown-permission");
  mkdirSync(unknown, { recursive: true });
  cpSync(join(EXAMPLES, "query-guard", "plugin.wasm"), join(unknown, "plugin.wasm"));
  writeFileSync(join(unknown, "plugin.toml"), `${manifestOf("query-guard")}files = true\n`);
  // A plugin whose second version asks for more than its first.
  for (const [folder, version, permissions] of [
    ["updated-1", "1.0.0", ""],
    ["updated-2", "2.0.0", `[permissions]\nhttp = ["api.example.com"]\nclipboard = true\n`],
  ]) {
    const home = join(packages, folder);
    mkdirSync(home, { recursive: true });
    cpSync(join(EXAMPLES, "settings-demo", "plugin.wasm"), join(home, "plugin.wasm"));
    writeFileSync(
      join(home, "plugin.toml"),
      `id = "updated"\nname = "Updated"\nversion = "${version}"\napi = 2\nkinds = ["actions"]\n${permissions}`,
    );
  }
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 8

[[connections]]
id = "00000000-0000-4000-8000-0000000e0c01"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "hooks.db")}'
save_password = false

[[connections]]
id = "00000000-0000-4000-8000-0000000e0c02"
name = "${READ_ONLY}"
driver = "sqlite"
database = '${join(dir, "hooks.db")}'
save_password = false
read_only = true
`,
  );
  writeFileSync(join(dir, "settings.toml"), `version = 1\n\n[drivers]\nauto_update = false\n`);
}

async function openPluginsPage(page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Plugins");
  await page.waitFor(`${panel} !== null`, 15_000);
}

async function closeSettings(page) {
  await page.eval(`document.querySelector('.modal.settings [role="tab"][aria-selected="true"]')?.focus()`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

/** Picks a package folder and waits for its review, or an error. */
async function pickPackage(page, dir, name) {
  await page.eval(`(() => { const e = ${panel}.querySelector('[data-field="plugin-error"]'); if (e) e.textContent = ''; })()`);
  await page.eval(`window.__platypus.answer("folder", ${JSON.stringify(join(dir, "packages", name))})`);
  await page.eval(`${panel}.querySelector('[data-action="install-folder"]').click()`);
  await page.waitFor(`${review} !== null || ${panelError} !== ''`, 15_000);
}

async function confirmInstall(page, id) {
  await page.eval(`${review}.querySelector('[data-action="confirm-install"]').click()`);
  await page.waitFor(`${pluginRow(id)} != null && ${review} === null`, 15_000);
}

function pluginState(dir, id) {
  return JSON.parse(readFileSync(join(dir, "plugins", "plugins.json"), "utf8")).plugins[id];
}

async function installs(page, dir) {
  await openPluginsPage(page);

  // Refused by name, before any review.
  await pickPackage(page, dir, "no-permission");
  assert.match(await page.eval(panelError), /permissions\.hooks/);
  assert.equal(await page.eval(`${review} === null`), true);
  await pickPackage(page, dir, "unknown-permission");
  assert.match(await page.eval(panelError), /files/);
  assert.equal(await page.eval(`${review} === null`), true);

  // The review says, in plain words, what hooks may do.
  await pickPackage(page, dir, "query-guard");
  const text = await page.eval(`${review}.textContent`);
  assert.match(text, /Runs before a statement, and may stop it/);
  assert.match(text, /Runs after a statement/);
  assert.match(text, /Runs when a connection opens/);
  assert.match(text, /Runs before edited rows are saved, and may stop it/);
  assert.match(text, /see statements with sensitive values masked/);
  assert.equal(await page.eval(`${review}.querySelector('[data-field="added-permissions"]') === null`), true, "a first install has no version to compare with");
  await confirmInstall(page, "query-guard");
  assert.equal(pluginState(dir, "query-guard").approved.hooks, true);
  assert.match(await page.eval(`${pluginRow("query-guard")}.textContent`), /see statements with sensitive values masked/);

  // An update asking for more says what is new, and nothing changes until it is approved.
  await pickPackage(page, dir, "updated-1");
  await confirmInstall(page, "updated");
  await pickPackage(page, dir, "updated-2");
  const added = await page.eval(`${review}.querySelector('[data-field="added-permissions"]').textContent`);
  assert.match(added, /api\.example\.com/);
  assert.match(added, /clipboard/);
  await page.eval(`${review}.querySelector('[data-action="cancel-install"]').click()`);
  await page.waitFor(`${review} === null`);
  assert.equal(pluginState(dir, "updated").version, "1.0.0");
  assert.deepEqual(pluginState(dir, "updated").approved.http, []);
  await pickPackage(page, dir, "updated-2");
  await confirmInstall(page, "updated");
  assert.deepEqual(pluginState(dir, "updated").approved.http, ["api.example.com"]);
  // Asking for nothing new, the review shows no such list.
  await pickPackage(page, dir, "updated-2");
  assert.equal(await page.eval(`${review}.querySelector('[data-field="added-permissions"]') === null`), true);
  await page.eval(`${review}.querySelector('[data-action="cancel-install"]').click()`);
  await page.waitFor(`${review} === null`);
  await closeSettings(page);
}

async function setEditor(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent === ${JSON.stringify(sql)}`);
  await page.key("Escape", "Escape", 27); // closes the completion list
}

/** Empties what shows how the last run went, then runs the editor's statement. */
async function press(page, sql) {
  await setEditor(page, sql);
  await page.eval(`(() => {
    const tab = ${visibleTab};
    const s = tab?.querySelector('.statusbar span');
    if (s) s.textContent = '';
    const e = tab?.querySelector('.result-error');
    if (e) e.textContent = '';
  })()`);
  await page.key("Enter", "Enter", 13, CTRL);
}

/** Runs a statement that goes through; returns its final status. */
async function runs(page, sql) {
  await press(page, sql);
  await page.waitFor(`(() => { const s = ${status}; return s !== '' && !s.includes('Executing') && !s.includes('more available'); })()`, 20_000);
  return page.eval(status);
}

/** Runs a statement a hook stops; returns the error shown. */
async function refused(page, sql) {
  await press(page, sql);
  await page.waitFor(`${resultError} !== ''`, 20_000);
  return page.eval(resultError);
}

async function openConnection(page, name) {
  const tabs = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await page.eval(`${treeRow(name)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-conn').removeAttribute('id')`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length === ${tabs + 1} && ${visibleTab}?.querySelector('.cm-content') != null`, 20_000);
}

/** The app's log, once it holds `text`. */
async function logWith(dir, text) {
  const deadline = Date.now() + 15_000;
  let log = "";
  while (Date.now() < deadline) {
    try {
      log = readFileSync(join(dir, "logs", "platypus.log"), "utf8");
    } catch {
      // Not written yet.
    }
    if (log.includes(text)) return log;
    await new Promise((r) => setTimeout(r, 100));
  }
  assert.fail(`the log does not hold ${JSON.stringify(text)}:\n${log.slice(-2000)}`);
}

async function blocksStatements(page, db) {
  assert.equal(await runs(page, "SELECT * FROM items"), "2 rows");

  const error = await refused(page, "SELECT * FROM forbidden_table");
  assert.ok(error.includes(`${BLOCKED} forbidden_table is off limits.`), error);

  // What the hook sees: the literal of a sensitive column masked, the others not.
  const seen = await refused(page, "SELECT id /* show_hook_view */ FROM items WHERE email = 'a@b.c' AND name = 'apple'");
  assert.ok(seen.includes(BLOCKED), seen);
  assert.ok(seen.includes("'••••••'"), seen);
  assert.ok(!seen.includes("a@b.c"), seen);
  assert.ok(seen.includes("'apple'"), seen);

  // Never a password.
  const secret = await refused(page, "CREATE USER bob WITH PASSWORD 'hunter2' /* show_hook_view */");
  assert.ok(secret.includes(BLOCKED), secret);
  assert.ok(!secret.includes("hunter2"), secret);

  // The block is the statement's error: the table has its rows.
  assert.equal(db.prepare("SELECT count(*) AS n FROM forbidden_table").get().n, 1);
}

async function safetyChecksComeFirst(page, db) {
  // A hook allows this one, and the question is asked all the same; declined, it does not run.
  await setEditor(page, "DELETE FROM items");
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`document.querySelector('.modal.guard') !== null`, 15_000);
  await page.eval(`${modalButton("Cancel")}.click()`);
  await page.waitFor(`document.querySelector('.modal.guard') === null`);
  assert.equal(db.prepare("SELECT count(*) AS n FROM items").get().n, 2);

  // The user says yes and the hook still refuses: it can only add a refusal.
  await setEditor(page, "DELETE FROM forbidden_table");
  await page.eval(`(() => { const e = ${visibleTab}?.querySelector('.result-error'); if (e) e.textContent = ''; })()`);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`document.querySelector('.modal.guard') !== null`, 15_000);
  await page.eval(`${modalButton("Run")}.click()`);
  await page.waitFor(`${resultError}.includes(${JSON.stringify(BLOCKED)})`, 20_000);
  assert.equal(db.prepare("SELECT count(*) AS n FROM forbidden_table").get().n, 1);
}

async function readOnlyStays(page, db) {
  await openConnection(page, READ_ONLY);
  // Allowed by the hook and refused by the connection, before it.
  await setEditor(page, "UPDATE items SET name = 'x'");
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${visibleTab}.querySelector('.notice.warn')?.textContent.includes('read-only') === true`, 15_000);
  assert.equal(await page.eval(`${visibleTab}.textContent.includes(${JSON.stringify(BLOCKED)})`), false);
  assert.equal(db.prepare("SELECT count(*) AS n FROM items WHERE name = 'x'").get().n, 0);
  // A hook can still refuse what the connection allows.
  const error = await refused(page, "SELECT * FROM forbidden_table");
  assert.ok(error.includes(BLOCKED), error);
}

async function brokenHooksDoNotBlock(page) {
  await page.eval(`document.querySelectorAll('.toast').forEach(t => t.click())`);
  await page.waitFor(`document.querySelectorAll('.toast').length === 0`);
  // A crash: the statement runs, and the user is told.
  assert.equal(await runs(page, "SELECT 1 AS one /* hook_crash */"), "1 row");
  await page.waitFor(`/hook of the plugin “Query guard” failed and the action went ahead/.test(${toasts})`, 15_000);
  await page.eval(`document.querySelectorAll('.toast').forEach(t => t.click())`);
  await page.waitFor(`document.querySelectorAll('.toast').length === 0`);
  // An endless loop: the statement runs too, after the hook's fuel or time ended, and the user
  // is not told again.
  assert.equal(await runs(page, "SELECT 2 AS two /* hook_endless */"), "1 row");
  assert.equal(await page.eval(toasts), "");
  // And the plugin goes on working: each call has a new instance.
  const error = await refused(page, "SELECT * FROM forbidden_table");
  assert.ok(error.includes(BLOCKED), error);
}

async function tellsTheLog(page, dir) {
  const log = await logWith(dir, "plugin query-guard: on_connect");
  assert.match(log, new RegExp(`on_connect "${NAME}"`));
  assert.equal(await runs(page, "SELECT id FROM items WHERE email = 'p@b.c'"), "1 row");
  const after = await logWith(dir, "email = '••••••'");
  assert.match(after, /plugin query-guard: after_query "ok"/);
  assert.ok(!after.includes("p@b.c"), "an email in the log");
  assert.ok(!after.includes("hunter2"), "a password in the log");
}

/** Focuses a cell of the grid and writes `text` in it. */
async function editCell(page, row, col, text) {
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("Home", "Home", 36, CTRL);
  for (let i = 0; i < row; i++) await page.key("ArrowDown", "ArrowDown", 40);
  for (let i = 0; i < col; i++) await page.key("ArrowRight", "ArrowRight", 39);
  await pressF2(page, ".cell-editor");
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(text);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`document.querySelector('.cell-editor') === null`);
}

async function savesGoThroughTheHook(page, dir, db) {
  // Edits of a table the hook locks: refused, and the row stays.
  assert.equal(await runs(page, "SELECT * FROM locked_items"), "1 row");
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
  await editCell(page, 0, 1, "changed");
  await page.waitFor(`${editBar}.includes('1 row changed')`);
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("s", "KeyS", 83, CTRL);
  await page.waitFor(`(${toasts} + ${editBar} + ${visibleTab}.textContent).includes('locked_items cannot be edited.')`, 20_000);
  assert.equal(db.prepare("SELECT v FROM locked_items WHERE id = 1").get().v, "kept");
  await page.eval(`document.querySelectorAll('.toast').forEach(t => t.click())`);
  // Discards them, as a new statement asks.
  await setEditor(page, "SELECT * FROM items ORDER BY id");
  await page.key("Enter", "Enter", 13, CTRL);
  const discard = modalButton("Discard");
  await page.waitFor(`${discard} !== undefined`, 15_000);
  await page.eval(`${discard}.click()`);
  await page.waitFor(`${status} === '2 rows' && ${editBar}.includes('to edit a cell')`, 20_000);

  // Edits of another table are saved, and the hook hears only how many.
  await editCell(page, 0, 1, "quince");
  await page.waitFor(`${editBar}.includes('1 row changed')`);
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').focus()`);
  await page.key("s", "KeyS", 83, CTRL);
  await page.waitFor(`${editBar}.includes('to edit a cell')`, 20_000);
  assert.equal(db.prepare("SELECT name FROM items WHERE id = 1").get().name, "quince");
  const log = await logWith(dir, 'before_save "items": 1 updates, 0 inserts, 0 deletes');
  assert.ok(!log.includes("quince"), "an edited value in the log");
}

async function goesAwayWithThePlugin(page, dir, db) {
  // Its permission taken away: the plugin stays installed and stops nothing.
  const stateFile = join(dir, "plugins", "plugins.json");
  const state = JSON.parse(readFileSync(stateFile, "utf8"));
  state.plugins["query-guard"].approved.hooks = false;
  writeFileSync(stateFile, JSON.stringify(state));
  await openPluginsPage(page);
  // A change to the plugins reads them again.
  await page.eval(`${pluginRow("query-guard")}.querySelector('[data-action="toggle"]').click()`);
  await page.waitFor(`${pluginRow("query-guard")}.dataset.enabled === 'false'`);
  await page.eval(`${pluginRow("query-guard")}.querySelector('[data-action="toggle"]').click()`);
  await page.waitFor(`${pluginRow("query-guard")}.dataset.enabled === 'true'`);
  assert.doesNotMatch(await page.eval(`${pluginRow("query-guard")}.textContent`), /see statements with sensitive values masked/);
  await closeSettings(page);
  assert.equal(await runs(page, "SELECT * FROM forbidden_table"), "1 row");

  // Approved again by installing it again; then turned off, it stops nothing.
  await openPluginsPage(page);
  await pickPackage(page, dir, "query-guard");
  assert.match(await page.eval(`${review}.querySelector('[data-field="added-permissions"]').textContent`), /see statements with sensitive values masked/);
  await page.eval(`${review}.querySelector('[data-action="confirm-install"]').click()`);
  await page.waitFor(`${review} === null`, 15_000);
  await closeSettings(page);
  const error = await refused(page, "SELECT * FROM forbidden_table");
  assert.ok(error.includes(BLOCKED), error);

  await openPluginsPage(page);
  await page.eval(`${pluginRow("query-guard")}.querySelector('[data-action="toggle"]').click()`);
  await page.waitFor(`${pluginRow("query-guard")}.dataset.enabled === 'false'`);
  await closeSettings(page);
  assert.equal(await runs(page, "SELECT * FROM forbidden_table"), "1 row");

  // Removed: asked first, and nothing of it is left.
  await openPluginsPage(page);
  await page.eval(`${pluginRow("query-guard")}.querySelector('[data-action="remove"]').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`document.querySelector('.modal.confirm button.danger, .modal.confirm button.primary').click()`);
  await page.waitFor(`${pluginRow("query-guard")} == null`);
  assert.equal(existsSync(join(dir, "plugins", "query-guard")), false);
  await closeSettings(page);
  assert.equal(await runs(page, "SELECT * FROM forbidden_table"), "1 row");
  assert.equal(db.prepare("SELECT count(*) AS n FROM forbidden_table").get().n, 1);
}

export default async function (page, { dir }) {
  const db = new DatabaseSync(join(dir, "hooks.db"));
  try {
    await page.waitFor(`${treeRow(NAME)} !== undefined`);
    await installs(page, dir);
    await openConnection(page, NAME);
    await blocksStatements(page, db);
    await safetyChecksComeFirst(page, db);
    await brokenHooksDoNotBlock(page);
    await tellsTheLog(page, dir);
    await savesGoThroughTheHook(page, dir, db);
    await readOnlyStays(page, db);
    await goesAwayWithThePlugin(page, dir, db);
  } finally {
    db.close();
  }
}
