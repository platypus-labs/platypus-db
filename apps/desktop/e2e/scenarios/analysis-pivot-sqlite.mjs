// Pivot of a result: the form opened from the bar above the grid writes a DuckDB query in the
// SQL field and runs it on the copy of the rows. Sales by region and month with a sum, with a
// count of rows, with totals and as a plain grouping sorted by value; a NULL in the column
// field is a column of its own; the form refuses a sum of text and keeps its choices; a column
// field with more than 200 values is an error, 200 are fine.
import assert from "node:assert/strict";
import { fieldError, fieldText, gridTsv, liteConnection, mark, newEditor, runQuery, status, visibleTab } from "../qa-ana.mjs";

const NAME = "E2E pivot";

const dialog = `document.querySelector('.pivot-dialog')`;
const button = (label) => `[...${dialog}.querySelectorAll('.actions button')].find(b => b.textContent === ${JSON.stringify(label)})`;
const rowBox = (column) =>
  `[...${dialog}.querySelectorAll('fieldset label')].find(l => l.textContent === ${JSON.stringify(column)})?.querySelector('input')`;
const select = (label) => `${dialog}.querySelector('select[aria-label=${JSON.stringify(label)}]')`;
const totalsBox = `[...${dialog}.querySelectorAll('label.check')].find(l => l.textContent === 'Totals').querySelector('input')`;
const options = (label) => `[...${select(label)}.options].filter(o => !o.disabled).map(o => o.value)`;
const rowNames = `[...${dialog}.querySelectorAll('fieldset label')].map(l => l.textContent)`;
const rowsChecked = `[...${dialog}.querySelectorAll('fieldset label')].filter(l => l.querySelector('input').checked).map(l => l.textContent)`;

export function setup({ dir }) {
  liteConnection(
    dir,
    NAME,
    "00000000-0000-4000-8000-0000000b0001",
    `CREATE TABLE sales (id INTEGER PRIMARY KEY, region TEXT, month TEXT, amount REAL);
     INSERT INTO sales VALUES (1, 'north', '2024-01', 10.5), (2, 'south', '2024-01', 5.5),
       (3, 'north', '2024-02', 7.5), (4, 'east', NULL, 3.25), (5, 'north', '2024-01', 4.25),
       (6, 'south', '2024-02', 8.75);`,
  );
}

/** Sets a select of the form as a user would. */
const choose = (label, value) => `(() => {
  const el = ${select(label)};
  Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event('change', { bubbles: true }));
})()`;

async function openForm(page) {
  await page.eval(`${visibleTab}.querySelector('.result-pivot-open').click()`);
  await page.waitFor(`${dialog} != null`);
}

/** Back to the result, so that the mark of the next pivot's output is a new one. */
async function backToResult(page, rows) {
  await page.eval(`${visibleTab}.querySelector('.result-sql-clear').click()`);
  await page.waitFor(`${mark} === '' && ${status} === ${JSON.stringify(rows)}`, 15_000);
}

/** Presses Pivot in the open form and waits for the grid to show the pivot, or the field for
 *  its error. */
async function submit(page, { error = false } = {}) {
  await page.eval(`${button("Pivot")}.click()`);
  await page.waitFor(`${dialog} == null`);
  if (error) await page.waitFor(`${fieldError} !== ''`, 30_000);
  else await page.waitFor(`${mark}.startsWith('SQL view')`, 30_000);
}

export default async function (page) {
  await newEditor(page, NAME);
  await runQuery(page, "SELECT region, month, amount FROM sales ORDER BY id");
  assert.equal(await page.eval(status), "6 rows");

  // The form: nothing runs without a row field; a field is down or across, not both.
  await openForm(page);
  assert.equal(await page.eval(`${button("Pivot")}.disabled`), true);
  assert.deepEqual(await page.eval(rowNames), ["region", "month", "amount"]);
  await page.eval(`${rowBox("region")}.click()`);
  await page.waitFor(`${button("Pivot")}.disabled === false`);
  await page.eval(`${rowBox("month")}.click()`);
  await page.eval(choose("Column field", "month"));
  await page.waitFor(`${rowNames}.length === 2`);
  assert.deepEqual(await page.eval(rowsChecked), ["region"], "the column field leaves the row fields");

  // Only a number column takes a sum or an average; no value counts rows.
  assert.deepEqual(await page.eval(options("Aggregate")), ["count"]);
  await page.eval(choose("Value", "region"));
  assert.deepEqual(await page.eval(options("Aggregate")), ["count", "count_distinct", "min", "max"], "no sum of text");
  await page.eval(choose("Value", "amount"));
  assert.deepEqual(await page.eval(options("Aggregate")), ["count", "count_distinct", "sum", "avg", "min", "max"]);
  await page.eval(choose("Aggregate", "sum"));
  await page.eval(choose("Value", "region"));
  assert.equal(await page.eval(`${select("Aggregate")}.value`), "count", "a sum of text falls back to count");
  await page.eval(choose("Value", "amount"));
  await page.eval(choose("Aggregate", "sum"));
  // A sort by value needs a column to sort by: with a column field, only totals give one.
  assert.deepEqual(await page.eval(options("Sort")), ["keys", "keys_desc"]);

  // Sum by region and month: the NULL month is a column of its own and its cells are empty
  // elsewhere; the query is in the field.
  await submit(page);
  assert.equal(await page.eval(status), "3 rows");
  assert.equal(await gridTsv(page), "region\t2024-01\t2024-02\tNULL\neast\t\t\t3.25\nnorth\t14.75\t7.5\t\nsouth\t5.5\t8.75\t");
  assert.ok((await page.eval(fieldText)).startsWith("PIVOT (SELECT"), "the field holds the query");
  assert.equal(await page.eval(fieldError), "");

  // The form keeps its choices for the statement; totals add a column and a row, each summed
  // over the rows (last whatever the sort).
  await backToResult(page, "6 rows");
  await openForm(page);
  assert.deepEqual(await page.eval(rowsChecked), ["region"]);
  assert.equal(await page.eval(`${select("Column field")}.value`), "month");
  assert.equal(await page.eval(`${select("Value")}.value`), "amount");
  assert.equal(await page.eval(`${select("Aggregate")}.value`), "sum");
  await page.eval(`${totalsBox}.click()`);
  assert.deepEqual(await page.eval(options("Sort")), ["keys", "keys_desc", "value_desc", "value_asc"]);
  await page.eval(choose("Sort", "value_desc"));
  await submit(page);
  assert.equal(await page.eval(status), "4 rows");
  assert.equal(
    await gridTsv(page),
    "region\t2024-01\t2024-02\tNULL\tTotal\nnorth\t14.75\t7.5\t\t22.25\nsouth\t5.5\t8.75\t\t14.25\neast\t\t\t3.25\t3.25\nTotal\t20.25\t16.25\t3.25\t39.75",
  );
  assert.ok((await page.eval(fieldText)).startsWith("WITH src AS"));

  // Without a value the cells count rows; a column with no row in a cell counts 0.
  await backToResult(page, "6 rows");
  await openForm(page);
  await page.eval(choose("Value", ""));
  assert.equal(await page.eval(`${select("Aggregate")}.value`), "count");
  await page.eval(choose("Sort", "keys"));
  await submit(page);
  assert.equal(
    await gridTsv(page),
    "region\t2024-01\t2024-02\tNULL\tTotal\neast\t0\t0\t1\t1\nnorth\t2\t1\t0\t3\nsouth\t1\t1\t0\t2\nTotal\t3\t2\t1\t6",
  );

  // No column field: a plain grouping, with a row of totals, sorted by the aggregate.
  await backToResult(page, "6 rows");
  await openForm(page);
  await page.eval(choose("Column field", ""));
  await page.eval(choose("Value", "amount"));
  await page.eval(choose("Aggregate", "max"));
  await page.eval(choose("Sort", "value_asc"));
  await submit(page);
  assert.equal(await page.eval(status), "4 rows");
  assert.equal(await gridTsv(page), "region\tmax_amount\neast\t3.25\nsouth\t8.75\nnorth\t10.5\nTotal\t10.5");

  // Edited in the field, the query runs as any other; Escape returns to the result.
  await page.eval(`${visibleTab}.querySelector('.result-sql .cm-content').focus()`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`${status} === '6 rows' && ${mark} === ''`, 15_000);

  // A column field may have 200 values, not more: above, the field names the problem and the
  // result stays.
  await numbers(page, 200);
  await openForm(page);
  await page.eval(`${rowBox("grp")}.click()`);
  await page.eval(choose("Column field", "i"));
  await submit(page);
  assert.equal(await page.eval(status), "3 rows");
  const header = (await gridTsv(page)).split("\n")[0].split("\t");
  assert.equal(header.length, 201, "grp and 200 columns");

  await numbers(page, 250);
  await openForm(page);
  await page.eval(`${rowBox("grp")}.click()`);
  await page.eval(choose("Column field", "i"));
  await submit(page, { error: true });
  const error = await page.eval(fieldError);
  assert.ok(error.startsWith("The column i has 250 distinct values"), error);
  assert.ok(error.includes("200"), error);
  assert.equal(await page.eval(status), "250 rows", "the result is still shown");
  assert.equal(await page.eval(mark), "");
}

/** Runs a statement of `n` rows with numbers `i` and three groups `grp`. */
async function numbers(page, n) {
  await runQuery(
    page,
    `WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ${n}) SELECT i, i % 3 AS grp FROM n`,
    60_000,
  );
  assert.equal(await page.eval(status), `${n} rows`);
}
