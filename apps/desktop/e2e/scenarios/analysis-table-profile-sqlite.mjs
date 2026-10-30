// Profile data: the navigator's menu of a table (or a view) opens a tab with one card per
// column: NULLs and empty strings, distinct values, extremes, the lengths of text, the patterns
// of its values (emails, numbers and dates stored as text) with the values that break them, the
// outliers of numbers; a value or an outlier opens the table's rows filtered to it. The Sample
// select cuts a long table and says so, Refresh reads the table again, an empty table and a
// table that went away are told, and the filter box finds columns.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { editorText, sqliteConnection, status, treeRow } from "../query.mjs";

const NAME = "E2E table profile";
const tab = `[...document.querySelectorAll('.table-profile-tab')].find(t => t.style.display !== 'none')`;
const card = (column) => `${tab}?.querySelector('.tp-card[data-column="${column}"]')`;
const text = (expression) => `(${expression}?.textContent ?? null)`;
const texts = (selector, root = tab) => `[...(${root}?.querySelectorAll(${JSON.stringify(selector)}) ?? [])].map(e => e.textContent)`;
const cardCount = `(${tab}?.querySelectorAll('.tp-card').length ?? 0)`;
const duplicates = `(${tab}?.querySelector('.tp-kpi[data-kpi="Duplicates"]'))`;
const barStatus = `(${tab}?.querySelector('.tp-status')?.textContent ?? '')`;

/** The rows of `people`: valid emails but for one bad, one empty and one NULL; numbers and dates
 *  stored as text with one value each that breaks the shape; a score with one outlier; notes
 *  with NULLs and empty strings. */
function peopleRows() {
  const rows = [];
  for (let i = 1; i <= 24; i++) {
    const email = i <= 20 || i === 24 ? `'user${i}@example.com'` : i === 21 ? "'not-an-email'" : i === 22 ? "''" : "NULL";
    const qty = i === 24 ? "NULL" : i === 5 ? "'n/a'" : `'${i * 10}'`;
    const joined = i === 7 ? "'07/01/2024'" : `'2024-01-${String(i).padStart(2, "0")}'`;
    const score = i === 24 ? 1000 : i;
    const note = i % 3 === 0 ? "NULL" : i % 6 === 1 ? "''" : "'n'";
    rows.push(`(${i}, ${email}, ${qty}, ${joined}, ${score}, ${note})`);
  }
  return rows.join(", ");
}

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "profile.db",
    id: "00000000-0000-4000-8000-000000000035",
    name: NAME,
    sql: `CREATE TABLE people (id INTEGER PRIMARY KEY, email TEXT, qty_text TEXT, joined TEXT, score REAL, note TEXT);
           INSERT INTO people VALUES ${peopleRows()};
           CREATE VIEW adults AS SELECT * FROM people WHERE id > 12;
           CREATE TABLE empty_t (a TEXT, b INTEGER);
           CREATE TABLE twins (a TEXT, b INTEGER);
           INSERT INTO twins VALUES ('x', 1), ('x', 1), ('x', 1), ('y', 2), ('z', NULL), ('z', NULL);
           CREATE TABLE gone (a TEXT);
           INSERT INTO gone VALUES ('x');
           CREATE TABLE big (id INTEGER PRIMARY KEY, label TEXT);
           WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 10050)
             INSERT INTO big SELECT i, 'v' || i FROM n;`,
  });
}

/** Right-clicks the tree row `label` and clicks `item` of its menu. */
async function menu(page, label, item) {
  await page.waitFor(`${treeRow(label)} !== undefined`, 15_000);
  await page.eval(`${treeRow(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(
    `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span').textContent === ${JSON.stringify(item)}).click()`,
  );
}

/** Opens the profile of `object` and waits for its `rows` read (the status of the header). */
async function profile(page, object, rows) {
  await menu(page, object, "Profile data…");
  await page.waitFor(`${tab}?.querySelector('.object-title strong')?.textContent === ${JSON.stringify(`Profile of ${object}`)}`, 15_000);
  await page.waitFor(`${barStatus}.includes(${JSON.stringify(rows)})`, 30_000);
}

/** Goes back to the profile tab of `object` from the data tab a click opened. */
async function backTo(page, object) {
  await page.eval(`[...document.querySelectorAll('.tab')].find(t => t.textContent.includes(${JSON.stringify(`Profile of ${object}`)}))?.click()`);
  await page.waitFor(`${tab}?.querySelector('.object-title strong')?.textContent === ${JSON.stringify(`Profile of ${object}`)}`);
}

/** The data tab a click opened: its statement has `where` and the grid shows `rows`. */
async function dataTab(page, where, rows) {
  await page.waitFor(`${editorText}.includes(${JSON.stringify(where)})`, 15_000);
  await page.waitFor(`${status} === ${JSON.stringify(rows)}`, 15_000);
}

/** Chooses `value` (a sample size, or "null" for the whole table) in the Sample select. */
async function chooseSample(page, value) {
  await page.eval(`(() => {
    const select = ${tab}.querySelector('.tp-sample');
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(select, ${JSON.stringify(value)});
    select.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
}

/** Replaces the text of the filter box. */
async function filterBy(page, value) {
  await page.eval(`${tab}.querySelector('.tp-filter').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  if (value === "") await page.key("Backspace", "Backspace", 8);
  else await page.type(value);
  await page.waitFor(`${tab}.querySelector('.tp-filter').value === ${JSON.stringify(value)}`);
}

export default async function (page, { dir }) {
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  // Views are in their own folder of the tree.
  await page.waitFor(`${treeRow("people")} !== undefined && ${treeRow("Views")} !== undefined`, 15_000);
  await page.eval(`${treeRow("Views")}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow("adults")} !== undefined`, 15_000);

  // The menu of a table opens the profile: a card per column, with the rows read.
  await profile(page, "people", "24 rows read");
  await page.waitFor(`${cardCount} === 6`);
  assert.deepEqual(await page.eval(texts(".tp-card .column-profile-head strong")), ["id", "email", "qty_text", "joined", "score", "note"]);
  assert.equal(await page.eval(`${tab}.querySelector('.tp-cut') ?? null`), null, "the whole table was read");
  assert.equal(await page.eval(`${tab}.querySelector('.tp-empty') ?? null`), null);

  // Emails: NULLs and empty strings are counted apart, one value breaks the pattern.
  assert.equal(await page.eval(text(`${card("email")}.querySelector('.tp-nulls')`)), "1 (4.2%)");
  assert.equal(await page.eval(text(`${card("email")}.querySelector('.tp-empty-strings')`)), "1 (4.2%)");
  assert.equal(await page.eval(text(`${card("email")}.querySelector('.tp-distinct')`)), "23");
  assert.equal(await page.eval(text(`${card("email")}.querySelector('.tp-dominant')`)), "Mostly Email address");
  assert.deepEqual(await page.eval(texts(".tp-pattern", card("email"))), ["Email address21 (95.5%)"]);
  assert.deepEqual(await page.eval(texts(".tp-exception", card("email"))), ["not-an-email"]);

  // Numbers stored as text: the integers dominate, and the lengths describe the text.
  assert.equal(await page.eval(text(`${card("qty_text")}.querySelector('.tp-dominant')`)), "Mostly Integer stored as text");
  assert.deepEqual(await page.eval(texts(".tp-pattern", card("qty_text"))), ["Integer stored as text22 (95.7%)"]);
  assert.deepEqual(await page.eval(texts(".tp-exception", card("qty_text"))), ["n/a"]);
  assert.equal(await page.eval(text(`${card("qty_text")}.querySelector('.tp-lengths')`)), "2 · 2.6522 · 3");
  assert.equal(await page.eval(text(`${card("qty_text")}.querySelector('.tp-nulls')`)), "1 (4.2%)");

  // Dates stored as text: ISO dates dominate, the date in another style is the exception.
  assert.equal(await page.eval(text(`${card("joined")}.querySelector('.tp-dominant')`)), "Mostly ISO date");
  assert.deepEqual(await page.eval(texts(".tp-exception", card("joined"))), ["07/01/2024"]);
  assert.equal(await page.eval(text(`${card("joined")}.querySelector('.tp-min')`)), "07/01/2024");

  // A number column: extremes, a histogram, the outlier by the IQR rule, and no patterns.
  // A REAL column may show its whole values with a decimal.
  assert.match(await page.eval(text(`${card("score")}.querySelector('.tp-min')`)), /^1(\.0)?$/);
  assert.match(await page.eval(text(`${card("score")}.querySelector('.tp-max')`)), /^1000(\.0)?$/);
  assert.equal(await page.eval(`${card("score")}.querySelectorAll('.column-profile-bars rect').length`), 20);
  assert.deepEqual(await page.eval(texts(".tp-outlier", card("score"))), ["1000"]);
  assert.equal(await page.eval(text(`${card("score")}.querySelector('.tp-outlier-summary')`)), "1 value (4.2%) · fences -10.5 to 35.5");
  assert.equal(await page.eval(`${card("score")}.querySelector('.tp-patterns') ?? null`), null);
  // A column without outliers or patterns shows neither.
  assert.equal(await page.eval(`${card("id")}.querySelector('.tp-outliers') ?? null`), null);
  assert.equal(await page.eval(`${card("id")}.querySelector('.tp-patterns') ?? null`), null);

  // Notes: NULLs, empty strings and the most frequent values, the empty one named.
  assert.equal(await page.eval(text(`${card("note")}.querySelector('.tp-nulls')`)), "8 (33.3%)");
  assert.equal(await page.eval(text(`${card("note")}.querySelector('.tp-empty-strings')`)), "4 (16.7%)");
  assert.deepEqual(await page.eval(texts(".tp-top-value", card("note"))), ["n", "(empty)"]);

  // The filter box finds columns by name or type; nothing found says so.
  await filterBy(page, "mail");
  await page.waitFor(`${cardCount} === 1`);
  assert.equal(await page.eval(`${card("email")} != null`), true);
  await filterBy(page, "REAL");
  await page.waitFor(`${cardCount} === 1 && ${card("score")} != null`);
  await filterBy(page, "zzz");
  await page.waitFor(`${cardCount} === 0`);
  assert.equal(await page.eval(text(`${tab}.querySelector('.tp-none')`)), "No column matches.");
  await filterBy(page, "");
  await page.waitFor(`${cardCount} === 6`);

  // A pattern exception opens the rows with that value.
  await page.eval(`${card("email")}.querySelector('.tp-exception').click()`);
  await dataTab(page, "email = 'not-an-email'", "1 row");
  await backTo(page, "people");

  // An outlier opens the rows past the fences.
  await page.eval(`${card("score")}.querySelector('.tp-outlier').click()`);
  await dataTab(page, "score < -10.5 OR score > 35.5", "1 row");
  await backTo(page, "people");

  // The empty string is a value too: its rows are found with an equality.
  await page.eval(`${card("note")}.querySelectorAll('.tp-top-value')[1].click()`);
  await dataTab(page, "note = ''", "4 rows");
  await backTo(page, "people");

  // Refresh reads the table again: a new row with a second bad email.
  const db = new DatabaseSync(join(dir, "profile.db"), { timeout: 5_000 });
  db.exec("INSERT INTO people VALUES (25, 'also bad', '250', '2024-01-25', 25, 'n')");
  db.close();
  await page.eval(`${tab}.querySelector('.tp-refresh').click()`);
  await page.waitFor(`${barStatus}.includes('25 rows read')`, 15_000);
  await page.waitFor(`${cardCount} === 6`);
  assert.deepEqual(await page.eval(texts(".tp-exception", card("email"))), ["not-an-email", "also bad"]);
  assert.deepEqual(await page.eval(texts(".tp-pattern", card("email"))), ["Email address21 (91.3%)"]);

  // A view is profiled like a table: the rows it selects.
  await profile(page, "adults", "13 rows read");
  await page.waitFor(`${cardCount} === 6`);

  // An empty table: told so, and its cards have nothing to show.
  await profile(page, "empty_t", "0 rows read");
  assert.equal(await page.eval(text(`${tab}.querySelector('.tp-empty')`)), "The table has no rows.");
  assert.equal(await page.eval(cardCount), 2);
  assert.equal(await page.eval(text(`${card("a")}.querySelector('.tp-nulls')`)), "0 (0%)");
  assert.equal(await page.eval(`${card("a")}.querySelector('.tp-patterns') ?? null`), null);
  assert.equal(await page.eval(`${card("b")}.querySelector('.tp-outliers') ?? null`), null);

  // Duplicates: rows identical to another one, NULLs counting as equal; none in a table with a key.
  await profile(page, "twins", "6 rows read");
  await page.waitFor(`${text(`${duplicates}?.querySelector('.tp-kpi-value')`)} === '3'`, 15_000);
  await profile(page, "people", "24 rows read");
  await page.waitFor(`${text(`${duplicates}?.querySelector('.tp-kpi-value')`)} === '0'`, 15_000);
  // A table cut by the sample is not counted: a dash, with the reason, and no error.
  await profile(page, "big", "10,050 rows read");
  await chooseSample(page, "10000");
  await page.waitFor(`${barStatus}.includes('first 10,000 rows')`, 30_000);
  await page.waitFor(`${text(`${duplicates}?.querySelector('.tp-kpi-value')`)} === '—'`, 15_000);
  assert.match(await page.eval(`${duplicates}.title`), /whole table/);
  assert.equal(await page.eval(`${tab}.querySelector('.tp-error') ?? null`), null);
  await chooseSample(page, "null");
  await page.waitFor(`${barStatus}.includes('10,050 rows read')`, 30_000);
  await page.waitFor(`${text(`${duplicates}?.querySelector('.tp-kpi-value')`)} === '0'`, 30_000);

  // A long table: all its rows by default; a smaller sample reads the first rows and says so;
  // the whole table comes back with the last choice.
  await profile(page, "big", "10,050 rows read");
  assert.equal(await page.eval(`${tab}.querySelector('.tp-cut') ?? null`), null);
  await chooseSample(page, "10000");
  await page.waitFor(`${barStatus}.includes('first 10,000 rows')`, 30_000);
  assert.equal(
    await page.eval(text(`${tab}.querySelector('.tp-cut')`)),
    "Only the first 10,000 rows were read: the table has more. Choose a larger sample to profile more of it.",
  );
  assert.equal(await page.eval(text(`${card("id")}.querySelector('.tp-max')`)), "10000");
  await chooseSample(page, "null");
  await page.waitFor(`${barStatus}.includes('10,050 rows read')`, 30_000);
  assert.equal(await page.eval(`${tab}.querySelector('.tp-cut') ?? null`), null);
  assert.equal(await page.eval(text(`${card("id")}.querySelector('.tp-max')`)), "10050");

  // A table dropped after the tab opened: Refresh shows the error, not stale cards.
  await profile(page, "gone", "1 row read");
  const dropper = new DatabaseSync(join(dir, "profile.db"), { timeout: 5_000 });
  dropper.exec("DROP TABLE gone");
  dropper.close();
  await page.eval(`${tab}.querySelector('.tp-refresh').click()`);
  await page.waitFor(`${tab}?.querySelector('.tp-error') != null`, 15_000);
  assert.match(await page.eval(text(`${tab}.querySelector('.tp-error')`)), /gone/);
  assert.equal(await page.eval(cardCount), 0);
}
