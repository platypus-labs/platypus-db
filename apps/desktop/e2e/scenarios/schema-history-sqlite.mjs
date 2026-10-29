// The schema history (group I) on a SQLite file: the tab is empty at first; a snapshot with a
// label is listed as a manual one with its table count; a second, identical one says nothing
// changed and stores nothing; after DDL run outside the app another is stored, and ticking
// both lists a table added, a column added and a type changed, grouped by table, with
// "Open script" putting the script that makes the change in the editor; the later snapshot
// against Now has no differences, the first against Now has the same changes as before;
// deleting a ticked snapshot (asked first) clears the comparison; and "Schedule…" opens the
// form of a snapshot task for the schema, refused without a schema, saved without a folder
// and listed with the scheduled tasks (it is not run here).
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expand, saveConnections, sqliteEntry } from "../perf.mjs";
import { palette, setField } from "../plans.mjs";
import { treeMenu } from "../qa-grid.mjs";

const NAME = "E2E History";
const tab = `[...document.querySelectorAll('.schema-history')].find(t => t.style.display !== 'none')`;
const rows = `${tab}.querySelectorAll('.history-row:not(.history-now)')`;
const editorText = `[...document.querySelectorAll('.cm-content')].map(e => e.textContent).join('\\n')`;
const button = (scope, text) => `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
const notice = `(${tab}.querySelector('.history-notice')?.textContent ?? '')`;
const group = (table) => `${tab}.querySelector('.history-table[data-table=${JSON.stringify(table)}]')`;
const form = `document.querySelector('.schedule-form')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "history.db"));
  db.exec(`CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT NOT NULL);
           CREATE TABLE orders (id INTEGER PRIMARY KEY, total REAL);`);
  db.close();
  saveConnections(dir, sqliteEntry(NAME, join(dir, "history.db")));
}

/** Ticks or unticks the checkbox of the row `id` (a snapshot's, or "now"). */
const tick = (page, id) => page.eval(`${tab}.querySelector('.history-row[data-id="${id}"] input[type=checkbox]').click()`);

/** The ids of the snapshot rows, newest first. */
const ids = (page) => page.eval(`[...${rows}].map(r => r.dataset.id)`);

export default async function (page, { dir }) {
  await expand(page, NAME);
  await treeMenu(page, "main", "Schema history");
  await page.waitFor(`${tab} !== undefined`, 15_000);

  // Nothing taken yet, and nothing to compare.
  await page.waitFor(`${tab}.querySelector('.history-empty') !== null`, 15_000);
  assert.equal(await page.eval(`${rows}.length`), 0);
  assert.ok(await page.eval(`${tab}.querySelector('.history-hint') !== null`), "asks to tick two states");

  // A first snapshot, with a label: manual, two tables.
  await setField(page, ".schema-history .history-label", "baseline");
  await page.eval(`${button(tab, "Take snapshot")}.click()`);
  await page.waitFor(`${rows}.length === 1`, 15_000);
  assert.equal(await page.eval(`${rows}[0].querySelector('[data-field="label"]').textContent`), "baseline");
  assert.equal(await page.eval(`${rows}[0].querySelector('[data-field="how"]').textContent`), "Manual");
  assert.equal(await page.eval(`${rows}[0].querySelector('[data-field="tables"]').textContent`), "2");
  assert.equal(await page.eval(notice), "Snapshot taken");
  assert.equal(await page.eval(`${tab}.querySelector('.history-label').value`), "", "the label is cleared once used");
  assert.ok(await page.eval(`${tab}.querySelector('.history-empty') === null`));

  // The same schema again: told so, and nothing stored.
  await page.eval(`${button(tab, "Take snapshot")}.click()`);
  await page.waitFor(`${notice} === 'No changes since the last snapshot'`, 15_000);
  assert.equal(await page.eval(`${rows}.length`), 1);

  // DDL outside the app: a table added, a column added, and a column's type changed.
  const db = new DatabaseSync(join(dir, "history.db"));
  db.exec(`CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT);
           ALTER TABLE customers ADD COLUMN phone TEXT;
           DROP TABLE orders;
           CREATE TABLE orders (id INTEGER PRIMARY KEY, total TEXT);`);
  db.close();
  await setField(page, ".schema-history .history-label", "after");
  await page.eval(`${button(tab, "Take snapshot")}.click()`);
  await page.waitFor(`${rows}.length === 2`, 15_000);
  assert.equal(await page.eval(notice), "Snapshot taken");
  const [after, baseline] = await ids(page);
  assert.equal(await page.eval(`${rows}[0].querySelector('[data-field="label"]').textContent`), "after", "the newest first");
  assert.equal(await page.eval(`${rows}[0].querySelector('[data-field="tables"]').textContent`), "3");

  // One state ticked is not a comparison; two are.
  await tick(page, baseline);
  assert.ok(await page.eval(`${tab}.querySelector('.history-hint') !== null`));
  await tick(page, after);
  await page.waitFor(`${group("products")} !== null`, 15_000);
  assert.equal(await page.eval(`${tab}.querySelector('.history-hint')`), null);
  assert.match(await page.eval(`${group("products")}.textContent`), /Table added/);
  assert.match(await page.eval(`${group("customers")}.textContent`), /Column phone added \(TEXT\)/);
  assert.match(await page.eval(`${group("orders")}.textContent`), /Column total: type REAL → TEXT/);
  assert.equal(await page.eval(`${tab}.querySelectorAll('.history-table').length`), 3, "one group per table that changed");

  // The script that makes the change, in the connection's editor.
  await page.eval(`${button(tab, "Open script")}.click()`);
  await page.waitFor(`(${editorText}).includes('products')`, 15_000);
  const script = await page.eval(editorText);
  assert.match(script, /Turns main as of/);
  assert.match(script, /CREATE TABLE/);
  assert.match(script, /phone/);

  // Back on the history: the later snapshot is what the database is now; the first is not.
  await page.eval(`[...document.querySelectorAll('.tab')].find(t => t.textContent.includes('History of main')).click()`);
  await page.waitFor(`${tab} !== undefined`);
  await tick(page, "now");
  await page.waitFor(`${tab}.querySelector('.history-same') !== null`, 15_000);
  assert.equal(await page.eval(`${tab}.querySelectorAll('.history-table').length`), 0);
  await tick(page, baseline);
  await page.waitFor(`${group("products")} !== null`, 15_000);
  assert.match(await page.eval(`${group("orders")}.textContent`), /type REAL → TEXT/);
  assert.equal(await page.eval(`${tab}.querySelector('.history-same')`), null);

  // Deleting a snapshot asks first, and takes it out of the comparison.
  const baselineRow = `${tab}.querySelector('.history-row[data-id="${baseline}"]')`;
  await page.eval(`${button(baselineRow, "Delete")}.click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`document.querySelector('.modal.confirm button.danger').click()`);
  await page.waitFor(`${rows}.length === 1`, 15_000);
  assert.deepEqual(await ids(page), [after]);
  await page.waitFor(`${tab}.querySelector('.history-hint') !== null`);
  assert.equal(await page.eval(`${tab}.querySelectorAll('.history-table').length`), 0);

  // Scheduled snapshots: the form starts on this schema, refuses an empty one, and has no folder.
  await page.eval(`${button(tab, "Schedule…")}.click()`);
  await page.waitFor(`${form} !== null`, 15_000);
  assert.equal(await page.eval(`${form}.querySelector('[data-field="kind"]').value`), "snapshot");
  assert.equal(await page.eval(`${form}.querySelector('[data-field="name"]').value`), "Snapshots of main");
  assert.equal(await page.eval(`${form}.querySelector('[data-field="snapshot-schema"]').value`), "main");
  assert.equal(await page.eval(`${form}.querySelector('[data-field="folder"]')`), null, "a snapshot writes no file");
  await setField(page, '.schedule-form [data-field="snapshot-schema"]', "");
  await page.eval(`${button(form, "Save")}.click()`);
  await page.waitFor(`${form}.querySelector('[data-field="error"]') !== null`, 15_000);
  assert.match(await page.eval(`${form}.querySelector('[data-field="error"]').textContent`), /schema/);
  await setField(page, '.schedule-form [data-field="snapshot-schema"]', "main");
  await page.eval(`${button(form, "Save")}.click()`);
  await page.waitFor(`${form} === null`, 15_000);

  // It is listed with the scheduled tasks, as a schema snapshot, and has not run.
  await palette(page, "Scheduled tasks");
  const row = `[...document.querySelectorAll('.schedule-row')].find(r => r.querySelector('strong')?.textContent === 'Snapshots of main')`;
  await page.waitFor(`${row} !== undefined`, 15_000);
  assert.match(await page.eval(`${row}.textContent`), /Schema snapshot/);
  assert.match(await page.eval(`${row}.children[1].textContent`), /Every day at 07:00/);
  assert.equal(await page.eval(`${row}.querySelector('[data-field="last"]').textContent`), "never");
  await page.eval(`${button("document.querySelector('.schedules-dialog')", "Close")}.click()`);
  await page.waitFor(`document.querySelector('.schedules-dialog') === null`);
}
