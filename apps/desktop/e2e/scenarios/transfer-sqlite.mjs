// Transfers between files: a SQLite schema copied into another SQLite database (structure,
// rows, defaults, indexes, keys, CHECKs and its view), every table exported to a folder, and a
// pasted CSV imported as a new table. The folder picker and the clipboard are the system's:
// the test answers those two through `window.__platypus.answer`, the rest runs as usual.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const SOURCE = "Shop db";
const TARGET = "Copy db";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const button = (scope, text) => `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;

const connection = (id, name, file) => `
[[connections]]
id = "00000000-0000-4000-8000-00000000003${id}"
name = "${name}"
driver = "sqlite"
database = '${file}'
connection_type = "dev"
save_password = false
`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "shop.db"));
  db.exec(`
    CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, code TEXT DEFAULT 'new' UNIQUE);
    CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers (id),
                         qty INTEGER CHECK (qty > 0));
    CREATE INDEX orders_customer ON orders (customer_id);
    CREATE VIEW big_orders AS SELECT * FROM orders WHERE qty > 5;
    INSERT INTO customers (name, code) VALUES ('Ada', 'a'), ('Alan', 'b');
    INSERT INTO orders (customer_id, qty) VALUES (1, 3), (2, 10);
  `);
  db.close();
  new DatabaseSync(join(dir, "copy.db")).close();
  writeFileSync(join(dir, "pasted.csv"), "city,people\nMilano,1371498\nTorino,841600\n");
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1\n${connection(1, SOURCE, join(dir, "shop.db"))}${connection(2, TARGET, join(dir, "copy.db"))}`,
  );
}

async function menu(page, label, item, modal) {
  await page.eval(`${row(label)}.querySelector('.tree-label').id = 'e2e-row'`);
  await page.clickSel("#e2e-row", { button: "right" });
  await page.eval(`document.getElementById('e2e-row')?.removeAttribute('id')`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(
    `[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.startsWith(${JSON.stringify(item)})).click()`,
  );
  await page.waitFor(`${modal} !== null`);
}

/** Answers `what` (the folder picker, the clipboard) with `value` from now on. */
const answer = (page, what, value) => page.eval(`window.__platypus.answer(${JSON.stringify(what)}, ${JSON.stringify(value)})`);

/** Clicks `action` in `modal` and waits for the job to end with `done`. */
async function runJob(page, modal, action, done) {
  await page.waitFor(`${button(modal, action)} && !${button(modal, action)}.disabled`, 15_000);
  await page.eval(`${button(modal, action)}.click()`);
  await page.waitFor(`${modal}.textContent.includes(${JSON.stringify(done)})`, 30_000);
}

export default async function (page, { dir }) {
  await page.waitFor(`${row(SOURCE)} !== undefined`);
  await page.eval(`${row(SOURCE)}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("main")} !== undefined`, 15_000);

  // The whole schema into the other database: SQLite to SQLite also brings the view.
  const migrate = `document.querySelector('.modal.migrate')`;
  await menu(page, "main", "Copy schema to a database", migrate);
  await page.waitFor(`${migrate}.querySelector('select')?.selectedOptions[0]?.textContent === ${JSON.stringify(TARGET)}`, 15_000);
  await runJob(page, migrate, "Copy", "Copied 4 rows");
  // Between databases of one kind nothing stays behind.
  assert.equal(await page.eval(`${migrate}.querySelector('.migrate-warnings')?.textContent ?? null`), null);
  await page.eval(`${button(migrate, "Close")}.click()`);

  const copy = new DatabaseSync(join(dir, "copy.db"));
  const all = (sql) => copy.prepare(sql).all().map((r) => ({ ...r }));
  assert.deepEqual(all("SELECT id, name, code FROM customers ORDER BY id"), [
    { id: 1, name: "Ada", code: "a" },
    { id: 2, name: "Alan", code: "b" },
  ]);
  // Identity and default: a new customer is numbered after the copied ones.
  copy.exec("INSERT INTO customers (name) VALUES ('Grace')");
  assert.deepEqual(all("SELECT id, code FROM customers WHERE name = 'Grace'"), [{ id: 3, code: "new" }]);
  assert.throws(() => copy.exec("INSERT INTO customers (name, code) VALUES ('Dup', 'a')"), /UNIQUE/);
  assert.throws(() => copy.exec("INSERT INTO orders (customer_id, qty) VALUES (1, 0)"), /CHECK/);
  copy.exec("PRAGMA foreign_keys = ON");
  assert.throws(() => copy.exec("INSERT INTO orders (customer_id, qty) VALUES (99, 1)"), /FOREIGN KEY/);
  assert.deepEqual(all("SELECT name FROM sqlite_master WHERE name = 'orders_customer'"), [{ name: "orders_customer" }]);
  assert.deepEqual(all("SELECT id, qty FROM big_orders"), [{ id: 2, qty: 10 }]);
  copy.close();

  // Every table of the schema, one CSV each, in the folder the picker answers.
  const out = join(dir, "out");
  mkdirSync(out);
  await answer(page, "folder", out);
  const tables = `document.querySelector('.modal.export-tables')`;
  await menu(page, "main", "Export all tables", tables);
  await page.waitFor(`${tables}.textContent.includes('customers, orders')`, 15_000);
  await runJob(page, tables, "Export to folder…", "Exported");
  await page.eval(`${button(tables, "Close")}.click()`);
  assert.equal(readFileSync(join(out, "customers.csv"), "utf8").replaceAll("\r\n", "\n"), "id,name,code\n1,Ada,a\n2,Alan,b\n");
  assert.ok(existsSync(join(out, "orders.csv")));

  // A CSV pasted as a new table, named after the file the clipboard is saved to.
  await answer(page, "clipboard", join(dir, "pasted.csv"));
  const imp = `document.querySelector('.modal.import-data')`;
  await menu(page, "main", "Paste CSV as a new table", imp);
  await page.waitFor(`${imp}.querySelectorAll('.import-preview tbody tr').length === 2`, 15_000);
  await runJob(page, imp, "Import", "Imported 2 rows");
  await page.eval(`${button(imp, "Close")}.click()`);
  await page.waitFor(`${imp} === null`);
  const shop = new DatabaseSync(join(dir, "shop.db"));
  assert.deepEqual({ ...shop.prepare("SELECT count(*) AS n, max(people) AS top FROM pasted").get() }, { n: 2, top: 1371498 });
  shop.close();
  // Not a file of the clipboard's: it stays.
  assert.ok(existsSync(join(dir, "pasted.csv")));
}
