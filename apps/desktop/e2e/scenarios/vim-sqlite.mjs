// Vim mode of the SQL editor (Settings › SQL editor › Vim mode): normal mode keys edit and
// do not type, `i` and Esc switch modes shown under the text, `dd` and `u` delete and undo,
// Ctrl+Enter still runs, `:w` saves the script to a file, an unknown `:` command changes
// nothing, and turning the setting off in the Settings gives back plain typing at once.
import assert from "node:assert/strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { saveConnections, sqliteEntry, status, visibleTab } from "../perf.mjs";
import { answerSave } from "../qa-xfer.mjs";

const NAME = "E2E Vim";
const content = `${visibleTab}.querySelector('.cm-content')`;
const text = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const mode = `(${visibleTab}.querySelector('.cm-vim-panel')?.textContent ?? '')`;
const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;

export function setup({ dir }) {
  const file = join(dir, "vim.db");
  const db = new DatabaseSync(file);
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO t (name) VALUES ('a'), ('b'), ('c');");
  db.close();
  saveConnections(dir, sqliteEntry(NAME, file));
  writeFileSync(join(dir, "settings.toml"), "[editor]\nvim = true\n");
}

/** Presses the keys of `keys`, one by one, as typed letters. */
async function keys(page, keys) {
  for (const ch of keys) {
    await page.key(ch, ch === ":" ? "Semicolon" : `Key${ch.toUpperCase()}`, ch === ":" ? 186 : ch.toUpperCase().charCodeAt(0), ch === ":" ? 8 : 0);
    await page.sleep(30);
  }
}

/** Types `command` on Vim's `:` command line and presses Enter. */
async function exCommand(page, command) {
  await page.eval(`${content}.focus()`);
  await keys(page, ":");
  await page.waitFor(`${visibleTab}.querySelector('.cm-vim-panel input') != null`);
  await page.type(command);
  await page.key("Enter", "Enter", 13);
}

export default async function (page, { dir }) {
  const row = `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(NAME)})`;
  await page.eval(`${row}.id = 'e2e-connection'`);
  await page.clickSel("#e2e-connection", { clickCount: 2 });
  await page.waitFor(`${content} != null`);
  await page.eval(`${content}.focus()`);
  await page.waitFor(`${mode}.includes('NORMAL')`);

  // Normal mode: letters are commands, nothing is typed.
  await keys(page, "xyz");
  assert.equal(await page.eval(text), "", "normal mode types nothing");

  // Insert mode, shown under the text, then back with Esc.
  await keys(page, "i");
  await page.waitFor(`${mode}.includes('INSERT')`);
  await page.type("SELECT name FROM t ORDER BY id;\nSELECT 'second';");
  await page.waitFor(`${text} === ${JSON.stringify("SELECT name FROM t ORDER BY id;\nSELECT 'second';")}`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`${mode}.includes('NORMAL')`);

  // dd deletes the line, u brings it back.
  await keys(page, "dd");
  await page.waitFor(`${text} === "SELECT name FROM t ORDER BY id;"`);
  await keys(page, "u");
  await page.waitFor(`${text} === ${JSON.stringify("SELECT name FROM t ORDER BY id;\nSELECT 'second';")}`);

  // Ctrl+Enter runs the statement under the cursor, as without Vim.
  await keys(page, "gg");
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${cancel}?.disabled === true && ${status} !== '' && !${status}.includes('Executing')`, 15_000);
  assert.equal(await page.eval(status), "3 rows");

  // An unknown command: an error in the panel, the text unchanged.
  await exCommand(page, "nosuchcommand");
  await page.waitFor(`${visibleTab}.querySelector('.cm-vim-panel input') === null`);
  assert.match(await page.eval(mode), /Not an editor command/, "the panel says why");
  assert.equal(await page.eval(text), "SELECT name FROM t ORDER BY id;\nSELECT 'second';");

  // :w saves the script, as Ctrl+S: a new script asks where.
  const file = join(dir, "vim.sql");
  await answerSave(page, file);
  await exCommand(page, "w");
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('vim.sql')`, 10_000);
  assert.ok(existsSync(file));
  assert.equal(readFileSync(file, "utf8"), "SELECT name FROM t ORDER BY id;\nSELECT 'second';");

  // Turned off in the Settings: plain typing at once, the mode line gone.
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.settingsTab("SQL editor");
  const toggle = `[...document.querySelectorAll('.modal.settings label')].find(l => l.textContent.includes('Vim mode'))?.querySelector('input[type=checkbox]')`;
  await page.waitFor(`${toggle} != null && ${toggle}.checked`);
  await page.eval(`${toggle}.click()`);
  await page.waitFor(`!${toggle}.checked`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
  await page.waitFor(`${visibleTab}.querySelector('.cm-vim-panel') === null`);
  await page.eval(`${content}.focus()`);
  await page.key("End", "End", 35, CTRL);
  await page.type(" -- x");
  await page.waitFor(`${text}.endsWith("SELECT 'second'; -- x")`);
}
