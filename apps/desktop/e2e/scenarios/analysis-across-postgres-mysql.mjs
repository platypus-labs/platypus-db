// Query across databases (group S) between two network servers: a PostgreSQL table and a MySQL
// table copied into one in-memory DuckDB, and joined there.
// - the dialog opens from the navigator on the PostgreSQL table, a second source is a MySQL
//   table (its database is the schema), and "Open" lists both copies in the bar;
// - a join returns what both servers hold: NULL customer ids (a LEFT JOIN keeps them), UTF-8
//   names (utf8mb4), dates, decimals, and an aggregate by country;
// - errors: a table that is not among the copies, a join of text with integers, a filter that
//   fails on the MySQL side (named with its source), a source with no table chosen;
// - the databases change (a row more, a table dropped): "Reload sources" reads again, and a
//   source that is gone is named and leaves the copies as they were;
// - the memory limit (512 MB): a PostgreSQL view that would hold more is cut, and the bar says
//   which table is partial.
import assert from "node:assert/strict";
import { expand, needs, sh, treeMenu } from "../qa-grid.mjs";
import { openPath } from "../qa-dbs.mjs";
import { setField } from "../plans.mjs";
import { gridTsv, runQuery, saveConnections, status, visibleTab } from "../qa-ana.mjs";

const PG = "E2E across pg";
const MY = "E2E across my";
const PG_ID = "00000000-0000-4000-8000-0000000c0011";
const MY_ID = "00000000-0000-4000-8000-0000000c0012";

export const skip = needs("pg", "mysql");

const dialog = ".modal.across";
const row = (i, label) => `${dialog} .across-row:nth-child(${i + 1}) [aria-label=${JSON.stringify(label)}]`;
const valueOf = (selector) => `(document.querySelector(${JSON.stringify(selector)})?.value ?? null)`;
const optionsOf = (selector) => `[...(document.querySelector(${JSON.stringify(selector)})?.options ?? [])].map(o => o.value)`;
const open = `document.querySelector('${dialog} [data-action=open]')`;
const dialogError = `(document.querySelector('${dialog} .notice.error:not(.across-problem)')?.textContent ?? '')`;
const bar = `(${visibleTab}?.querySelector('.across-bar')?.textContent ?? '')`;
const queryTabs = `document.querySelectorAll('.query-tab').length`;
const resultError = `(${visibleTab}?.querySelector('.result-error')?.textContent ?? '')`;

export function setup({ dir }) {
  sh(
    "pg",
    `DROP VIEW IF EXISTS demo.e2e_across_big;
     DROP TABLE IF EXISTS demo.e2e_across_orders;
     CREATE TABLE demo.e2e_across_orders (id int PRIMARY KEY, customer_id int, total numeric(10,2), placed date, note text);
     INSERT INTO demo.e2e_across_orders VALUES
       (1, 1, 250.50, '2026-01-05', 'first'), (2, 2, 40.00, '2026-02-10', NULL),
       (3, 1, 120.25, '2026-03-15', 'again'), (4, 3, 500.00, '2026-04-20', 'Zoë''s'),
       (5, NULL, 9.99, '2026-05-25', 'no customer');
     CREATE VIEW demo.e2e_across_big AS SELECT g AS id, repeat('x', 400) AS pad FROM generate_series(1, 4000000) g;`,
  );
  sh(
    "mysql",
    `DROP TABLE IF EXISTS e2e_across_customers;
     CREATE TABLE e2e_across_customers (id int PRIMARY KEY, name varchar(40), country char(2));
     INSERT INTO e2e_across_customers VALUES (1, 'Ada', 'UK'), (2, 'Bo', 'SE'), (3, 'Zoë', 'US'), (4, 'Dee', 'UK');`,
  );
  saveConnections(dir, [
    { id: PG_ID, name: PG, driver: "postgres", host: "localhost", port: 55432, database: "platypus", user: "platypus", password: "platypus" },
    { id: MY_ID, name: MY, driver: "mysql", host: "localhost", port: 53306, database: "platypus", user: "platypus", password: "platypus" },
  ]);
}

async function loaded(page, i, table) {
  await page.waitFor(`${optionsOf(row(i, "Table"))}.includes(${JSON.stringify(table)})`, 15_000);
  await page.waitFor(`${valueOf(row(i, "Table"))} === ${JSON.stringify(table)}`);
}

async function pressOpen(page, timeout = 60_000) {
  await page.eval(`${open}.click()`);
  await page.waitFor(`document.querySelector('${dialog}') === null || (${dialogError} !== '' && !${open}.disabled)`, timeout);
}

/** Runs `sql` in the editor and returns the status or, when it failed, the error shown. */
async function attempt(page, sql) {
  await runQuery(page, sql);
  return page.eval(resultError);
}

export default async function (page) {
  try {
    await expand(page, PG);
    await expand(page, MY);
    await openPath(page, [PG, "platypus", "demo", "Tables", "e2e_across_orders"]);

    // The PostgreSQL table, chosen from the navigator; the schema is `demo`.
    await treeMenu(page, "e2e_across_orders", "Query across databases");
    await page.waitFor(`document.querySelector('${dialog}') !== null`);
    await loaded(page, 0, "e2e_across_orders");
    assert.equal(await page.eval(valueOf(row(0, "Connection"))), PG_ID);
    assert.equal(await page.eval(valueOf(row(0, "Schema"))), "demo");

    // A second source: the MySQL table (its database is the schema). A source with no table
    // chosen cannot be opened.
    await page.eval(`document.querySelector('${dialog} [data-action=add-source]').click()`);
    await page.waitFor(`document.querySelectorAll('${dialog} .across-row').length === 2`);
    await setField(page, row(1, "Connection"), MY_ID);
    assert.equal(await page.eval(valueOf(row(1, "Table"))), "", "no table chosen yet");
    assert.equal(await page.eval(`${open}.disabled`), true);
    await page.waitFor(`${optionsOf(row(1, "Table"))}.includes('e2e_across_customers')`, 15_000);
    await setField(page, row(1, "Table"), "e2e_across_customers");
    assert.equal(await page.eval(valueOf(row(1, "Schema"))), "platypus");
    await setField(page, row(0, "Name in DuckDB"), "orders");
    await setField(page, row(1, "Name in DuckDB"), "customers");
    assert.equal(await page.eval(`${open}.disabled`), false);

    // A filter the MySQL server refuses names its source; the dialog stays, no tab opens.
    const tabs = await page.eval(queryTabs);
    await setField(page, row(1, "Filter (WHERE)"), "no_such_column = 1");
    await pressOpen(page);
    const failed = await page.eval(dialogError);
    assert.ok(failed.startsWith(`${MY} · platypus.e2e_across_customers: `), failed);
    assert.match(failed, /no_such_column/);
    assert.equal(await page.eval(queryTabs), tabs);
    await setField(page, row(1, "Filter (WHERE)"), "");
    await pressOpen(page);
    assert.equal(await page.eval(`document.querySelector('${dialog}')`), null);
    await page.waitFor(`${visibleTab}?.querySelector('.across-bar') != null`, 15_000);
    const text = await page.eval(bar);
    assert.ok(text.includes(`orders ← ${PG} · demo.e2e_across_orders`), text);
    assert.ok(text.includes(`customers ← ${MY} · platypus.e2e_across_customers`), text);

    // The copies hold what the servers hold: NULLs, dates, decimals and UTF-8.
    await runQuery(page, "SELECT * FROM orders ORDER BY id");
    assert.equal(await page.eval(status), "5 rows");
    assert.equal(
      await gridTsv(page),
      "id\tcustomer_id\ttotal\tplaced\tnote\n1\t1\t250.50\t2026-01-05\tfirst\n2\t2\t40.00\t2026-02-10\t\n3\t1\t120.25\t2026-03-15\tagain\n4\t3\t500.00\t2026-04-20\tZoë's\n5\t\t9.99\t2026-05-25\tno customer",
    );

    // A join across the two servers; the NULL customer drops out of an inner join and stays in a left one.
    await runQuery(page, "SELECT o.id, c.name, o.total FROM orders o JOIN customers c ON c.id = o.customer_id ORDER BY o.id");
    assert.equal(await page.eval(status), "4 rows");
    assert.equal(await gridTsv(page), "id\tname\ttotal\n1\tAda\t250.50\n2\tBo\t40.00\n3\tAda\t120.25\n4\tZoë\t500.00");
    await runQuery(page, "SELECT o.id, c.name FROM orders o LEFT JOIN customers c ON c.id = o.customer_id WHERE c.id IS NULL");
    assert.equal(await gridTsv(page), "id\tname\n5\t");
    await runQuery(
      page,
      "SELECT c.country, count(o.id) AS orders, sum(o.total) AS revenue FROM customers c LEFT JOIN orders o ON o.customer_id = c.id GROUP BY c.country ORDER BY revenue DESC NULLS LAST, c.country",
    );
    assert.equal(await gridTsv(page), "country\torders\trevenue\nUS\t1\t500.00\nUK\t2\t370.75\nSE\t1\t40.00");
    assert.equal(await page.eval(status), "3 rows");

    // Errors: a table that is not among the copies, text compared with integers.
    assert.match(await attempt(page, "SELECT * FROM e2e_across_orders"), /e2e_across_orders|does not exist/i);
    assert.match(await attempt(page, "SELECT * FROM orders o JOIN customers c ON c.name = o.id"), /Conversion|convert|Ada|INT/i);
    assert.match(await attempt(page, "SELECT count(*) FROM orders o JOIN customers c ON c.id = o.no_such_column"), /no_such_column/);
    // The copies survive the errors.
    await runQuery(page, "SELECT count(*) AS n FROM orders");
    assert.equal(await gridTsv(page), "n\n5");

    // The servers change: a row more in each. "Reload sources" copies them again.
    sh("pg", "INSERT INTO demo.e2e_across_orders VALUES (6, 4, 77.70, '2026-06-30', 'dee');");
    sh("mysql", "UPDATE e2e_across_customers SET name = 'Zoe' WHERE id = 3;");
    await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
    await page.eval(`${visibleTab}.querySelector('[data-action=reload-sources]').click()`);
    await page.waitFor(`${status} !== ''`, 30_000);
    await runQuery(page, "SELECT o.id, c.name FROM orders o JOIN customers c ON c.id = o.customer_id WHERE o.id >= 4 ORDER BY o.id");
    assert.equal(await gridTsv(page), "id\tname\n4\tZoe\n6\tDee");

    // A source that is gone on reload is named, and the copies stay as they were.
    sh("mysql", "DROP TABLE e2e_across_customers;");
    await page.eval(`${visibleTab}.querySelector('[data-action=reload-sources]').click()`);
    await page.waitFor(`(document.querySelector('.toast')?.textContent ?? '').includes(${JSON.stringify(`${MY} · platypus.e2e_across_customers: `)})`, 30_000);
    assert.match(await page.eval(`document.querySelector('.toast').textContent`), /doesn't exist|exist/i);
    await page.eval(`document.querySelector('.toast').click()`);
    await runQuery(page, "SELECT count(*) AS n FROM orders");
    assert.equal(await gridTsv(page), "n\n6");

    // The memory limit: a view of about 1.6 GB is read up to 512 MB and cut, and the bar says
    // which table is partial.
    await openPath(page, [PG, "platypus", "demo", "Views", "e2e_across_big"]).catch(() => {});
    if (await page.eval(`[...document.querySelectorAll('.tree-row')].some(r => r.querySelector('.tree-label')?.textContent === 'e2e_across_big')`)) {
      await treeMenu(page, "e2e_across_big", "Query across databases");
      await page.waitFor(`document.querySelector('${dialog}') !== null`);
      await loaded(page, 0, "e2e_across_big");
      await setField(page, row(0, "Name in DuckDB"), "big");
      await pressOpen(page, 240_000);
      assert.equal(await page.eval(`document.querySelector('${dialog}')`), null);
      await page.waitFor(`${visibleTab}?.querySelector('.across-bar') != null`, 15_000);
      assert.match(await page.eval(bar), /Partial: the memory limit stopped the reading of /);
      await runQuery(page, "SELECT count(*) AS n FROM big");
      const rows = Number((await gridTsv(page)).split("\n")[1]);
      assert.ok(rows > 0 && rows < 4_000_000, `cut at ${rows} rows`);
    } else {
      throw new Error("the view is not in the navigator");
    }
  } finally {
    sh("pg", "DROP VIEW IF EXISTS demo.e2e_across_big; DROP TABLE IF EXISTS demo.e2e_across_orders;");
    sh("mysql", "DROP TABLE IF EXISTS e2e_across_customers;");
  }
}
