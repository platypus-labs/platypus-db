// Saved results: "Save result…" in the grid menu keeps the whole result as a Parquet file of the
// data folder; the panel under the scripts lists, searches, renames and deletes them; opening
// one reads the file into an in-memory DuckDB, so the rows, a chart, the column statistics and
// SQL on the result all work after the connection's database file is gone.
import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { chart, gridTsv, inField, liteConnection, markTitles, newEditor, profileOf, runQuery, showChart, status, toggle, treeRow, visibleTab } from "../analysis-helpers.mjs";

const NAME = "E2E saved results";
const QUERY = "SELECT region, amount, note FROM sales ORDER BY id";
const panel = `document.querySelector('.saved-results-panel')`;
const rows = `[...(${panel}?.querySelectorAll('.saved-result-row') ?? [])]`;
const rowNames = `${rows}.map(r => r.dataset.name)`;
const rowOf = (name) => `${rows}.find(r => r.dataset.name === ${JSON.stringify(name)})`;
const button = (name, label) => `${rowOf(name)}.querySelector('button[aria-label=${JSON.stringify(label)}]')`;
const dialogInput = `document.querySelector('.modal.name-dialog input')`;
const dialogSubmit = `document.querySelector('.modal.name-dialog button[type=submit]')`;
const savedDir = (dir) => join(dir, "saved-results");

export function setup({ dir }) {
  liteConnection(
    dir,
    NAME,
    "00000000-0000-4000-8000-0000000a0150",
    `CREATE TABLE sales (id INTEGER PRIMARY KEY, region TEXT, amount INTEGER, note TEXT);
     INSERT INTO sales VALUES (1, 'north', 10, 'a'), (2, 'south', 5, NULL), (3, 'north', 7, 'b'),
       (4, 'east', 3, NULL), (5, 'west', 8, 'c');
     CREATE TABLE many (id INTEGER PRIMARY KEY, cat TEXT);
     WITH RECURSIVE g(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM g WHERE n < 30000)
       INSERT INTO many SELECT n, 'c' || (n % 120) FROM g;`,
  );
}

/** The files of the saved results: the Parquet ones and the index's entries. */
function saved(dir) {
  const folder = savedDir(dir);
  const files = existsSync(folder) ? readdirSync(folder) : [];
  const index = files.includes("index.json") ? JSON.parse(readFileSync(join(folder, "index.json"), "utf8")).results : [];
  return {
    parquet: files.filter((f) => f.endsWith(".parquet")),
    leftovers: files.filter((f) => f.endsWith(".tmp") || f.endsWith(".part")),
    index,
  };
}

/** Right-clicks the visible grid and chooses `item` of its menu. */
async function gridMenu(page, item) {
  await page.waitFor(`${visibleTab}?.querySelector('.grid-scroll') != null`);
  await page.eval(`${visibleTab}.querySelector('.grid-scroll').id = 'e2e-saved-grid'`);
  const r = await page.rect("#e2e-saved-grid");
  await page.eval(`document.getElementById('e2e-saved-grid').removeAttribute('id')`);
  await page.click(r.x + 40, r.y + 40, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(
    `[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.trim().startsWith(${JSON.stringify(item)})).click()`,
  );
}

/** Replaces the text of the name dialog's field. */
async function typeName(page, text) {
  await page.waitFor(`${dialogInput} != null`);
  await page.eval(`${dialogInput}.focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  // Inserting no text would leave the selection in place.
  if (text === "") await page.key("Backspace", "Backspace", 8);
  else await page.type(text);
  await page.waitFor(`${dialogInput}.value === ${JSON.stringify(text)}`);
}

/** Saves the visible tab's result under `name` and waits for it to be listed. */
async function save(page, name) {
  await gridMenu(page, "Save result");
  await page.waitFor(`${dialogInput} != null`, 30_000);
  await typeName(page, name);
  await page.eval(`${dialogSubmit}.click()`);
  await page.waitFor(`${rowNames}.includes(${JSON.stringify(name)})`, 15_000);
}

/** Types `text` in the panel's search field. */
async function search(page, text) {
  await page.eval(`${panel}.querySelector('.saved-results-search').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(text);
  await page.waitFor(`${panel}.querySelector('.saved-results-search').value === ${JSON.stringify(text)}`);
}

export default async function (page, { dir }) {
  await newEditor(page, NAME);
  await runQuery(page, QUERY);
  assert.equal(await page.eval(status), "5 rows");

  // Nothing is saved yet.
  await page.waitFor(`${panel}?.textContent.includes('No saved results')`);
  assert.equal(await page.eval(rowNames + ".length"), 0);
  assert.equal(await page.eval(`${panel}.querySelector('.saved-results-search') ?? null`), null);
  assert.deepEqual(saved(dir).parquet, []);

  // The name dialog offers the first line of the query; an empty name cannot be saved, and
  // Cancel saves nothing.
  await gridMenu(page, "Save result");
  await page.waitFor(`${dialogInput}?.value === ${JSON.stringify(QUERY)}`, 30_000);
  await page.eval(`${dialogInput}.focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.key("Backspace", "Backspace", 8);
  await page.waitFor(`${dialogInput}.value === '' && ${dialogSubmit}.disabled`);
  await typeName(page, "   ");
  assert.equal(await page.eval(`${dialogSubmit}.disabled`), true);
  await page.eval(`[...document.querySelectorAll('.modal.name-dialog button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`document.querySelector('.modal.name-dialog') === null`);
  assert.equal(await page.eval(rowNames + ".length"), 0);
  assert.deepEqual(saved(dir).parquet, []);

  // Saved under a name: listed with its connection and rows, and written as a Parquet file
  // whose index entry keeps the query, the connection and the columns.
  await save(page, "Sales by region");
  const detail = await page.eval(`${rowOf("Sales by region")}.querySelector('.tree-detail').textContent`);
  assert.ok(detail.startsWith(`${NAME} · `), detail);
  assert.ok(detail.endsWith("5 rows"), detail);
  let files = saved(dir);
  assert.equal(files.parquet.length, 1);
  assert.deepEqual(files.leftovers, []);
  assert.equal(files.index.length, 1);
  const [entry] = files.index;
  assert.equal(files.parquet[0], `${entry.id}.parquet`);
  assert.equal(entry.name, "Sales by region");
  assert.equal(entry.sql, QUERY);
  assert.equal(entry.connection, NAME);
  assert.equal(entry.driver, "sqlite");
  assert.equal(entry.rows, 5);
  assert.equal(entry.partial, false);
  assert.deepEqual(
    entry.columns.map((c) => c.name),
    ["region", "amount", "note"],
  );
  assert.match(entry.saved_at, /^\d{4}-\d{2}-\d{2}T/);
  // The file is Parquet: it starts and ends with the magic number.
  const parquet = readFileSync(join(savedDir(dir), files.parquet[0]));
  assert.equal(parquet.subarray(0, 4).toString("latin1"), "PAR1");
  assert.equal(parquet.subarray(-4).toString("latin1"), "PAR1");

  // A result not read in full is loaded whole first; a long first line is cut.
  const long = `SELECT id, cat, 'a long first line of a statement that goes on past the limit' AS padding FROM many`;
  await runQuery(page, long);
  assert.match(await page.eval(status), /loaded · more available/);
  await gridMenu(page, "Save result");
  await page.waitFor(`${dialogInput} != null && ${dialogInput}.value !== ''`, 60_000);
  const offered = await page.eval(`${dialogInput}.value`);
  assert.ok(offered.endsWith("…") && [...offered].length <= 60 && long.startsWith(offered.slice(0, 40)), offered);
  await typeName(page, "Many rows");
  await page.eval(`${dialogSubmit}.click()`);
  await page.waitFor(`${rowNames}.includes('Many rows')`, 30_000);
  assert.match(await page.eval(`${rowOf("Many rows")}.querySelector('.tree-detail').textContent`), /30\D?000 rows$/);
  files = saved(dir);
  assert.equal(files.parquet.length, 2);
  assert.equal(files.index.find((r) => r.name === "Many rows").rows, 30000);
  assert.equal(files.index.find((r) => r.name === "Many rows").partial, false);

  // Search: every word must be in the name, the query or the connection.
  await search(page, "padding");
  await page.waitFor(`${rowNames}.length === 1 && ${rowNames}[0] === 'Many rows'`);
  await search(page, "REGION sales");
  await page.waitFor(`${rowNames}.length === 1 && ${rowNames}[0] === 'Sales by region'`);
  await search(page, "no such result");
  await page.waitFor(`${rowNames}.length === 0 && ${panel}.textContent.includes('No saved result matches')`);
  await search(page, "");
  await page.waitFor(`${rowNames}.length === 2`);

  // Rename: the dialog starts with the current name; an empty one cannot be saved.
  await page.eval(`${button("Sales by region", "Rename")}.click()`);
  await page.waitFor(`${dialogInput}?.value === 'Sales by region'`);
  await typeName(page, "");
  assert.equal(await page.eval(`${dialogSubmit}.disabled`), true);
  await typeName(page, "Sales 2026");
  await page.eval(`${dialogSubmit}.click()`);
  await page.waitFor(`${rowOf("Sales 2026")} != null && ${rowOf("Sales by region")} == null`);
  assert.deepEqual(saved(dir).index.map((r) => r.name).sort(), ["Many rows", "Sales 2026"]);
  assert.equal(saved(dir).parquet.length, 2);

  // Delete asks first; Cancel keeps it, Delete removes the entry and its file.
  await page.eval(`${button("Many rows", "Delete")}.click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') != null`);
  assert.match(await page.eval(`document.querySelector('.modal.confirm').textContent`), /Many rows/);
  await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') == null`);
  assert.equal(await page.eval(rowNames + ".length"), 2);
  await page.eval(`${button("Many rows", "Delete")}.click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') != null`);
  await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === 'Delete').click()`);
  await page.waitFor(`${rowOf("Many rows")} == null`);
  files = saved(dir);
  assert.equal(files.parquet.length, 1);
  assert.deepEqual(files.index.map((r) => r.name), ["Sales 2026"]);

  // Offline: the connection is closed and its database file deleted, then the saved result
  // opens all the same. (Windows keeps a file open under its sessions: Disconnect ends the
  // navigator's, the editor tab keeps its own until it is closed; the delete waits for the handles.)
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  const at = await page.eval(`(() => { const b = ${treeRow(NAME)}.getBoundingClientRect(); return { x: b.x, y: b.y, h: b.height }; })()`);
  await page.click(at.x + 30, at.y + at.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.trim().startsWith('Disconnect')).click()`);
  await page.eval(`document.querySelector('.tab [aria-label="Close tab"]').click()`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length === 0`, 15_000);
  const database = join(dir, "qa_ana.db");
  for (let i = 0; i < 100 && existsSync(database); i++) {
    try {
      rmSync(database);
    } catch {
      await page.sleep(100);
    }
  }
  assert.equal(existsSync(database), false, "the database file is deleted");

  const tabs = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await page.eval(`${rowOf("Sales 2026")}.querySelector('button[aria-label=Open]').click()`);
  // The notice is the new tab's own: the old tab shows no such text.
  await page.waitFor(
    `document.querySelectorAll('.query-tab').length === ${tabs + 1} && ${visibleTab}?.querySelector('.result-source')?.textContent.startsWith('Saved result of ${NAME} from ')`,
    30_000,
  );
  const notice = await page.eval(`${visibleTab}.querySelector('.result-source').textContent`);
  assert.match(notice, /5 rows/);
  assert.match(notice, /No connection is needed/);
  await page.waitFor(`${status} === '5 rows'`, 30_000);
  assert.equal(await page.eval(`${visibleTab}.querySelector('.editor .cm-content').textContent`), "SELECT * FROM result");
  assert.equal(await gridTsv(page), "region\tamount\tnote\nnorth\t10\ta\nsouth\t5\t\nnorth\t7\tb\neast\t3\t\nwest\t8\tc");

  // SQL on results works on it: NULLs came back as NULLs.
  const run = await inField(page, "SELECT count(*) AS n, count(note) AS notes, sum(amount) AS total FROM result");
  assert.equal(run.error, "");
  assert.equal(await gridTsv(page), "n\tnotes\ttotal\n5\t3\t33");
  // Escape in the field (reading the grid moved the focus to it) returns to the result.
  await page.eval(`${visibleTab}.querySelector('.result-sql .cm-content').focus()`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`${status} === '5 rows'`);

  // A chart of it: one bar per region, the amounts summed (north has two rows: 10 + 7).
  await showChart(page, "rect.chart-bar");
  const bars = await markTitles(page, 'rect.chart-bar');
  assert.equal(bars.length, 4, JSON.stringify(bars));
  assert.ok(bars.includes('north · amount: 17'), JSON.stringify(bars));
  await page.eval(`${toggle("Grid")}.click()`);
  await page.waitFor(`${chart} == null && ${visibleTab}.querySelector('.grid-scroll') != null`);

  // The column statistics of it: two NULLs among five, three distinct values.
  const note = await profileOf(page, "note");
  assert.match(note.text, /NULL values\s*40%/);
  assert.match(note.text, /Distinct\s*3/);

  // The last saved result goes too: the list says so, and the folder keeps no Parquet file.
  await page.eval(`${rowOf("Sales 2026")}.querySelector('button[aria-label=Delete]').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') != null`);
  await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === 'Delete').click()`);
  await page.waitFor(`${rowNames}.length === 0 && ${panel}.textContent.includes('No saved results')`);
  files = saved(dir);
  assert.deepEqual(files.parquet, []);
  assert.deepEqual(files.index, []);
  assert.deepEqual(files.leftovers, []);
}
