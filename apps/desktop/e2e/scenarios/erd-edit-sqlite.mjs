// Group I: the ER diagram in edit mode, on a SQLite schema. The Edit button turns on a bar with
// the pending changes; Add table opens the table form (a name that is blank or taken, no
// column, a column without a name or a type are refused); tables added are drawn with a dashed
// border, changed ones and dropped ones marked; a foreign key is dragged from the circle of a
// column onto a table (onto one with no key, a composite key or a table going to be dropped it
// says why; the same key twice is refused); a double click changes a table, drops it, reverts it
// or keeps a dropped one; the edits survive a reload of the page; Generate DDL opens the script
// in the editor with the tables created in foreign key order, the column added and the table
// dropped, and the script runs; Discard (and leaving the edit mode) ask first; a read-only
// connection has no Edit button.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { DST, ENGINES, SRC, buttonIn, openPath, rowMenu, runEditorScript, saveEngines, queryText, visibleErd } from "../qa3-schema.mjs";

const RO = "QA3 SQLite ro";

export function setup({ dir }) {
  ENGINES.sqlite.seed(dir, SRC, `CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT NOT NULL);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER);
CREATE TABLE legacy (id INTEGER PRIMARY KEY, v TEXT);
CREATE TABLE obsolete (id INTEGER PRIMARY KEY);
CREATE TABLE parts (a INTEGER, b INTEGER, PRIMARY KEY (a, b));
CREATE TABLE loose (v TEXT);
INSERT INTO customers VALUES (1, 'a@example.com');
INSERT INTO orders VALUES (10, 1);
INSERT INTO legacy VALUES (1, 'kept');`);
  ENGINES.sqlite.seed(dir, DST, "");
  const ro = new DatabaseSync(join(dir, "ro.db"));
  ro.exec("CREATE TABLE IF NOT EXISTS t (id INTEGER PRIMARY KEY)");
  ro.close();
  saveEngines(dir, ["sqlite"], [
    { id: "00000000-0000-4000-8000-0000000c3101", name: RO, body: `driver = "sqlite"\ndatabase = '${join(dir, "ro.db")}'\nread_only = true` },
  ]);
}

/** A statement `verb` on the table `name`, quoted or not, with or without its schema. */
const table = (verb, name) => new RegExp(`${verb}\\s+(?:"?main"?\\.)?"?${name}"?(?![\\w])`, "i");

const erd = visibleErd;
const dialog = `document.querySelector('.modal.design-dialog')`;
const confirmBox = `document.querySelector('.modal.confirm')`;
const tabs = `[...document.querySelectorAll('.tab')]`;
const g = (name) => `[...${erd}.querySelectorAll('.erd-table')].find(g => g.querySelector('text').textContent === ${JSON.stringify(name)})`;
const stateOf = (name) => `(${g(name)}?.getAttribute('data-state') ?? null)`;
const pendingText = `(${erd}.querySelector('.erd-pending')?.textContent ?? null)`;
const messageText = `(${erd}.querySelector('.erd-message')?.textContent ?? null)`;
const dialogError = `(${dialog}?.querySelector('.notice.error')?.textContent ?? null)`;
const line = (table, column, target) => `${erd}.querySelector('path[data-fk=${JSON.stringify(`${table}.${column}->${target}`)}]') != null`;
const count = (state) => `${erd}.querySelectorAll('.erd-table[data-state=${JSON.stringify(state)}]').length`;

/** Replaces the text of the input `selector` with `text`. */
async function typeInto(page, selector, text) {
  await page.eval(`(() => { const e = ${selector}; e.focus(); e.select(); })()`);
  await page.type(text);
  await page.waitFor(`${selector}.value === ${JSON.stringify(text)}`);
}

/** Adds a column to the open form: its name and type. */
async function addColumn(page, name, type) {
  const rows = await page.eval(`${dialog}.querySelectorAll('tbody tr').length`);
  await page.eval(`${dialog}.querySelector('button.add-row').click()`);
  await page.waitFor(`${dialog}.querySelectorAll('tbody tr').length === ${rows + 1}`);
  const input = (n) => `${dialog}.querySelector('tbody tr:last-child').querySelectorAll('input')[${n}]`;
  if (name) await typeInto(page, input(0), name);
  if (type) await typeInto(page, input(1), type);
}

const saveForm = (page) => page.eval(`${buttonIn(dialog, "Save")}.click()`);

/** Adds a table with the columns `extra` (name, type) after its `id`. */
async function addTable(page, name, extra = []) {
  await page.eval(`${buttonIn(erd, "Add table")}.click()`);
  await page.waitFor(`${dialog} !== null`);
  await typeInto(page, `${dialog}.querySelector('input[aria-label="Table name"]')`, name);
  for (const [column, type] of extra) await addColumn(page, column, type);
  await saveForm(page);
  await page.waitFor(`${dialog} === null && ${stateOf(name)} === 'new'`);
}

/** Opens the form of a table with a double click on its head. */
async function openForm(page, name) {
  const at = await page.eval(`(() => { const b = ${g(name)}.querySelector('rect').getBoundingClientRect(); return { x: b.x + 20, y: b.y + 12 }; })()`);
  await page.click(at.x, at.y);
  await page.click(at.x, at.y);
}

/** Drags from the circle of `column` in `table` onto the box of `target`. */
async function dragKey(page, table, column, target) {
  const handle = `${erd}.querySelector('circle.erd-handle[data-table=${JSON.stringify(table)}][data-column=${JSON.stringify(column)}]')`;
  await page.eval(`${handle}.scrollIntoView({ block: 'center', inline: 'center' })`);
  const p = await page.eval(`(() => {
    const c = ${handle}.getBoundingClientRect();
    const t = ${g(target)}.querySelector('rect').getBoundingClientRect();
    return { sx: c.x + c.width / 2, sy: c.y + c.height / 2, tx: t.x + t.width / 2, ty: t.y + t.height / 2, w: innerWidth, h: innerHeight };
  })()`);
  assert.ok(p.ty > 0 && p.ty < p.h && p.tx > 0 && p.tx < p.w, `the table ${target} is out of the window: ${JSON.stringify(p)}`);
  await page.mouse("mousePressed", p.sx, p.sy);
  // The press clears a message of the diagram, and the canvas moves up: the target is measured again.
  await page.sleep(150);
  const t = await page.eval(`(() => { const r = ${g(target)}.querySelector('rect').getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`);
  await page.mouse("mouseMoved", (p.sx + t.x) / 2, (p.sy + t.y) / 2, { buttons: 1 });
  await page.mouse("mouseMoved", t.x, t.y, { buttons: 1 });
  await page.mouse("mouseReleased", t.x, t.y);
}

/** A drag that adds nothing: the message the diagram shows says why. */
async function refusedDrag(page, table, column, target, why) {
  await dragKey(page, table, column, target);
  try {
    await page.waitFor(`${messageText} !== null && ${messageText}.includes(${JSON.stringify(why)})`);
  } catch (e) {
    throw new Error(`${e.message}; message: ${await page.eval(messageText)}; drag ${table}.${column} -> ${target}; ${JSON.stringify(await page.eval(`[...${erd}.querySelectorAll('.erd-table')].map(g => [g.querySelector('text').textContent, JSON.stringify(g.querySelector('rect').getBoundingClientRect())])`))}`);
  }
}

export default async function (page, { dir }) {
  await openPath(page, ["QA3 SQLite src", "main"]);
  await rowMenu(page, ["QA3 SQLite src"], "main", "ER diagram");
  await page.waitFor(`${erd}?.querySelectorAll('.erd-table').length === 6`, 20_000);

  // Not in edit mode: no bar, no handles.
  assert.equal(await page.eval(`${erd}.querySelector('.erd-edit-bar') === null && ${erd}.querySelector('.erd-handle') === null`), true);
  await page.eval(`${buttonIn(erd, "Full screen")}.click()`);
  await page.waitFor(`${erd}.classList.contains('erd-full')`);
  await page.eval(`${buttonIn(erd, "Zoom out")}.click()`);
  await page.waitFor(`${erd}.textContent.includes('75%')`);

  // Edit mode: the bar counts nothing yet, and there is nothing to generate or discard.
  await page.eval(`${buttonIn(erd, "Edit")}.click()`);
  await page.waitFor(`${pendingText} === '0 pending changes'`);
  assert.equal(await page.eval(`${buttonIn(erd, "Generate DDL")}.disabled && ${buttonIn(erd, "Discard changes")}.disabled`), true);
  assert.equal(await page.eval(`${erd}.querySelectorAll('.erd-handle').length`), 2 + 2 + 2 + 1 + 2 + 1, "a handle for every column");

  // The form refuses a table that cannot be created, saying why.
  await page.eval(`${buttonIn(erd, "Add table")}.click()`);
  await page.waitFor(`${dialog} !== null`);
  const name = `${dialog}.querySelector('input[aria-label="Table name"]')`;
  await saveForm(page);
  await page.waitFor(`(${dialogError}) === 'The table needs a name.'`);
  await typeInto(page, name, "Customers");
  await saveForm(page);
  await page.waitFor(`(${dialogError}) === 'The schema has a table with this name.'`);
  await typeInto(page, name, "fresh");
  await page.eval(`${dialog}.querySelector('tbody button[aria-label="Remove"]').click()`);
  await page.waitFor(`${dialog}.querySelectorAll('tbody tr').length === 0`);
  await saveForm(page);
  await page.waitFor(`(${dialogError}) === 'The table needs at least one column.'`);
  await addColumn(page, "", "");
  await saveForm(page);
  await page.waitFor(`(${dialogError}) === 'Every column needs a name.'`);
  await typeInto(page, `${dialog}.querySelector('tbody tr input')`, "a");
  await saveForm(page);
  await page.waitFor(`(${dialogError}) === 'Every column needs a type.'`);
  await page.eval(`${buttonIn(dialog, "Cancel")}.click()`);
  await page.waitFor(`${dialog} === null`);
  assert.equal(await page.eval(count("new")), 0, "nothing was added");
  assert.equal(await page.eval(pendingText), "0 pending changes");

  // Two tables: products, then invoices with a column for the key to products.
  await addTable(page, "products", [["title", "TEXT"]]);
  await page.waitFor(`${pendingText} === '1 pending change'`);
  await addTable(page, "invoices", [["product_id", "INTEGER"]]);
  await page.waitFor(`${pendingText} === '2 pending changes'`);
  assert.equal(await page.eval(count("new")), 2);
  // A table with a name in another case is a duplicate too.
  await page.eval(`${buttonIn(erd, "Add table")}.click()`);
  await page.waitFor(`${dialog} !== null`);
  await typeInto(page, name, "PRODUCTS");
  await saveForm(page);
  await page.waitFor(`(${dialogError}) === 'The schema has a table with this name.'`);
  await page.eval(`${buttonIn(dialog, "Cancel")}.click()`);
  await page.waitFor(`${dialog} === null`);
  // The new tables are drawn apart: dashed border, below the others, tagged.
  assert.equal(await page.eval(`${g("products")}.querySelector('rect').getAttribute('stroke-dasharray')`), "6 3");
  assert.equal(await page.eval(`${g("products")}.querySelector('.erd-tag').textContent`), "new");
  assert.equal(await page.eval(`${g("products")}.getBoundingClientRect().y > ${g("customers")}.getBoundingClientRect().y`), true);

  // A key between two new tables, and one that is refused: onto a table with no key, one with
  // a composite key, and the same key twice.
  await dragKey(page, "invoices", "product_id", "products");
  await page.waitFor(line("invoices", "product_id", "products"));
  assert.equal(await page.eval(pendingText), "2 pending changes", "a key in a new table is no new change");
  await refusedDrag(page, "invoices", "product_id", "loose", "has no primary key");
  await refusedDrag(page, "invoices", "product_id", "parts", "more than one column");
  await refusedDrag(page, "invoices", "product_id", "products", "foreign key from product_id to products already");
  assert.equal(await page.eval(`${erd}.querySelectorAll('path[data-fk]').length`), 1);

  // A key between existing tables: orders changes, with its design loaded first.
  await dragKey(page, "orders", "customer_id", "customers");
  await page.waitFor(`${stateOf("orders")} === 'changed'`);
  await page.waitFor(line("orders", "customer_id", "customers"));
  await page.waitFor(`${pendingText} === '3 pending changes'`);
  assert.equal(await page.eval(count("changed")), 1);
  // Its form is the table's own, with the key named for it; the name of the table is fixed.
  await openForm(page, "orders");
  await page.waitFor(`${dialog} !== null`);
  assert.equal(await page.eval(`${dialog}.querySelector('input[aria-label="Table name"]').readOnly`), true);
  await page.eval(`${buttonIn(dialog, "Foreign keys")}.click()`);
  await page.waitFor(`${dialog}.querySelector('input[aria-label="Foreign key name"]') !== null`);
  assert.equal(await page.eval(`${dialog}.querySelector('input[aria-label="Foreign key name"]').value`), "fk_orders_customer_id");
  // Revert puts the table back as the database has it, and the key goes.
  await page.eval(`${buttonIn(dialog, "Revert")}.click()`);
  await page.waitFor(`${dialog} === null && ${stateOf("orders")} === 'plain'`);
  assert.equal(await page.eval(`${line("orders", "customer_id", "customers")}`), false);
  assert.equal(await page.eval(pendingText), "2 pending changes");
  await dragKey(page, "orders", "customer_id", "customers");
  await page.waitFor(`${stateOf("orders")} === 'changed'`);
  await page.waitFor(`${pendingText} === '3 pending changes'`);
  await refusedDrag(page, "orders", "customer_id", "customers", "foreign key from customer_id to customers already");

  // A column added to an existing table, in its form.
  await openForm(page, "legacy");
  await page.waitFor(`${dialog} !== null`);
  await addColumn(page, "note", "TEXT");
  await saveForm(page);
  await page.waitFor(`${dialog} === null && ${stateOf("legacy")} === 'changed'`);
  await page.waitFor(`${pendingText} === '4 pending changes'`);
  // A form saved without a change leaves the table as it is.
  await openForm(page, "customers");
  await page.waitFor(`${dialog} !== null`);
  await saveForm(page);
  await page.waitFor(`${dialog} === null`);
  assert.equal(await page.eval(stateOf("customers")), "plain");

  // A table dropped: faded, its name struck through, and nothing can be dragged to it.
  await openForm(page, "obsolete");
  await page.waitFor(`${dialog} !== null`);
  await page.eval(`${buttonIn(dialog, "Drop table")}.click()`);
  await page.waitFor(`${dialog} === null && ${stateOf("obsolete")} === 'dropped'`);
  await page.waitFor(`${pendingText} === '5 pending changes'`);
  assert.equal(await page.eval(`${g("obsolete")}.querySelector('text').getAttribute('text-decoration')`), "line-through");
  assert.equal(await page.eval(`${g("obsolete")}.querySelector('.erd-handle') === null`), true, "no handle on a dropped table");
  await refusedDrag(page, "legacy", "v", "obsolete", "is going to be dropped");
  // A double click on it asks whether to keep it: Cancel leaves it dropped, Keep takes it back.
  await openForm(page, "obsolete");
  await page.waitFor(`${confirmBox} !== null`);
  await page.eval(`${buttonIn(confirmBox, "Cancel")}.click()`);
  await page.waitFor(`${confirmBox} === null`);
  assert.equal(await page.eval(stateOf("obsolete")), "dropped");
  await openForm(page, "obsolete");
  await page.waitFor(`${confirmBox} !== null`);
  await page.eval(`${buttonIn(confirmBox, "Keep")}.click()`);
  await page.waitFor(`${stateOf("obsolete")} === 'plain'`);
  await page.waitFor(`${pendingText} === '4 pending changes'`);
  await openForm(page, "obsolete");
  await page.waitFor(`${dialog} !== null`);
  await page.eval(`${buttonIn(dialog, "Drop table")}.click()`);
  await page.waitFor(`${stateOf("obsolete")} === 'dropped'`);
  await page.waitFor(`${pendingText} === '5 pending changes'`);
  assert.deepEqual([await page.eval(count("new")), await page.eval(count("changed")), await page.eval(count("dropped"))], [2, 2, 1]);

  // The edits are kept with the tab: after a reload of the page they are all there.
  await page.waitFor(`(localStorage.getItem('platypus.workspace.v2') ?? '').includes('fk_orders_customer_id')`);
  await page.sleep(600);
  await page.eval(`location.reload()`);
  await page.waitFor(`${erd}?.querySelectorAll('.erd-table').length === 8`, 30_000);
  await page.waitFor(`${pendingText} === '5 pending changes'`);
  assert.deepEqual([await page.eval(count("new")), await page.eval(count("changed")), await page.eval(count("dropped"))], [2, 2, 1]);
  assert.equal(await page.eval(line("invoices", "product_id", "products") + " && " + line("orders", "customer_id", "customers")), true);

  // Generate DDL: the script opens in the editor of the connection.
  await page.eval(`${buttonIn(erd, "Generate DDL")}.click()`);
  await page.waitFor(`document.querySelectorAll('.query-tab').length > 0 && (${queryText}).includes('CREATE TABLE')`, 20_000);
  const sql = await page.eval(queryText);
  const at = (re) => {
    const m = re.exec(sql);
    assert.ok(m, `${re} is not in the script:\n${sql}`);
    return m.index;
  };
  assert.match(sql, /^-- Applies the edits of the ER diagram of main/);
  const products = at(table("CREATE TABLE", "products"));
  const invoices = at(table("CREATE TABLE", "invoices"));
  assert.ok(products < invoices, `products is created before invoices, which references it:\n${sql}`);
  assert.match(sql, table("REFERENCES", "products"));
  at(/ADD COLUMN\s+"?note"?/i);
  const dropped = at(table("DROP TABLE", "obsolete"));
  assert.ok(dropped > invoices, `the table is dropped after the others are created:\n${sql}`);
  assert.doesNotMatch(sql, table("DROP TABLE", "(?:customers|loose|parts)"));

  // The script runs, and leaves the database as drawn, the rows kept.
  assert.equal(await runEditorScript(page), null, `the script fails:\n${sql}`);
  const db = new DatabaseSync(join(dir, `${SRC}.db`), { readOnly: true });
  const names = db.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").all().map((r) => r.name);
  assert.deepEqual(names, ["customers", "invoices", "legacy", "loose", "orders", "parts", "products"]);
  assert.ok(db.prepare("PRAGMA table_info(legacy)").all().some((c) => c.name === "note"));
  assert.deepEqual(db.prepare("SELECT id, v FROM legacy").all().map((r) => ({ ...r })), [{ id: 1, v: "kept" }]);
  assert.equal(db.prepare("PRAGMA foreign_key_list(orders)").all()[0]?.table, "customers");
  assert.equal(db.prepare("PRAGMA foreign_key_list(invoices)").all()[0]?.table, "products");
  assert.deepEqual(db.prepare("SELECT id, customer_id FROM orders").all().map((r) => ({ ...r })), [{ id: 10, customer_id: 1 }]);
  db.close();

  // Back on the diagram, the edits are still there until they are discarded; Refresh reads the
  // tables the script made.
  await page.eval(`${tabs}[0].click()`);
  await page.waitFor(`${erd}?.querySelector('.erd-edit-bar') != null`);
  assert.equal(await page.eval(pendingText), "5 pending changes");
  await page.eval(`${buttonIn(erd, "Refresh")}.click()`);
  await page.waitFor(`${erd}.querySelectorAll('.erd-table').length === 7`, 15_000);
  assert.equal(await page.eval(`${erd}.querySelectorAll('.notice.error').length`), 0);

  // Discard asks first: Cancel keeps the edits, Discard clears them (the mode stays on).
  await page.eval(`${buttonIn(erd, "Discard changes")}.click()`);
  await page.waitFor(`${confirmBox} !== null`);
  await page.eval(`${buttonIn(confirmBox, "Cancel")}.click()`);
  await page.waitFor(`${confirmBox} === null`);
  assert.equal(await page.eval(pendingText), "5 pending changes");
  await page.eval(`${buttonIn(erd, "Discard changes")}.click()`);
  await page.waitFor(`${confirmBox} !== null`);
  await page.eval(`${buttonIn(confirmBox, "Discard")}.click()`);
  await page.waitFor(`${pendingText} === '0 pending changes'`);
  assert.deepEqual([await page.eval(count("new")), await page.eval(count("changed")), await page.eval(count("dropped"))], [0, 0, 0]);
  assert.equal(await page.eval(`${buttonIn(erd, "Generate DDL")}.disabled`), true);

  // Leaving the edit mode with edits pending asks too; without any it just leaves.
  await addTable(page, "scratch");
  await page.waitFor(`${pendingText} === '1 pending change'`);
  await page.eval(`${buttonIn(erd, "Edit")}.click()`);
  await page.waitFor(`${confirmBox} !== null`);
  await page.eval(`${buttonIn(confirmBox, "Cancel")}.click()`);
  await page.waitFor(`${confirmBox} === null`);
  assert.equal(await page.eval(`${erd}.querySelector('.erd-edit-bar') !== null && ${stateOf("scratch")} === 'new'`), true);
  await page.eval(`${buttonIn(erd, "Edit")}.click()`);
  await page.waitFor(`${confirmBox} !== null`);
  await page.eval(`${buttonIn(confirmBox, "Discard")}.click()`);
  await page.waitFor(`${erd}.querySelector('.erd-edit-bar') === null`);
  assert.equal(await page.eval(`${g("scratch")} === undefined && ${erd}.querySelector('.erd-handle') === null`), true);
  await page.eval(`${buttonIn(erd, "Edit")}.click()`);
  await page.waitFor(`${erd}.querySelector('.erd-edit-bar') !== null`);
  await page.eval(`${buttonIn(erd, "Edit")}.click()`);
  await page.waitFor(`${erd}.querySelector('.erd-edit-bar') === null`);
  assert.equal(await page.eval(`${confirmBox}`), null, "nothing pending: no question");

  // A read-only connection has no edit mode.
  await openPath(page, [RO, "main"]);
  await rowMenu(page, [RO], "main", "ER diagram");
  await page.waitFor(`${erd}?.querySelectorAll('.erd-table').length === 1`, 20_000);
  assert.equal(await page.eval(`${buttonIn(erd, "Edit")} === undefined && ${erd}.querySelector('.erd-edit-bar') === null`), true);
}
