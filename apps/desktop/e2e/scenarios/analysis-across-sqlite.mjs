// Query across databases (group M) on two SQLite files:
// - "Query across databases…" on a table of the navigator opens the dialog with it chosen, and
//   gives the copy the default name `<connection>_<table>`;
// - a second source on another connection, with a filter, is added in the dialog;
// - a name that is empty or already taken (ignoring case) is refused before anything is read;
// - a failing filter is reported for its own source and nothing opens; a filter that is more
//   than a condition is refused and changes nothing in the database;
// - "Open" copies both tables into one DuckDB and opens an editor on it, with the tables
//   listed in a comment; a join of the two tables returns the rows of the filtered copy;
// - "Reload sources" copies the tables again (the join runs again on the new rows), and a
//   source that fails on reload is named and leaves the tables as they were;
// - the palette's command opens the dialog without a table chosen, so Open is disabled.
import assert from "node:assert/strict";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { expand, treeMenu } from "../grid-helpers.mjs";
import { setField, palette } from "../plans.mjs";
import { gridTsv, rerun, runQuery, saveConnections, sqliteExec, sqliteFile, status, visibleTab } from "../analysis-helpers.mjs";

const SHOP = "E2E across shop";
const CRM = "E2E across crm";
const SHOP_ID = "00000000-0000-4000-8000-0000000c0001";
const CRM_ID = "00000000-0000-4000-8000-0000000c0002";

const dialog = ".modal.across";
const row = (i, label) => `${dialog} .across-row:nth-child(${i + 1}) [aria-label=${JSON.stringify(label)}]`;
const valueOf = (selector) => `(document.querySelector(${JSON.stringify(selector)})?.value ?? null)`;
const optionsOf = (selector) => `[...(document.querySelector(${JSON.stringify(selector)})?.options ?? [])].map(o => o.value)`;
const open = `document.querySelector('${dialog} [data-action=open]')`;
const dialogError = `(document.querySelector('${dialog} .notice.error:not(.across-problem)')?.textContent ?? '')`;
const problem = (i) => `(document.querySelector('${dialog} .across-row:nth-child(${i + 1}) .across-problem')?.textContent ?? '')`;
const bar = `(${visibleTab}?.querySelector('.across-bar')?.textContent ?? '')`;
const queryTabs = `document.querySelectorAll('.query-tab').length`;

export function setup({ dir }) {
  sqliteFile(
    dir,
    "shop.db",
    `CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER, total INTEGER);
     INSERT INTO orders VALUES (1, 1, 250), (2, 2, 40), (3, 1, 120), (4, 3, 500), (5, 2, 100);`,
  );
  sqliteFile(
    dir,
    "crm.db",
    `CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT, country TEXT);
     INSERT INTO customers VALUES (1, 'Ada', 'UK'), (2, 'Bo', 'SE'), (3, 'Cy', 'US');`,
  );
  saveConnections(dir, [
    { id: SHOP_ID, name: SHOP, driver: "sqlite", database: join(dir, "shop.db") },
    { id: CRM_ID, name: CRM, driver: "sqlite", database: join(dir, "crm.db") },
  ]);
}

/** Waits for the dialog's source `i` to have `table` chosen on `schema`. */
async function loaded(page, i, table) {
  await page.waitFor(`${optionsOf(row(i, "Table"))}.includes(${JSON.stringify(table)})`, 15_000);
  await page.waitFor(`${valueOf(row(i, "Table"))} === ${JSON.stringify(table)}`);
}

/** Clicks Open and waits until the dialog is busy no more: it closed, or it shows an error
 *  (an old error is cleared when Open starts). */
async function pressOpen(page) {
  await page.eval(`${open}.click()`);
  await page.waitFor(`document.querySelector('${dialog}') === null || (${dialogError} !== '' && !${open}.disabled)`, 30_000);
}

export default async function (page, { dir }) {
  await expand(page, SHOP, null, "orders");
  await expand(page, CRM, null, "customers");

  // The table of the navigator is chosen, named after its connection.
  await treeMenu(page, "orders", "Query across databases");
  await page.waitFor(`document.querySelector('${dialog}') !== null`);
  await loaded(page, 0, "orders");
  assert.equal(await page.eval(valueOf(row(0, "Connection"))), SHOP_ID);
  assert.equal(await page.eval(valueOf(row(0, "Schema"))), "main");
  assert.equal(await page.eval(valueOf(row(0, "Name in DuckDB"))), "e2e_across_shop_orders");

  // A second source on the other connection, with a filter on the first.
  await page.eval(`document.querySelector('${dialog} [data-action=add-source]').click()`);
  await page.waitFor(`document.querySelectorAll('${dialog} .across-row').length === 2`);
  await setField(page, row(1, "Connection"), CRM_ID);
  await page.waitFor(`${optionsOf(row(1, "Table"))}.includes('customers')`, 15_000);
  await setField(page, row(1, "Table"), "customers");
  await page.waitFor(`${valueOf(row(1, "Name in DuckDB"))} === 'e2e_across_crm_customers'`);
  await setField(page, row(0, "Name in DuckDB"), "orders");
  await setField(page, row(1, "Name in DuckDB"), "customers");
  assert.equal(await page.eval(`${open}.disabled`), false);

  // A taken name (ignoring case) and an empty one are refused, with the reason beside the row.
  await setField(page, row(1, "Name in DuckDB"), "ORDERS");
  await page.waitFor(`${problem(1)} === 'Another table has this name.'`);
  assert.equal(await page.eval(`${open}.disabled`), true);
  await setField(page, row(1, "Name in DuckDB"), "  ");
  await page.waitFor(`${problem(1)} === 'The table needs a name.'`);
  assert.equal(await page.eval(`${open}.disabled`), true);
  await setField(page, row(1, "Name in DuckDB"), "customers");
  await page.waitFor(`${problem(1)} === ''`);
  assert.equal(await page.eval(`${open}.disabled`), false);

  // A filter that fails names its source; the dialog stays and no tab opens.
  const tabs = await page.eval(queryTabs);
  await setField(page, row(0, "Filter (WHERE)"), "nope = 1");
  await pressOpen(page);
  const failed = await page.eval(dialogError);
  assert.ok(failed.startsWith(`${SHOP} · main.orders: `), failed);
  assert.ok(failed.includes("nope"), failed);
  assert.equal(await page.eval(queryTabs), tabs);

  // A filter that is more than a condition is refused, and the rows stay.
  await setField(page, row(0, "Filter (WHERE)"), "1 = 1; DELETE FROM orders");
  await pressOpen(page);
  const refused = await page.eval(dialogError);
  assert.ok(refused.startsWith(`${SHOP} · main.orders: `), refused);
  assert.ok(refused.includes("single condition"), refused);
  const shop = new DatabaseSync(join(dir, "shop.db"), { timeout: 5_000 });
  assert.equal(shop.prepare("SELECT count(*) AS n FROM orders").get().n, 5);
  shop.close();

  // The right filter opens the editor on the copies.
  await setField(page, row(0, "Filter (WHERE)"), "total >= 100");
  await pressOpen(page);
  assert.equal(await page.eval(`document.querySelector('${dialog}')`), null);
  await page.waitFor(`${visibleTab}?.querySelector('.across-bar') != null`, 15_000);
  const text = await page.eval(bar);
  assert.ok(text.includes(`orders ← ${SHOP} · main.orders (total >= 100)`), text);
  assert.ok(text.includes(`customers ← ${CRM} · main.customers`), text);
  const sql = await page.eval(`[...${visibleTab}.querySelectorAll('.editor .cm-line')].map(l => l.textContent).join('\\n')`);
  assert.ok(sql.includes("--   orders(id, customer_id, total)"), sql);
  assert.ok(sql.includes("--   customers(id, name, country)"), sql);
  assert.ok(sql.endsWith("SELECT * FROM orders LIMIT 100"), sql);

  // Run on its own, the first query shows the filtered copy; a join shows both databases.
  await runQuery(page, "SELECT * FROM orders ORDER BY id");
  assert.equal(await page.eval(status), "4 rows");
  const join_ = "SELECT o.id, c.name, o.total FROM orders o JOIN customers c ON c.id = o.customer_id ORDER BY o.id";
  await runQuery(page, join_);
  assert.equal(await page.eval(status), "4 rows");
  assert.equal(await gridTsv(page), "id\tname\ttotal\n1\tAda\t250\n3\tAda\t120\n4\tCy\t500\n5\tBo\t100");

  // The databases change; Reload sources copies them again and the query runs on the new rows.
  sqliteExec(join(dir, "shop.db"), "INSERT INTO orders VALUES (6, 3, 900), (7, 1, 5)");
  sqliteExec(join(dir, "crm.db"), "UPDATE customers SET name = 'Cyrus' WHERE id = 3");
  await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
  await page.eval(`${visibleTab}.querySelector('[data-action=reload-sources]').click()`);
  await page.waitFor(`${status} === '5 rows'`, 30_000);
  assert.equal(
    await gridTsv(page),
    "id\tname\ttotal\n1\tAda\t250\n3\tAda\t120\n4\tCyrus\t500\n5\tBo\t100\n6\tCyrus\t900",
  );

  // A source that fails on reload is named; the tables stay as they were.
  sqliteExec(join(dir, "crm.db"), "ALTER TABLE customers RENAME TO clients");
  await page.eval(`${visibleTab}.querySelector('[data-action=reload-sources]').click()`);
  await page.waitFor(`(document.querySelector('.toast')?.textContent ?? '').includes(${JSON.stringify(`${CRM} · main.customers: `)})`, 30_000);
  await page.eval(`document.querySelector('.toast').click()`);
  await rerun(page);
  assert.equal(await page.eval(status), "5 rows");
  assert.ok((await page.eval(bar)).includes("Reload sources"));

  // The palette's command opens the dialog with nothing chosen: Open waits for a table.
  await palette(page, "Query across databases");
  await page.waitFor(`document.querySelector('${dialog}') !== null`);
  assert.equal(await page.eval(`${open}.disabled`), true);
  await page.eval(`document.querySelector('${dialog} [data-action=cancel]').click()`);
  await page.waitFor(`document.querySelector('${dialog}') === null`);
}
