// SQL completion on a throwaway SQLite database: the columns of the tables in the statement
// under the cursor, aliases, CTEs and subqueries, quoting on insert, no list in strings and
// comments, and the keys (Tab accepts, Enter after a whole word is a newline).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const NAME = "E2E SQLite";
const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const editorText = `[...${visibleTab}.querySelectorAll('.cm-line')].map(l => l.textContent).join('\\n')`;
const options = `[...document.querySelectorAll('.cm-tooltip-autocomplete li')].map(li => li.textContent)`;
const open = `document.querySelector('.cm-tooltip-autocomplete') !== null`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "e2e.db"));
  db.exec(`
    CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT, "Full Name" TEXT);
    CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers (id), total REAL, placed_at TEXT);
    CREATE TABLE "My Table" (id INTEGER, "Some Col" TEXT);
    CREATE TABLE products (sku TEXT, price REAL);
  `);
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000001"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "e2e.db")}'
save_password = false
`,
  );
}

/** Writes `sql`, whose `|` marks the cursor, then types `typed` there key by key. */
async function typeAt(page, sql, typed) {
  const [before, after = ""] = sql.split("|");
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(before + after);
  await page.waitFor(`${editorText} === ${JSON.stringify(before + after)}`);
  for (let i = 0; i < after.length; i++) await page.key("ArrowLeft", "ArrowLeft", 37);
  // Moving the cursor closes a list the text may have opened.
  await page.waitFor(`!(${open})`);
  for (const ch of typed) {
    await page.type(ch);
    await page.sleep(20);
  }
  const expected = before + typed + after;
  await page.waitFor(`${editorText} === ${JSON.stringify(expected)}`);
}

/** The list once it shows `wanted` (an option starting with it). */
async function listWith(page, wanted) {
  await page.waitFor(`${options}.some(o => o.startsWith(${JSON.stringify(wanted)}))`, 10_000);
  // CodeMirror ignores keys for 75 ms after the list opens.
  await page.sleep(200);
  return page.eval(options);
}

/** The list after typing, or [] when none opens within a short wait. */
async function listAfter(page) {
  await page.sleep(600);
  return page.eval(options);
}

export default async function (page) {
  const connectionRow = `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(NAME)})`;
  await page.waitFor(`${connectionRow} !== undefined`);
  await page.eval(`${connectionRow}.id = 'e2e-connection'`);
  await page.clickSel("#e2e-connection", { clickCount: 2 });
  await page.waitFor(`${visibleTab} !== undefined`);

  // The columns of the table named after the cursor come first; Tab accepts.
  await typeAt(page, "SELECT | FROM orders", "tot");
  assert.match((await listWith(page, "total"))[0], /^totalorders$/);
  await page.key("Tab", "Tab", 9);
  await page.waitFor(`${editorText} === 'SELECT total FROM orders'`);

  // The clauses offer the columns of the statement's tables only.
  await typeAt(page, "SELECT * FROM products;\nSELECT * FROM orders WHERE id = 1 AND |", "pl");
  const where = await listWith(page, "placed_at");
  assert.ok(!where.some((o) => o.startsWith("price")), `no column of another statement: ${where}`);
  await typeAt(page, "UPDATE orders SET |", "cust");
  await listWith(page, "customer_id");

  // Aliases and table names in any case.
  await typeAt(page, "SELECT * FROM orders o JOIN customers AS c ON |", "c.");
  assert.deepEqual((await listWith(page, "id")).filter((o) => !o.includes("=")).sort(), ["Full Name", "id", "name"]);
  await typeAt(page, "SELECT * FROM Orders WHERE |", "Orders.");
  await listWith(page, "placed_at");

  // CTEs and subqueries in FROM have the columns of their select list; no snippets after a dot.
  await typeAt(page, "WITH x AS (SELECT id, total AS amount FROM orders) SELECT | FROM x", "x.");
  assert.deepEqual((await listWith(page, "amount")).sort(), ["amount", "id"]);
  await typeAt(page, "SELECT | FROM (SELECT sku, price AS p FROM products) s", "s.");
  assert.deepEqual((await listWith(page, "p")).sort(), ["p", "sku"]);

  // Names that need quotes are quoted on insert.
  await typeAt(page, "SELECT * FROM |", "My");
  await listWith(page, "My Table");
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`${editorText} === 'SELECT * FROM "My Table"'`);
  await typeAt(page, "SELECT | FROM customers", "Ful");
  await listWith(page, "Full Name");
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`${editorText} === 'SELECT "Full Name" FROM customers'`);

  // A function is listed once, not again as a keyword.
  await typeAt(page, "SELECT |", "cou");
  const count = (await listWith(page, "count")).filter((o) => /^count/i.test(o));
  assert.equal(count.length, 1, `${count}`);

  // No list in strings and comments.
  await typeAt(page, "SELECT '|'", "cou");
  assert.deepEqual(await listAfter(page), []);
  await typeAt(page, "-- |", "sel");
  assert.deepEqual(await listAfter(page), []);

  // Enter after a word typed in full is a newline, not the entry it already is.
  await typeAt(page, "SELECT * FROM |", "orders");
  await listWith(page, "orders");
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`${editorText} === 'SELECT * FROM orders\\n'`);
}
