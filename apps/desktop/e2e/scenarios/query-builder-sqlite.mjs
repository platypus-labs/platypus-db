// The visual query builder on SQLite: a table opened from the navigator, a second one added by
// double click and joined on their foreign key (the JOIN button flips to LEFT JOIN and back),
// columns ticked into the SELECT, a WHERE, a second box of the same table getting its own alias,
// the tables list filtered, a box removed; Run opens an editor that executes the SQL and shows
// the rows (an inner and a left join differ in the customer without orders), Open in editor
// opens it without running, a WHERE that does not hold ends in the error of the database, and
// with no table the SQL says so and both buttons are off.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { rows, stubClipboard } from "../qa-grid.mjs";
import { expand, fill, menu } from "../qa-xfer.mjs";
import { editorText, sqliteConnection, status, visibleTab } from "../query.mjs";

const NAME = "Builder db";
const builder = `document.querySelector('.builder-tab:not([style*="none"])')`;
const builderSql = `(${builder}?.querySelector('.builder-sql')?.textContent ?? '')`;
const sources = `[...${builder}.querySelectorAll('.builder-source .tree-label')].map(l => l.textContent)`;
const boxes = `[...${builder}.querySelectorAll('.builder-box')].map(b => b.dataset.table)`;
const source = (name) => `[...${builder}.querySelectorAll('.builder-source')].find(r => r.textContent === ${JSON.stringify(name)})`;
const action = (label) => `[...${builder}.querySelectorAll('.builder-bottom .actions button')].find(b => b.textContent === ${JSON.stringify(label)})`;
const column = (table, name) =>
  `[...${builder}.querySelectorAll('.builder-box[data-table="${table}"] label')].find(l => l.textContent === ${JSON.stringify(name)}).querySelector('input')`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "builder.db",
    id: "00000000-0000-4000-8000-000000000b81",
    name: NAME,
    sql: `CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT);
      CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers (id), total REAL);
      CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT);
      INSERT INTO customers VALUES (1, 'ann'), (2, 'bob'), (3, 'cy');
      INSERT INTO orders VALUES (1, 1, 50), (2, 1, 150), (3, 2, 250);`,
  });
}

/** Runs the builder's `label` button ("Run" or "Open in editor") and waits for the new editor. */
async function press(page, label) {
  const tabs = await page.eval(`document.querySelectorAll('.query-tab').length`);
  await page.eval(`${action(label)}.click()`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length === ${tabs + 1} && ${visibleTab} !== undefined`, 15_000);
}

/** Goes back to the builder's tab. */
async function backToBuilder(page) {
  await page.eval(`[...document.querySelectorAll('.tab')].find(t => /builder/i.test(t.textContent))?.click()`);
  await page.waitFor(`${builder} !== null`);
}

export default async function (page) {
  await stubClipboard(page);
  await expand(page, NAME, "customers");
  await menu(page, "customers", "Visual query builder", null, NAME);
  await page.waitFor(`${builderSql}.includes('FROM customers c')`, 15_000);
  // The list has the schema's tables; the filter narrows it.
  await page.waitFor(`${sources}.length === 3`, 10_000);
  await page.eval(fill(`${builder}.querySelector('input[aria-label="Filter tables…"]')`, "ord"));
  await page.waitFor(`${sources}.length === 1`);
  assert.deepEqual(await page.eval(sources), ["orders"]);

  // Double click adds orders, joined to customers on the foreign key.
  await page.eval(`${source("orders")}.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  await page.waitFor(`${builderSql}.includes('JOIN orders o ON o.customer_id = c.id')`, 10_000);
  assert.deepEqual(await page.eval(boxes), ["customers", "orders"]);
  await page.waitFor(`${column("orders", "total")} !== undefined`, 10_000);

  // JOIN to LEFT JOIN and back.
  const join = `${builder}.querySelector('.builder-join')`;
  await page.eval(`${join}.click()`);
  await page.waitFor(`${builderSql}.includes('LEFT JOIN orders o')`);
  assert.equal(await page.eval(`${join}.textContent`), "LEFT JOIN");
  await page.eval(`${join}.click()`);
  await page.waitFor(`!${builderSql}.includes('LEFT JOIN')`);

  // Columns ticked make the SELECT list; the WHERE closes the statement.
  await page.eval(`${column("customers", "name")}.click()`);
  await page.eval(`${column("orders", "total")}.click()`);
  await page.waitFor(`${builderSql}.startsWith('SELECT c.name,\\n       o.total\\nFROM customers c')`);
  await page.eval(fill(`${builder}.querySelector('.builder-where input')`, "o.total > 100"));
  await page.waitFor(`${builderSql}.endsWith('WHERE o.total > 100;')`);

  // Run: an editor with that SQL, executed; only the orders above 100.
  await press(page, "Run");
  await page.waitFor(`${status} === '2 rows'`, 15_000);
  assert.deepEqual(await rows(page), [["ann", "150.0"], ["bob", "250.0"]]);
  assert.match(await page.eval(editorText), /WHERE o\.total > 100;$/);

  // Without the WHERE and with a LEFT JOIN, the customer without orders is there too.
  await backToBuilder(page);
  await page.eval(fill(`${builder}.querySelector('.builder-where input')`, ""));
  await page.eval(`${join}.click()`);
  await page.waitFor(`${builderSql}.includes('LEFT JOIN orders o') && !${builderSql}.includes('WHERE')`);
  await press(page, "Run");
  await page.waitFor(`${status} === '4 rows'`, 15_000);
  const names = (await rows(page)).map((r) => r[0]);
  assert.deepEqual(names, ["ann", "ann", "bob", "cy"]);

  // Open in editor: the same text, not run.
  await backToBuilder(page);
  await press(page, "Open in editor");
  await page.waitFor(`${editorText}.includes('LEFT JOIN orders o')`);
  await page.sleep(500);
  assert.equal(await page.eval(`(${status}).includes('rows')`), false, "not executed");

  // A WHERE the database refuses: the error, not a result.
  await backToBuilder(page);
  await page.eval(fill(`${builder}.querySelector('.builder-where input')`, "o.nope > 1"));
  await page.waitFor(`${builderSql}.includes('o.nope')`);
  await press(page, "Run");
  await page.waitFor(`/no such column/i.test(${visibleTab}.textContent)`, 15_000);
  assert.match(await page.eval(`${visibleTab}.textContent`), /no such column/i);

  // The same table twice gets an alias of its own; a box is removed with its button.
  await backToBuilder(page);
  await page.eval(fill(`${builder}.querySelector('.builder-where input')`, ""));
  await page.eval(fill(`${builder}.querySelector('input[aria-label="Filter tables…"]')`, ""));
  await page.waitFor(`${sources}.length === 3`);
  await page.eval(`${source("customers")}.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  await page.waitFor(`${boxes}.length === 3`);
  await page.waitFor(`${builderSql}.includes('c2')`, 10_000);
  await page.eval(`${builder}.querySelectorAll('.builder-box')[2].querySelector('button[aria-label="Remove"]').click()`);
  await page.waitFor(`${boxes}.length === 2`);
  await page.waitFor(`!${builderSql}.includes('c2')`);

  // No table at all: the SQL says so and the buttons are off.
  for (const _ of [0, 1]) await page.eval(`${builder}.querySelector('.builder-box button[aria-label="Remove"]').click()`);
  await page.waitFor(`${boxes}.length === 0`);
  assert.match(await page.eval(builderSql), /No tables yet/);
  assert.equal(await page.eval(`${action("Run")}.disabled && ${action("Open in editor")}.disabled`), true);
  // A table that is not joined to anything is a CROSS JOIN with the first.
  await page.eval(`${source("notes")}.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  await page.eval(`${source("customers")}.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`);
  await page.waitFor(`${builderSql}.includes('CROSS JOIN')`, 10_000);
}
