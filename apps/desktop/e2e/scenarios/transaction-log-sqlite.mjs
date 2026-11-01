// The transaction log of a tab: statements that committed on their own stand alone, those of
// a transaction come together with how it ended (a ROLLBACK, Commit from the toolbar), a
// failed statement is marked, and Clear empties the log.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { ALT, CTRL } from "../cdp.mjs";
import { confirmDanger } from "../admin.mjs";

const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
const toolbarButton = (label) => `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === ${JSON.stringify(label)})`;
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const dialog = `document.querySelector('.modal.tx-log')`;
const groups = `[...${dialog}.querySelectorAll('.tx-log-group')].map(g => ({ tone: g.className.replace('tx-log-group', '').trim(), head: g.querySelector('header').textContent, sql: [...g.querySelectorAll('code')].map(c => c.textContent), failed: g.querySelectorAll('li.failed').length }))`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "log.db"));
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT);");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-000000000911"
name = "Log db"
driver = "sqlite"
database = '${join(dir, "log.db")}'
save_password = false
`,
  );
}

/** Replaces the editor's text and runs it all, waiting for the run to end. */
async function runScript(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  // The script's lines joined (the content's own text has no line breaks), not those of the
  // SQL line over the results.
  await page.waitFor(`[...${visibleTab}.querySelectorAll('.cm-line')].filter(l => !l.closest('.sql-line')).map(l => l.textContent).join('\\n') === ${JSON.stringify(sql)}`);
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.key("x", "KeyX", 88, ALT);
  await page.waitFor(`${toolbarButton("Cancel")}?.disabled && ${status} !== '' && !${status}.includes('Executing')`, 30_000);
}

async function openLog(page) {
  await page.eval(`${toolbarButton("Transaction log")}.click()`);
  await page.waitFor(`${dialog} !== null`);
  return page.eval(groups);
}

async function closeLog(page) {
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent === 'Close').click()`);
  await page.waitFor(`${dialog} === null`);
}

export default async function (page) {
  await page.waitFor(`${row("Log db")} !== undefined`);
  await page.eval(`${row("Log db")}.querySelector('button[aria-label="Open SQL editor"]').click()`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 15_000);

  assert.deepEqual(await openLog(page), [], "nothing ran yet");
  await closeLog(page);

  await runScript(page, "INSERT INTO t VALUES (1, 'a');\nBEGIN;\nINSERT INTO t VALUES (2, 'b');\nSELECT count(*) FROM t;\nROLLBACK;");
  let log = await openLog(page);
  assert.equal(log.length, 2, JSON.stringify(log));
  assert.equal(log[0].tone, "rollback");
  assert.deepEqual(log[0].sql, ["ROLLBACK", "SELECT count(*) FROM t", "INSERT INTO t VALUES (2, 'b')", "BEGIN"]);
  assert.match(log[0].head, /Rolled back at/);
  assert.deepEqual([log[1].tone, log[1].sql], ["auto", ["INSERT INTO t VALUES (1, 'a')"]]);
  await closeLog(page);

  // A failed statement in a transaction ended by Commit from the toolbar.
  await runScript(page, "BEGIN;\nINSERT INTO t VALUES (3, 'c');\nINSERT INTO t VALUES (3, 'again');");
  await page.waitFor(`${visibleTab}.querySelector('.tx-open') !== null`, 15_000);
  log = await openLog(page);
  assert.deepEqual([log[0].tone, log[0].failed], ["open", 1], JSON.stringify(log));
  await closeLog(page);
  await page.eval(`${visibleTab}.querySelector('.tx-open').click()`);
  await page.waitFor(`document.querySelector('.modal.guard') !== null`);
  await page.eval(`[...document.querySelectorAll('.modal.guard button')].find(b => b.textContent === 'Commit').click()`);
  await page.waitFor(`${visibleTab}.querySelector('.tx-open') === null`, 15_000);
  log = await openLog(page);
  assert.equal(log[0].tone, "commit", JSON.stringify(log));
  assert.match(log[0].head, /Committed at/);

  // Clear asks first: Cancel keeps the log, the red button empties it.
  const clear = `[...${dialog}.querySelectorAll('button')].find(b => b.textContent === 'Clear')`;
  await page.eval(`${clear}.click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') === null`);
  assert.ok((await page.eval(`${dialog}.querySelectorAll('.tx-log-group').length`)) > 0, "Cancel keeps the log");
  await page.eval(`${clear}.click()`);
  await confirmDanger(page);
  await page.waitFor(`${dialog}.querySelectorAll('.tx-log-group').length === 0`);
  await closeLog(page);
}
