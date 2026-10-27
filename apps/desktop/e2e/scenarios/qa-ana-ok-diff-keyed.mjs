// Regression guard (works): "Compare with previous run" on a table with a composite primary key
// matches rows by the key: a changed value, a changed key (removed + added), NULL → '' and
// '' → NULL are each told apart with the old value in the cell's title; same rows in another
// order are no difference; without a key, duplicate rows pair one by one.
import assert from "node:assert/strict";
import { compareButton, diffTab, liteConnection, newEditor, rerun, runQuery, sqliteExec, status } from "../qa-ana.mjs";

const NAME = "QA ana diff keyed";
let file;

export function setup({ dir }) {
  file = liteConnection(
    dir,
    NAME,
    "00000000-0000-4000-8000-0000000a0120",
    `CREATE TABLE keyed (k1 INTEGER, k2 TEXT, v TEXT, PRIMARY KEY (k1, k2));
     INSERT INTO keyed VALUES (1, 'a', 'one'), (1, 'b', 'two'), (2, 'a', 'three'), (3, 'z', NULL), (4, 'e', '');
     CREATE TABLE nokey (a INTEGER, b TEXT);
     INSERT INTO nokey VALUES (1, 'x'), (1, 'x'), (2, 'y');`,
  );
}

const rows = `[...${diffTab}.querySelectorAll('tbody tr')].map(tr => [...tr.querySelectorAll('td')].map(td => td.textContent + (td.title ? '{' + td.title + '}' : '')).join('|'))`;

/** Opens the diff of the shown result and returns its counts and rows, then goes back. */
async function compare(page) {
  await page.waitFor(`${compareButton}?.disabled === false`);
  const tabs = await page.eval(`document.querySelectorAll('.diff-tab').length`);
  await page.eval(`${compareButton}.click()`);
  await page.waitFor(`document.querySelectorAll('.diff-tab').length > ${tabs} && ${diffTab}?.querySelector('.diff-counts') != null`, 10_000);
  const out = {
    counts: await page.eval(`${diffTab}.querySelector('.diff-counts').textContent`),
    keyed: await page.eval(`${diffTab}.querySelector('.diff-summary .muted').textContent`),
    rows: await page.eval(rows),
  };
  await page.eval(`[...document.querySelectorAll('.tab')].find(t => t.textContent.startsWith(${JSON.stringify(NAME)})).click()`);
  await page.waitFor(`${compareButton} != null`);
  return out;
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT * FROM keyed ORDER BY k1, k2");
  assert.equal(await page.eval(status), "5 rows");
  sqliteExec(
    file,
    `UPDATE keyed SET v = 'TWO' WHERE k1 = 1 AND k2 = 'b'; UPDATE keyed SET k2 = 'c' WHERE k1 = 2;
     UPDATE keyed SET v = '' WHERE k1 = 3; UPDATE keyed SET v = NULL WHERE k1 = 4;`,
  );
  await rerun(page);
  const keyed = await compare(page);
  assert.equal(keyed.counts, "+1 −1 ~3");
  assert.equal(keyed.keyed, "Rows matched by the table's key");
  assert.deepEqual(keyed.rows, ["~|1|b|TWO{Before: two}", "+|2|c|three", "~|3|z|{Before: NULL}", "~|4|e|NULL{Before: }", "−|2|a|three"]);

  await runQuery(page, "SELECT * FROM keyed ORDER BY random()");
  await rerun(page);
  assert.equal((await compare(page)).counts, "+0 −0 ~0");

  await runQuery(page, "SELECT * FROM nokey");
  sqliteExec(file, "DELETE FROM nokey WHERE rowid = 1; INSERT INTO nokey VALUES (3, 'w');");
  await rerun(page);
  const plain = await compare(page);
  assert.equal(plain.counts, "+1 −1 ~0");
  assert.deepEqual(plain.rows, ["+|3|w", "−|1|x"]);
}
