// Snapshots before changes: an UPDATE and a DELETE run from the editor keep the rows they
// change; the transaction log's Restore… opens a script that puts them back, and running it
// does; Snapshots… lists them. An UPDATE whose subquery joins still reads one table: it has
// one too.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { ALT, CTRL } from "../cdp.mjs";
import { palette } from "../plans.mjs";

const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const toolbarButton = (label) => `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === ${JSON.stringify(label)})`;
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const log = `document.querySelector('.modal.tx-log')`;
const tabCount = `document.querySelectorAll('.query-tab').length`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "shop.db"));
  db.exec(`CREATE TABLE accounts (id INTEGER PRIMARY KEY, owner TEXT, balance REAL);
    INSERT INTO accounts VALUES (1, 'it''s me', 10.5), (2, NULL, 0), (3, 'c', -1);`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000931"
name = "Shop db"
driver = "sqlite"
database = '${join(dir, "shop.db")}'
save_password = false
`,
  );
}

const accounts = (dir) => {
  const db = new DatabaseSync(join(dir, "shop.db"));
  try {
    return db.prepare("SELECT id, owner, balance FROM accounts ORDER BY id").all().map((r) => ({ ...r }));
  } finally {
    db.close();
  }
};

/** Replaces the visible editor's text and runs it all, waiting for the run to end. */
async function runScript(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent.includes(${JSON.stringify(sql.slice(-20))})`);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("x", "KeyX", 88, ALT);
  await page.waitFor(`${toolbarButton("Cancel")}?.disabled && ${status} !== '' && !${status}.includes('Executing')`, 30_000);
}

/** Opens the transaction log and clicks the Restore… of its `n`-th one (newest first). */
async function restoreFromLog(page, n) {
  await page.eval(`${toolbarButton("Transaction log")}.click()`);
  await page.waitFor(`${log} !== null`);
  const tabs = await page.eval(tabCount);
  await page.eval(`[...${log}.querySelectorAll('.tx-log-restore')][${n}].click()`);
  await page.waitFor(`${tabCount} === ${tabs + 1} && ${visibleTab}.querySelector('.cm-content').textContent.includes('Puts back the rows')`, 15_000);
}

/** Runs the restore script the visible tab holds, as it is. */
async function runShown(page) {
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("x", "KeyX", 88, ALT);
  await page.waitFor(`${toolbarButton("Cancel")}?.disabled && ${status} !== '' && !${status}.includes('Executing')`, 30_000);
}

export default async function (page, { dir }) {
  const before = accounts(dir);
  await page.waitFor(`${row("Shop db")} !== undefined`);
  await page.eval(`${row("Shop db")}.querySelector('button[aria-label="Open SQL editor"]').click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);

  await runScript(page, "UPDATE accounts SET owner = 'x', balance = 0 WHERE id <= 2;");
  assert.equal(accounts(dir)[0].owner, "x");
  await restoreFromLog(page, 0);
  assert.match(await page.eval(`${visibleTab}.querySelector('.cm-content').textContent`), /UPDATE "main"\."accounts" SET/);
  await runShown(page);
  assert.deepEqual(accounts(dir), before, "the update undone");

  // Back in the first editor: a DELETE, then a join a snapshot cannot read, which still runs.
  await page.eval(`document.querySelectorAll('.tab')[0].click()`);
  await page.waitFor(`${visibleTab}.querySelector('.cm-content').textContent.includes('UPDATE accounts')`, 15_000);
  await runScript(page, "DELETE FROM accounts WHERE balance <= 0;\nUPDATE accounts SET owner = 'y' WHERE id IN (SELECT a.id FROM accounts a JOIN accounts b ON a.id = b.id);");
  assert.equal(accounts(dir).length, 1);
  await page.eval(`${toolbarButton("Transaction log")}.click()`);
  await page.waitFor(`${log} !== null`);
  assert.equal(await page.eval(`${log}.querySelectorAll('.tx-log-restore').length`), 3, "the UPDATE with a subquery reads one table: it has one too");
  await page.eval(`[...${log}.querySelectorAll('button')].find(b => b.textContent === 'Close').click()`);
  await restoreFromLog(page, 1);
  await runShown(page);
  const restored = accounts(dir);
  assert.equal(restored.length, 3, JSON.stringify(restored));
  assert.deepEqual(restored.slice(1), before.slice(1), "the deleted rows are back");

  // Snapshots… lists every one, newest first.
  await palette(page, "Snapshots before changes");
  await page.waitFor(`document.querySelectorAll('.modal.snapshots .snapshot-list li').length === 3`, 15_000);
  assert.match(await page.eval(`document.querySelector('.modal.snapshots .snapshot-list li').textContent`), /UPDATE/);
}
