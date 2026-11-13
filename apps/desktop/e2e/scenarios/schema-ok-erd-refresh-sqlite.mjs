// Regression guard (passes): the ER diagram of a SQLite schema with a self-reference, a
// composite key, odd and accented names and a view (not drawn); a custom diagram of chosen
// tables with a note and zoom that survive a Refresh after DDL run outside (a chosen table
// dropped, a table added), the picker listing the new table.
import assert from "node:assert/strict";
import { DST, ENGINES, SRC, buttonIn, openPath, rowMenu, saveEngines, visibleErd } from "../schema-compare-helpers.mjs";
import { CTRL } from "../cdp.mjs";

const picker = `document.querySelector('.modal.erd-tables')`;
const tables = `[...${visibleErd}.querySelectorAll('.erd-table > text')].map(t => t.textContent).sort()`;
const lines = `${visibleErd}.querySelectorAll('path[marker-start]').length`;

export function setup({ dir }) {
  ENGINES.sqlite.seed(dir, SRC, `CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT NOT NULL);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id));
CREATE TABLE employees (id INTEGER PRIMARY KEY, manager_id INTEGER REFERENCES employees(id));
CREATE TABLE parts (a INTEGER, b INTEGER, PRIMARY KEY (a, b));
CREATE TABLE part_refs (id INTEGER PRIMARY KEY, a INTEGER, b INTEGER, FOREIGN KEY (a, b) REFERENCES parts(a, b));
CREATE TABLE "Ordini è <x>" ("select" TEXT, "Weird Col" INTEGER);
CREATE VIEW v_orders AS SELECT * FROM orders;`);
  ENGINES.sqlite.seed(dir, DST, "");
  saveEngines(dir, ["sqlite"]);
}

export default async function (page, { dir }) {
  await openPath(page, ["QA3 SQLite src", "main"]);
  await rowMenu(page, ["QA3 SQLite src"], "main", "ER diagram");
  await page.waitFor(`${visibleErd}?.querySelectorAll('.erd-table').length === 6`, 20_000);
  assert.deepEqual(await page.eval(tables), ["Ordini è <x>", "customers", "employees", "orders", "part_refs", "parts"]);
  assert.equal(await page.eval(lines), 3, "orders → customers, employees → itself, part_refs → parts");

  // A custom diagram: three tables, a note, 125%.
  await page.eval(`${buttonIn(visibleErd, "Add tables")}.click()`);
  await page.waitFor(`${picker} !== null`);
  await page.eval(`${buttonIn(picker, "Clear shown")}.click()`);
  for (const t of ["customers", "orders", "employees"]) {
    await page.eval(`[...${picker}.querySelectorAll('label')].find(l => l.textContent === ${JSON.stringify(t)}).querySelector('input').click()`);
  }
  await page.eval(`${buttonIn(picker, "Show")}.click()`);
  await page.waitFor(`${visibleErd}.querySelectorAll('.erd-table').length === 3`, 15_000);
  assert.equal(await page.eval(lines), 2);
  await page.eval(`${buttonIn(visibleErd, "Add note")}.click()`);
  await page.waitFor(`document.querySelector('.modal textarea') !== null`);
  await page.eval(`document.querySelector('.modal textarea').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type("keep me");
  await page.eval(`${buttonIn("document.querySelector('.modal')", "Save")}.click()`);
  await page.waitFor(`${visibleErd}.querySelector('.erd-note')?.textContent.includes('keep me')`);
  await page.eval(`${buttonIn(visibleErd, "Zoom in")}.click()`);
  await page.waitFor(`${visibleErd}.textContent.includes('125%')`);

  // DDL outside the app, then Refresh.
  ENGINES.sqlite.exec(dir, SRC, `DROP VIEW v_orders; DROP TABLE orders; CREATE TABLE invoices (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id));`);
  await page.eval(`${buttonIn(visibleErd, "Refresh")}.click()`);
  await page.waitFor(`${visibleErd}.querySelectorAll('.erd-table').length === 2`, 15_000);
  assert.deepEqual(await page.eval(tables), ["customers", "employees"]);
  assert.equal(await page.eval(`${visibleErd}.querySelectorAll('.notice.error').length`), 0);
  assert.ok(await page.eval(`${visibleErd}.querySelector('.erd-note')?.textContent.includes('keep me')`), "the note survives");
  assert.ok(await page.eval(`${visibleErd}.textContent.includes('125%')`), "the zoom survives");
  await page.eval(`${buttonIn(visibleErd, "Add tables")}.click()`);
  await page.waitFor(`${picker} !== null`);
  const listed = await page.eval(`[...${picker}.querySelectorAll('label')].map(l => l.textContent)`);
  assert.ok(listed.includes("invoices") && !listed.includes("orders"), JSON.stringify(listed));
  await page.eval(`${buttonIn(picker, "Cancel")}.click()`);
  await page.waitFor(`${picker} === null`);

}
