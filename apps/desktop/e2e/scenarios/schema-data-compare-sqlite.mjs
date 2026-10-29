// The data comparison of two SQLite databases (Compare data with… on a table): a composite key
// with rows added, changed and removed, the counts and the sampled rows, the script with and
// without the DELETE of the rows missing from the source, a filter, a bad filter, identical
// tables, tables without a key, and the script opened in the editor of the target.
import assert from "node:assert/strict";
import { DST, ENGINES, SRC, buttonIn, openPath, queryText, rowMenu, saveEngines } from "../qa3-schema.mjs";

const SOURCE = "QA3 SQLite src";
const TARGET = "QA3 SQLite dst";
const dlg = `document.querySelector('.modal.data-compare')`;
const counts = `(${dlg}?.querySelector('.data-diff-counts')?.textContent ?? '')`;
const samples = `(${dlg}?.querySelector('.lint-findings')?.textContent ?? '')`;
const notice = `(${dlg}?.querySelector('.notice:not(.error)')?.textContent ?? '')`;
const errorText = `(${dlg}?.querySelector('.notice.error')?.textContent ?? '')`;
const done = `${dlg}?.querySelector('.data-diff-counts') != null || ${dlg}?.querySelector('.notice.error') != null`;

export function setup({ dir }) {
  ENGINES.sqlite.seed(
    dir,
    SRC,
    `CREATE TABLE stock (warehouse TEXT NOT NULL, sku INTEGER NOT NULL, qty INTEGER, note TEXT, PRIMARY KEY (warehouse, sku));
INSERT INTO stock VALUES ('A', 1, 10, 'x'), ('A', 2, 20, NULL), ('B', 1, 5, 'new'), ('B', 2, 7, 'same');
CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT);
INSERT INTO notes VALUES (1, 'one'), (2, 'two');
CREATE TABLE log (line TEXT);
INSERT INTO log VALUES ('a');`,
  );
  ENGINES.sqlite.seed(
    dir,
    DST,
    `CREATE TABLE stock (warehouse TEXT NOT NULL, sku INTEGER NOT NULL, qty INTEGER, note TEXT, PRIMARY KEY (warehouse, sku));
INSERT INTO stock VALUES ('A', 1, 10, 'x'), ('A', 2, 25, 'old'), ('B', 2, 7, 'same'), ('C', 9, 1, 'gone');
CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT);
INSERT INTO notes VALUES (1, 'one'), (2, 'two');
CREATE TABLE log (line TEXT);`,
  );
  saveEngines(dir, ["sqlite"]);
}

/** Sets a React-controlled field (`select` or `input`) to `value`. */
const setField = (page, expr, value, proto = "HTMLSelectElement") =>
  page.eval(`(() => {
    const el = ${expr};
    const set = Object.getOwnPropertyDescriptor(${proto}.prototype, 'value').set;
    set.call(el, ${JSON.stringify(value)});
    el.dispatchEvent(new Event(${JSON.stringify(proto === "HTMLSelectElement" ? "change" : "input")}, { bubbles: true }));
  })()`);

const tableSelect = `${dlg}.querySelectorAll('select')[2]`;
const filterInput = `${dlg}.querySelector('input:not([type=checkbox])')`;
const checkbox = `${dlg}.querySelector('input[type=checkbox]')`;

/** Opens the dialog on the table `name` of the source and waits until its target table is chosen. */
async function open(page, name, table = name) {
  await rowMenu(page, [SOURCE, "main", "Tables"], name, "Compare data with");
  await page.waitFor(`${dlg} !== null`);
  // The target is the other connection, its schema main, and the table of the same name.
  await page.waitFor(`${dlg}.querySelectorAll('select')[0].selectedOptions[0]?.textContent === ${JSON.stringify(TARGET)}`, 15_000);
  await page.waitFor(`[...${tableSelect}.options].some(o => o.textContent === ${JSON.stringify(table)})`, 15_000);
  // The table of the same name is chosen by itself; any other one is picked.
  if (table !== name) await setField(page, tableSelect, table);
  await page.waitFor(`${tableSelect}.value === ${JSON.stringify(table)}`);
}

/** Presses Compare and waits for the counts or an error. */
async function compare(page) {
  await page.eval(`${buttonIn(dlg, "Compare")}.click()`);
  await page.waitFor(done, 20_000);
}

const close = async (page) => {
  await page.eval(`${buttonIn(dlg, "Close")}.click()`);
  await page.waitFor(`${dlg} === null`);
};

/** Presses Open script and resolves to the text of the editor of the new tab. */
async function openScript(page) {
  const tabs = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await page.eval(`${buttonIn(dlg, "Open script")}.click()`);
  await page.waitFor(`${dlg} === null`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length > ${tabs} && (${queryText}).length > 0`, 15_000);
  return page.eval(queryText);
}

export default async function (page) {
  await openPath(page, [SOURCE, "main", "Tables", "stock"]);

  // Without the deletes: one row to insert, one to update (two cells), one only in the target kept.
  await open(page, "stock");
  assert.equal(await page.eval(`${checkbox}.checked`), false, "deleting is off by default");
  assert.equal(await page.eval(`${buttonIn(dlg, "Open script")} === undefined`), true, "no script before comparing");
  await compare(page);
  assert.equal(await page.eval(errorText), "");
  let c = await page.eval(counts);
  assert.match(c, /Same: 2/);
  assert.match(c, /Only in source → inserted: 1/);
  assert.match(c, /Changed → updated: 1/);
  assert.match(c, /Only in target → kept: 1/);
  let s = await page.eval(samples);
  assert.match(s, /Only in source\s*B, 1/);
  assert.match(s, /Changed\s*A, 2/);
  assert.match(s, /qty: 25 → 20/);
  assert.match(s, /note: old → NULL/, "NULL is shown as NULL");
  assert.match(s, /Only in target\s*C, 9/);
  assert.doesNotMatch(await page.eval(`${dlg}.textContent`), /First \d+ of/, "nothing is cut");
  // Changing an option drops the result: the script must match what is asked.
  await page.eval(`${checkbox}.click()`);
  await page.waitFor(`${dlg}.querySelector('.data-diff-counts') === null`);
  assert.equal(await page.eval(`${buttonIn(dlg, "Open script")} === undefined`), true);
  await page.eval(`${checkbox}.click()`);
  await compare(page);
  let sql = await openScript(page);
  assert.match(sql, /-- Makes QA3 SQLite dst · main\.stock like QA3 SQLite src · main\.stock/);
  assert.match(sql, /INSERT INTO [^\n]*stock[^\n]*/);
  assert.match(sql, /UPDATE [^\n]*stock[^\n]*/);
  assert.doesNotMatch(sql, /DELETE/, "the rows only in the target are kept");

  // With the deletes.
  await open(page, "stock");
  await page.eval(`${checkbox}.click()`);
  await page.waitFor(`${checkbox}.checked === true`);
  await compare(page);
  c = await page.eval(counts);
  assert.match(c, /Only in target → deleted: 1/);
  sql = await openScript(page);
  assert.match(sql, /INSERT INTO [^\n]*stock/);
  assert.match(sql, /UPDATE [^\n]*stock/);
  assert.match(sql, /DELETE FROM [^\n]*stock[^\n]*/);
  assert.match(sql, /'C'/, "the DELETE names the key of the missing row");

  // A filter narrows both tables: only warehouse A, so B and C are out of the comparison.
  await open(page, "stock");
  await setField(page, filterInput, "warehouse = 'A'", "HTMLInputElement");
  await page.waitFor(`${filterInput}.value === "warehouse = 'A'"`);
  await compare(page);
  c = await page.eval(counts);
  assert.match(c, /Same: 1/);
  assert.match(c, /Only in source → inserted: 0/);
  assert.match(c, /Changed → updated: 1/);
  assert.match(c, /Only in target → kept: 0/);
  s = await page.eval(samples);
  assert.match(s, /Changed\s*A, 2/);
  assert.doesNotMatch(s, /B, 1|C, 9/);
  // A filter that matches nothing on either side: no rows, nothing to change.
  await setField(page, filterInput, "warehouse = 'Z'", "HTMLInputElement");
  await page.waitFor(`${dlg}.querySelector('.data-diff-counts') === null`);
  await compare(page);
  assert.match(await page.eval(counts), /Same: 0/);
  assert.match(await page.eval(notice), /The tables have the same rows/);
  assert.equal(await page.eval(`${buttonIn(dlg, "Open script")} === undefined`), true);
  // A bad filter is an error in the dialog, not a result.
  await setField(page, filterInput, "warehouse = ", "HTMLInputElement");
  await page.waitFor(`${filterInput}.value === "warehouse = "`);
  await compare(page);
  assert.notEqual(await page.eval(errorText), "", "a bad filter is reported");
  assert.equal(await page.eval(`${dlg}.querySelector('.data-diff-counts')`), null);
  assert.equal(await page.eval(`${buttonIn(dlg, "Compare")}.disabled`), false, "the dialog can be used again");

  // Identical tables: the notice and no script.
  await close(page);
  await open(page, "notes");
  await compare(page);
  assert.match(await page.eval(counts), /Same: 2/);
  assert.match(await page.eval(counts), /Only in source → inserted: 0/);
  assert.match(await page.eval(notice), /The tables have the same rows/);
  assert.equal(await page.eval(`${buttonIn(dlg, "Open script")} === undefined`), true);
  await close(page);

  // A target table without the key columns of the source: an error that names the column.
  await open(page, "stock", "log");
  await compare(page);
  assert.match(await page.eval(errorText), /no column warehouse|warehouse/);
  assert.equal(await page.eval(`${buttonIn(dlg, "Open script")} === undefined`), true);
  await close(page);

  // A source table without a key.
  await openPath(page, [SOURCE, "main", "Tables", "log"]);
  await open(page, "log");
  await compare(page);
  assert.match(await page.eval(errorText), /no primary key/i);
  await close(page);
}
