// Scheduled tasks (group J) on a SQLite connection, run with "Run now": a script that
// changes rows, done, with its count; one that fails, with the statement that did and no
// retry for a run by hand; a CSV export into a folder made for it, where with "Files kept"
// at 1 a second run leaves one file; a task without its file refused on Save; disabling a
// task leaves it without a next run; and deleting it, asked first. Each run is listed under
// its task, the newest first.
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { palette, setField } from "../plans.mjs";
import { saveConnections, sqliteEntry } from "../perf.mjs";
import { treeMenu } from "../qa-grid.mjs";

const NAME = "E2E Schedules";
const dialog = `document.querySelector('.schedules-dialog')`;
const row = (name) =>
  `[...document.querySelectorAll('.schedule-row')].find(r => r.querySelector('strong')?.textContent === ${JSON.stringify(name)})`;
const last = (name) => `(${row(name)}?.querySelector('[data-field="last"]')?.textContent ?? '')`;
const button = (scope, text) => `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;

export function setup({ dir }) {
  const file = join(dir, "schedules.db");
  const db = new DatabaseSync(file);
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v INTEGER); INSERT INTO t (v) VALUES (1), (2), (3);");
  db.close();
  writeFileSync(join(dir, "bump.sql"), "UPDATE t SET v = v + 10;\nUPDATE t SET v = v + 1 WHERE id = 1;\n");
  writeFileSync(join(dir, "broken.sql"), "UPDATE t SET v = 0 WHERE id = 1;\nUPDATE no_such_table SET v = 1;\n");
  saveConnections(dir, sqliteEntry(NAME, file));
}

/** Fills the open form with `fields` (`data-field` → value) and saves it. */
async function fillAndSave(page, fields) {
  for (const [field, value] of Object.entries(fields)) {
    await page.waitFor(`document.querySelector('.schedule-form [data-field="${field}"]') !== null`);
    await setField(page, `.schedule-form [data-field="${field}"]`, value);
  }
  await page.eval(`${button("document.querySelector('.schedule-form')", "Save")}.click()`);
}

/** Runs the task `name` now and waits for its last run to be newer than it was. */
async function runNow(page, name) {
  const before = await page.eval(`${row(name)}.querySelectorAll('[data-field="last"] .admin-badge').length + ':' + ${last(name)}`);
  await page.eval(`${button(row(name), "Run now")}.click()`);
  await page.waitFor(`!${row(name)}.querySelector('[data-field="next"] .admin-badge.running') && ${row(name)}.querySelectorAll('[data-field="last"] .admin-badge').length + ':' + ${last(name)} !== ${JSON.stringify(before)}`, 30_000);
}

export default async function (page, { dir }) {
  const db = join(dir, "schedules.db");
  const values = () => {
    const conn = new DatabaseSync(db);
    const out = conn.prepare("SELECT v FROM t ORDER BY id").all().map((r) => r.v);
    conn.close();
    return out;
  };

  // From the connection's menu: the form starts on it; a script without its file is refused.
  await treeMenu(page, NAME, "Schedule a task…");
  await page.waitFor(`document.querySelector('.schedule-form') !== null`);
  await fillAndSave(page, { name: "Bump" });
  await page.waitFor(`document.querySelector('.schedule-form [data-field="error"]') !== null`);
  assert.match(await page.eval(`document.querySelector('.schedule-form [data-field="error"]').textContent`), /SQL file/);
  await fillAndSave(page, { script: join(dir, "bump.sql") });
  await page.waitFor(`${dialog} === null`);

  // The list, from the command palette: the task runs at 7 every day.
  await palette(page, "Scheduled tasks");
  await page.waitFor(`${row("Bump")} !== undefined`);
  assert.match(await page.eval(`${row("Bump")}.children[1].textContent`), /Every day at 07:00/);
  assert.match(await page.eval(`${row("Bump")}.textContent`), /Run a SQL script/);
  assert.equal(await page.eval(last("Bump")), "never");

  await runNow(page, "Bump");
  assert.match(await page.eval(last("Bump")), /^done/);
  assert.deepEqual(values(), [12, 12, 13]);
  await page.eval(`${row("Bump")}.children[1].click()`);
  await page.waitFor(`document.querySelector('.schedule-runs li') !== null`);
  assert.match(await page.eval(`document.querySelector('.schedule-runs li').textContent`), /2 statements run, 4 rows changed/);
  assert.match(await page.eval(`document.querySelector('.schedule-runs li').textContent`), /by hand/);
  await page.eval(`${row("Bump")}.children[1].click()`);

  // A script that fails: the statement that did, and nothing retried for a run by hand.
  await page.eval(`${button(dialog, "New task…")}.click()`);
  await fillAndSave(page, { name: "Broken", script: join(dir, "broken.sql"), retries: "2" });
  await page.waitFor(`${row("Broken")} !== undefined`);
  await runNow(page, "Broken");
  assert.match(await page.eval(last("Broken")), /^failed/);
  assert.match(await page.eval(`${row("Broken")}.querySelector('[data-field="last"] .admin-badge').title`), /statement 2: .*no_such_table/);
  assert.notEqual(await page.eval(`${row("Broken")}.querySelector('[data-field="next"]').textContent`), "running");
  // The statements run one at a time, as Run SQL file runs them: the one before the failure stays.
  assert.deepEqual(values(), [0, 12, 13]);

  // An export into a folder made for it; with one file kept, a second run leaves one.
  const folder = join(dir, "exports");
  await page.eval(`${button(dialog, "New task…")}.click()`);
  await page.waitFor(`document.querySelector('.schedule-form') !== null`);
  await setField(page, `.schedule-form [data-field="kind"]`, "export");
  await fillAndSave(page, { name: "Values", sql: "SELECT id, v FROM t ORDER BY id", folder, keep: "1" });
  await page.waitFor(`${row("Values")} !== undefined`);
  await runNow(page, "Values");
  assert.match(await page.eval(last("Values")), /^done/, await page.eval(`${row("Values")}.querySelector('[data-field="last"] .admin-badge')?.title`));
  assert.ok(existsSync(folder), "the folder was made");
  let files = readdirSync(folder);
  assert.equal(files.length, 1, `${files}`);
  assert.match(files[0], /^Values-\d{8}-\d{6}\.csv$/);
  assert.match(readFileSync(join(folder, files[0]), "utf8"), /^id,v\r?\n1,0\r?\n2,12/);
  // A second later: another stamp, and the first file goes. The last run reads the same to the
  // minute: the new file in the folder says the run ended.
  const first = files[0];
  await page.sleep(1100);
  await page.eval(`${button(row("Values"), "Run now")}.click()`);
  const replaced = () => {
    const now = readdirSync(folder);
    return now.length === 1 && now[0] !== first;
  };
  for (let i = 0; i < 300 && !replaced(); i++) await page.sleep(100);
  await page.waitFor(`!${row("Values")}.querySelector('[data-field="next"] .admin-badge.running')`, 30_000);
  files = readdirSync(folder);
  assert.equal(files.length, 1, `one kept: ${files}`);
  assert.notEqual(files[0], first);

  // Disabled: no next run. Deleted after asking.
  await page.eval(`${row("Broken")}.querySelector('input[type=checkbox]').click()`);
  await page.waitFor(`${row("Broken")}.querySelector('[data-field="next"]').textContent === 'off'`);
  await page.eval(`${button(row("Broken"), "Delete")}.click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`document.querySelector('.modal.confirm button.danger').click()`);
  await page.waitFor(`${row("Broken")} === undefined`);
  assert.ok(await page.eval(`${row("Bump")} !== undefined`), "Bump stays");

  // Kept in the data folder for the next start.
  const saved = JSON.parse(readFileSync(join(dir, "schedules.json"), "utf8"));
  assert.deepEqual(saved.schedules.map((s) => s.name).sort(), ["Bump", "Values"]);
  await page.eval(`${button(dialog, "Close")}.click()`);
  await page.waitFor(`${dialog} === null`);
}
