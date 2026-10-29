// Computed columns of the grid on a throwaway SQLite database: added from the Columns menu,
// worked out from the row (numbers, text, NULL, functions), copied with the rest of the
// selection, left out of what reads the result on the backend (copy as, editing), kept for the
// table, edited, hidden and removed; an expression that cannot be used shows why and cannot be
// added. The grid is a canvas: values are read from what Ctrl+C copies.
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { assert, CTRL, SHIFT, copied, focusCell, grid, gridMenu, rows, shotter, status, stubClipboard, treeRow, visibleTab } from "../qa-grid.mjs";

const NAME = "E2E grid computed";
const dialog = `document.querySelector('.computed-dialog')`;
const field = (name) => `${dialog}.querySelector('[data-field=${name}]')`;
const saveButton = `${dialog}.querySelector('[data-action=save-computed]')`;
const listed = `[...${dialog}.querySelectorAll('.formatting-rule .formatting-text')].map(b => b.textContent)`;
const problem = `${field("problem")}.textContent`;

export function setup({ dir }) {
  const path = join(dir, "computed.db");
  const db = new DatabaseSync(path);
  db.exec(`CREATE TABLE orders (id INTEGER PRIMARY KEY, customer TEXT, price REAL, qty INTEGER, note TEXT);
           INSERT INTO orders VALUES
             (1, 'Ada', 12.5, 3, 'gift'),
             (2, 'Bob', 0.1, 2, NULL),
             (3, 'Cy', 7, NULL, 'rush'),
             (4, 'Dee', 100, 0, NULL);`);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-0000000c0001"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
}

async function openTable(page) {
  await page.waitFor(`${treeRow("orders")} !== undefined`);
  await page.eval(`${treeRow("orders")}.id = 'e2e-table'`);
  await page.clickSel("#e2e-table", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-table').removeAttribute('id')`);
  await page.waitFor(`${status} === '4 rows'`, 15_000);
}

async function openDialog(page) {
  await gridMenu(page, "Columns", "Computed columns…");
  await page.waitFor(`${dialog} !== null`);
}

/** Types into a field of the dialog as typing would. */
async function set(page, name, value) {
  await page.eval(`(() => {
    const el = ${field(name)};
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
    el.dispatchEvent(new Event('input', { bubbles: true }));
  })()`);
  await page.waitFor(`${field(name)}.value === ${JSON.stringify(value)}`);
}

/** Sets the name and expression and waits for the dialog's verdict on them. */
async function draft(page, name, expression) {
  await set(page, "name", name);
  await set(page, "expression", expression);
}

async function add(page, count) {
  await page.waitFor(`!${saveButton}.disabled`);
  await page.eval(`${saveButton}.click()`);
  await page.waitFor(`${dialog}.querySelectorAll('.formatting-rule').length === ${count} && ${field("name")}.value === ''`);
}

async function close(page) {
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent.trim() === 'Close').click()`);
  await page.waitFor(`${dialog} === null`);
}

/** The refused expression `expression` and the start of the reason shown. */
async function refused(page, name, expression, reason) {
  await draft(page, name, expression);
  await page.waitFor(`${problem}.startsWith(${JSON.stringify(reason)})`);
  assert.ok(await page.eval(`${saveButton}.disabled`), `${expression}: cannot be added`);
  assert.ok(await page.eval(`${field("problem")}.classList.contains('error-text')`), `${expression}: shown as an error`);
}

export default async function (page, { shots }) {
  const shot = shotter(page, shots, "grid-computed");
  await stubClipboard(page);
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await openTable(page);
  // The table's own values, as copied before any computed column.
  const before = await rows(page);
  const plain = before;
  assert.deepEqual(
    before.map((r) => [r[1], r[3], r[4]]),
    [["Ada", "3", "gift"], ["Bob", "2", ""], ["Cy", "", "rush"], ["Dee", "0", ""]],
  );

  // An empty dialog; nothing to add until both name and expression are there.
  await openDialog(page);
  assert.equal(await page.eval(`${dialog}.querySelector('.formatting-empty') !== null`), true);
  assert.ok(await page.eval(`${saveButton}.disabled`), "no name, no column");
  await set(page, "expression", "price * qty");
  assert.ok(await page.eval(`${saveButton}.disabled`), "no name yet");

  // Expressions that cannot be used say why.
  await refused(page, "x", "missing + 1", "No column named missing");
  await refused(page, "x", "nope(qty)", "No function named nope");
  await refused(page, "x", "round()", "Wrong number of values for round");
  await refused(page, "x", "price *", "The expression is incomplete");
  await refused(page, "x", "price ? 2", "The expression is not valid near");
  await refused(page, "price", "qty", "A column named price already exists");

  // Arithmetic, text and a condition; NULL in an operation gives NULL.
  await draft(page, "total", "price * qty");
  await page.waitFor(`!${field("problem")}.classList.contains('error-text')`);
  await add(page, 1);
  await draft(page, "label", `upper(customer) || ' x' || coalesce(qty, 0)`);
  await add(page, 2);
  await draft(page, "size", "if(qty >= 3, 'bulk', if(qty IS NULL, 'unknown', 'single'))");
  await add(page, 3);
  await draft(page, "per unit", "round(price / qty, 2)");
  await add(page, 4);
  assert.deepEqual(await page.eval(listed), [
    "total = price * qty",
    "label = upper(customer) || ' x' || coalesce(qty, 0)",
    "size = if(qty >= 3, 'bulk', if(qty IS NULL, 'unknown', 'single'))",
    "per unit = round(price / qty, 2)",
  ]);
  await shot("dialog");
  await close(page);

  // The computed columns follow the result's, worked out on each row; a division by zero
  // and a NULL quantity give NULL (copied as nothing).
  await page.waitFor(`${grid} !== null`);
  assert.deepEqual(await rows(page), [
    [...plain[0], "37.5", "ADA x3", "bulk", "4.17"],
    [...plain[1], "0.2", "BOB x2", "single", "0.05"],
    [...plain[2], "", "CY x0", "unknown", ""],
    [...plain[3], "0", "DEE x0", "single", ""],
  ]);
  await shot("grid");

  // Copy as (built from the result on the backend) leaves them out: the header has only the
  // table's columns.
  const csv = await copied(page, async () => {
    await page.eval(`${grid}.focus()`);
    await page.key("a", "KeyA", 65, CTRL);
    await page.key("C", "KeyC", 67, CTRL | SHIFT);
  });
  assert.equal(csv.split("\n")[0], "id\tcustomer\tprice\tqty\tnote");

  // A computed cell cannot be edited: the editor says so.
  await focusCell(page, 0, 5);
  await page.key("F2", "F2", 113);
  await page.waitFor(`${visibleTab}.textContent.includes('total is not a column of orders')`);
  assert.equal(await page.eval(`document.querySelector('.cell-editor, .cell-popup-text')`), null);
  await page.key("Escape", "Escape", 27);

  // A selection of only computed columns copies nothing from the backend, and plain copy of a
  // mixed one has both: the customer and its label.
  await focusCell(page, 1, 1);
  for (let i = 0; i < 5; i++) await page.key("ArrowRight", "ArrowRight", 39, SHIFT);
  const mixed = await copied(page, () => page.key("c", "KeyC", 67, CTRL));
  assert.equal(mixed, "Bob\t0.1\t2\t\t0.2\tBOB x2");

  // Kept for the table: closed and opened again, the same columns and values.
  await page.key("w", "KeyW", 87, CTRL);
  await openTable(page);
  assert.deepEqual((await rows(page))[0], [...plain[0], "37.5", "ADA x3", "bulk", "4.17"]);

  // Editing one changes its values in place; removing another drops it.
  await openDialog(page);
  await page.eval(`[...${dialog}.querySelectorAll('.formatting-text')].find(b => b.textContent.startsWith('total =')).click()`);
  await page.waitFor(`${field("expression")}.value === 'price * qty' && [...${dialog}.querySelectorAll('button')].some(b => b.textContent.trim() === 'Update column')`);
  // Its own name is not taken by itself.
  assert.ok(!(await page.eval(`${saveButton}.disabled`)), "the name of the column being edited is free");
  await set(page, "expression", "price * qty * 2");
  await page.eval(`${saveButton}.click()`);
  await page.waitFor(`${listed}[0] === 'total = price * qty * 2'`);
  await page.eval(`[...${dialog}.querySelectorAll('.formatting-rule')].find(r => r.dataset.computed === 'size').querySelector('button[title="Remove"]').click()`);
  await page.waitFor(`${dialog}.querySelectorAll('.formatting-rule').length === 3`);
  await close(page);
  assert.deepEqual((await rows(page))[0], [...plain[0], "75", "ADA x3", "4.17"]);

  // The Columns dialog lists them with the others: hidden, a computed column is not copied.
  await gridMenu(page, "Columns", "Columns…");
  await page.waitFor(`document.querySelector('.columns-list') !== null`);
  await page.eval(`[...document.querySelectorAll('.columns-row label')].find(l => l.textContent === 'label').querySelector('input').click()`);
  await page.waitFor(`[...document.querySelectorAll('.columns-row.muted label')].some(l => l.textContent === 'label')`);
  await page.eval(`[...document.querySelectorAll('.modal button')].find(b => b.textContent.trim() === 'Close').click()`);
  await page.waitFor(`document.querySelector('.columns-list') === null`);
  assert.deepEqual((await rows(page))[0], [...plain[0], "75", "4.17"]);
  await gridMenu(page, "Columns", "Reset columns");
  assert.deepEqual((await rows(page))[0], [...plain[0], "75", "ADA x3", "4.17"]);

  // Removing them all leaves the table as it was, also when opened again.
  await openDialog(page);
  for (let left = 2; left >= 0; left--) {
    await page.eval(`${dialog}.querySelector('.formatting-rule button[title="Remove"]').click()`);
    await page.waitFor(`${dialog}.querySelectorAll('.formatting-rule').length === ${left}`);
  }
  await close(page);
  assert.deepEqual(await rows(page), before);
  await page.key("w", "KeyW", 87, CTRL);
  await openTable(page);
  assert.deepEqual(await rows(page), before);
}
