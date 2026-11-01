// UX P2 fixes in server administration, scheduled tasks, security and agents (SQLite part;
// the admin panel resize and the ended-session toast are in admin-parameters-postgres.mjs and
// admin-sessions-postgres.mjs):
// - Snapshots: Delete asks first (Cancel keeps it, the red button deletes and says so), and the
//   restore script opens in a tab named after the table, with the table and verb in its comment;
// - Agents: the introduction folds behind "How it works", the Security rows are reachable and
//   chosen with the keyboard, the details panel has a resizer, an empty Activity offers
//   "Connect an agent", which shows Setup;
// - Scheduled tasks: the empty list has "New task…" as its action, which opens the form.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { confirmDanger } from "../admin.mjs";
import { palette } from "../plans.mjs";

const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const modal = `document.querySelector('.modal.snapshots')`;
const items = `${modal}.querySelectorAll('.snapshot-list li').length`;
const toasts = `[...document.querySelectorAll('.toaster .toast')].map(t => t.textContent)`;
const button = (scope, text) => `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "ux.db"));
  db.exec(`CREATE TABLE accounts (id INTEGER PRIMARY KEY, owner TEXT);
    INSERT INTO accounts VALUES (1, 'a'), (2, 'b');`);
  db.close();
  const entry = (n, name) => `
[[connections]]
id = "00000000-0000-4000-8000-00000000094${n}"
name = "${name}"
driver = "sqlite"
database = '${join(dir, "ux.db")}'
save_password = false
agent_access = "read_only"
`;
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${entry(1, "Ux admin")}${entry(2, "Ux other")}`);
}

/** Replaces the visible editor's text and runs it, waiting for the run to end. */
async function runScript(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent.includes(${JSON.stringify(sql.slice(-12))})`);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${status} !== '' && !${status}.includes('Executing')`, 30_000);
}

async function snapshots(page) {
  await page.waitFor(`${row("Ux admin")} !== undefined`);
  await page.eval(`${row("Ux admin")}.querySelector('button[aria-label="Open SQL editor"]').click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);
  await runScript(page, "UPDATE accounts SET owner = 'x' WHERE id = 1;");
  await runScript(page, "DELETE FROM accounts WHERE id = 2;");

  await palette(page, "Snapshots before changes");
  await page.waitFor(`${modal} !== null && ${items} === 2`, 15_000);
  // Cancel keeps the snapshot.
  await page.eval(`${button(modal + ".querySelector('.snapshot-list li')", "Delete")}.click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  assert.match(await page.eval(`document.querySelector('.modal.confirm p').textContent`), /Delete the snapshot of (DELETE|UPDATE)/);
  await page.eval(`${button("document.querySelector('.modal.confirm')", "Cancel")}.click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') === null`);
  assert.equal(await page.eval(items), 2, "Cancel keeps both");
  // The red button deletes and says so.
  await page.eval(`${button(modal + ".querySelector('.snapshot-list li')", "Delete")}.click()`);
  await confirmDanger(page);
  await page.waitFor(`${items} === 1`, 15_000);
  await page.waitFor(`${toasts}.includes('Snapshot deleted')`);

  // The restore script opens in a tab named after the table, and its comment says what it is.
  const tabs = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await page.eval(`${button(modal, "Open restore script")}.click()`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length === ${tabs + 1} && document.querySelector('.modal.snapshots') === null`, 15_000);
  await page.waitFor(`[...document.querySelectorAll('.tab')].some(t => /Restore of .*accounts/.test(t.textContent))`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content')?.textContent.includes('Puts back the rows of ')`, 15_000);
  const text = await page.eval(`${visibleTab}.querySelector('.cm-content').textContent`);
  assert.doesNotMatch(text, /snapshot \d{6,}/, "no raw id in the comment");

  // The last one gone: the list says so, with a sentence, not a spinner.
  await palette(page, "Snapshots before changes");
  await page.waitFor(`${modal} !== null && ${items} === 1`, 15_000);
  await page.eval(`${button(modal, "Delete")}.click()`);
  await confirmDanger(page);
  await page.waitFor(`${modal}.querySelector('.snapshots-empty') !== null`, 15_000);
  assert.equal(await page.eval(`${modal}.querySelector('.snapshots-empty').getAttribute('aria-busy')`), null);
  await page.eval(`${button(modal, "Close")}.click()`);
  await page.waitFor(`${modal} === null`);
}

async function agents(page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Agents");
  // The introduction is one line that opens on demand.
  await page.waitFor(`document.querySelector('.agents-intro') !== null`);
  assert.equal(await page.eval(`document.querySelector('.agents-intro').open`), false);
  assert.match(await page.eval(`document.querySelector('.agents-intro summary').textContent`), /How it works/);
  await page.eval(`document.querySelector('.agents-intro summary').click()`);
  await page.waitFor(`document.querySelector('.agents-intro').open === true`);

  // Activity with no call: an action that leads to Setup.
  await page.eval(`document.querySelector('[data-agents-page="activity"]').click()`);
  await page.waitFor(`document.querySelector('.agent-calls tbody button') !== null`, 15_000);
  await page.eval(`document.querySelector('.agent-calls tbody button').click()`);
  await page.waitFor(`document.querySelector('[data-agents-panel="setup"]').hidden === false`);

  // Security: the rows take the keyboard.
  await page.eval(`document.querySelector('[data-agents-page="security"]').click()`);
  const rows = `document.querySelectorAll('.agents-connections tbody tr')`;
  await page.waitFor(`${rows}.length === 2`);
  assert.equal(await page.eval(`[...${rows}].every(r => r.tabIndex === 0)`), true, "the rows are focusable");
  const selected = `document.querySelector('.agents-connections tr.selected')?.dataset.connection`;
  const first = await page.eval(selected);
  const key = (key) => `document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: ${JSON.stringify(key)}, bubbles: true, cancelable: true }))`;
  await page.eval(`${rows}[0].focus()`);
  await page.eval(key("ArrowDown"));
  assert.equal(first, "Ux admin");
  await page.waitFor(`${selected} === 'Ux other'`);
  assert.equal(await page.eval(selected), "Ux other", "the arrow moves to the next connection");
  assert.equal(await page.eval(`document.activeElement.dataset.connection`), "Ux other", "and the focus follows");
  await page.eval(key("ArrowUp"));
  await page.waitFor(`${selected} === 'Ux admin'`);
  await page.eval(`${rows}[1].focus()`);
  await page.eval(key("Enter"));
  await page.waitFor(`${selected} === 'Ux other'`);

  // The details panel has a handle: Left grows it, Home brings it back.
  const width = `document.querySelector('.agent-detail').getBoundingClientRect().width`;
  const handle = `document.querySelector('.agents-split .panel-resizer')`;
  const base = await page.eval(width);
  await page.eval(`${handle}.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true, cancelable: true }))`);
  await page.waitFor(`${width} > ${base}`);
  await page.eval(`${handle}.dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', bubbles: true, cancelable: true }))`);
  await page.waitFor(`Math.abs(${width} - ${base}) < 1`);
  await page.eval(`${button("document.querySelector('.modal.settings')", "Done")}.click()`);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

async function schedules(page) {
  await palette(page, "Scheduled tasks");
  const dialog = `document.querySelector('.schedules-dialog')`;
  await page.waitFor(`${dialog}?.querySelector('.schedules-empty') != null`, 15_000);
  // "New task…" is the action of the empty list, in the list itself.
  await page.eval(`${button(dialog + ".querySelector('.schedules-empty')", "New task…")}.click()`);
  await page.waitFor(`document.querySelector('.schedule-form') !== null`);
  // Retries at 0 leave the delay between them off.
  await page.eval(`(() => {
    const el = document.querySelector('.schedule-form [data-field="retries"]');
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, '0');
    el.dispatchEvent(new Event('input', { bubbles: true }));
  })()`);
  await page.waitFor(`document.querySelector('.schedule-form [data-field="retry_minutes"]').disabled === true`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.schedule-form') === null`);
  await page.waitFor(`${dialog}.querySelector('.schedules-empty') != null`);
  await page.eval(`${button(dialog, "Close")}.click()`);
  await page.waitFor(`document.querySelector('.schedules-dialog') === null`);
}

export default async function (page) {
  await snapshots(page);
  await agents(page);
  await schedules(page);
}
